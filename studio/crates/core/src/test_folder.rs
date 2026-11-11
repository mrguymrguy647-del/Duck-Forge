//! For the Studio's tests only (its own and the app's): a folder of its
//! own for each test.
//!
//! `cargo test` runs a file's tests at the same time, so two tests must
//! never write the same file: one can read the other's half-written text,
//! and on Windows one can't even open a file while another is replacing or
//! deleting it ("Access is denied"). The engine's tests use the same thing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A new, empty folder in the system's temporary folder, for one test.
///
/// Its name has the test's own name, the test program's process number,
/// the time it was made and a count, so no two tests share one, and
/// neither do two runs. It's deleted when the test passes, and kept when
/// it fails, to look at.
pub struct TestFolder(PathBuf);

impl TestFolder {
    pub fn new(name: &str) -> TestFolder {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let made = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos());
        let folder = std::env::temp_dir().join(format!(
            "duck-forge-studio-test-{name}-{}-{made}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder)
            .unwrap_or_else(|error| panic!("can't make {}: {error}", folder.display()));
        TestFolder(folder)
    }

    /// Settings that are saved in this folder, never in the person's own
    /// settings file.
    pub fn settings(&self) -> crate::settings::Settings {
        crate::settings::Settings {
            file: Some(self.0.join("settings.json")),
            ..crate::settings::Settings::default()
        }
    }
}

impl std::ops::Deref for TestFolder {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TestFolder {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

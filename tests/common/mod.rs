//! Shared by the tests in this folder: a folder of its own for each test.
//!
//! `cargo test` runs a file's tests at the same time, so two tests must
//! never write the same file (on Windows one can't even open a file while
//! another is replacing or deleting it). The engine's own tests use the
//! same thing (`src/test_folder.rs`).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A new, empty folder in the system's temporary folder, for one test,
/// named so that no other test or run shares it. Deleted when the test
/// passes, kept when it fails.
pub struct TestFolder(PathBuf);

impl TestFolder {
    pub fn new(name: &str) -> TestFolder {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let made = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos());
        let folder = std::env::temp_dir().join(format!(
            "duckforge-test-{name}-{}-{made}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder)
            .unwrap_or_else(|error| panic!("can't make {}: {error}", folder.display()));
        TestFolder(folder)
    }
}

impl TestFolder {
    /// A private copy of the files in `tests/fixtures/<from>`, in this
    /// folder, so the code being tested never opens a file that other
    /// tests are opening at the same moment.
    pub fn fixture(&self, from: &str) -> PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(from);
        let copy = self.0.join(from);
        std::fs::create_dir_all(&copy).unwrap();
        for entry in std::fs::read_dir(&source).unwrap().flatten() {
            if entry.path().is_file() {
                copy_file(&entry.path(), &copy.join(entry.file_name()));
            }
        }
        copy
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

/// Copies a file by reading it and writing it again, keeping whether it can
/// be run. Not `fs::copy`: on Windows that also brings along the file's
/// attributes (read-only, say) and hidden data (the "downloaded from the
/// internet" mark), and the files here are read by other tests at the same time.
///
/// If Windows says "Access is denied" for a moment (a virus scanner, a
/// backup or OneDrive can hold a file open), it waits and tries again for
/// up to two seconds.
pub fn copy_file(from: &Path, to: &Path) {
    let mut tries = 0;
    let bytes = loop {
        match std::fs::read(from) {
            Ok(bytes) => break bytes,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && tries < 40 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => panic!("can't read {}: {e}", from.display()),
        }
    };
    std::fs::write(to, bytes).unwrap_or_else(|e| panic!("can't write {}: {e}", to.display()));
    #[cfg(unix)]
    {
        let permissions = std::fs::metadata(from).unwrap().permissions();
        std::fs::set_permissions(to, permissions).unwrap();
    }
}

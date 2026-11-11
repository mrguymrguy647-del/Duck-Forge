//! For the engine's tests only: a folder of its own for each test.
//!
//! `cargo test` runs a file's tests at the same time, on several threads of
//! one program. Two tests that write the same temporary file can read each
//! other's half-written text, and on Windows one test can't open a file
//! while another is replacing or deleting it ("Access is denied"). So no
//! test writes anywhere but in a [`TestFolder`] of its own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A new, empty folder in the system's temporary folder, for one test.
///
/// Its name has the test's own name, the test program's process number,
/// the time it was made and a count, so no two tests share one, and
/// neither do two runs (Windows soon gives a process number to another
/// program). It's deleted when the test passes, and kept when it fails,
/// to look at.
pub(crate) struct TestFolder(PathBuf);

impl TestFolder {
    pub(crate) fn new(name: &str) -> TestFolder {
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

    /// A file in the folder with `text` in it.
    pub(crate) fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    /// A private copy of the files in `tests/fixtures/<from>`, in this
    /// folder: the test reads its own copy, so the code being tested never
    /// opens a file that other tests are opening at the same moment.
    pub(crate) fn fixture(&self, from: &str) -> PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(from);
        let copy = self.join(from);
        std::fs::create_dir_all(&copy).unwrap();
        for entry in std::fs::read_dir(&source).unwrap().flatten() {
            if entry.path().is_file() {
                std::fs::write(copy.join(entry.file_name()), read_shared(&entry.path())).unwrap();
            }
        }
        copy
    }
}

/// Reads a file that other tests (or other programs) may be reading too.
///
/// Read and written again, never `fs::copy`'d: on Windows that also copies
/// the file's attributes and hidden data. And if Windows says "Access is
/// denied" for a moment (a virus scanner, a backup or OneDrive can hold a
/// file open), it waits and tries again for up to two seconds.
pub(crate) fn read_shared(path: &Path) -> Vec<u8> {
    let mut tries = 0;
    loop {
        match std::fs::read(path) {
            Ok(bytes) => return bytes,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied && tries < 40 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => panic!("can't read {}: {error}", path.display()),
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

#[test]
fn every_test_folder_is_new_and_goes_away() {
    let (a, b) = (TestFolder::new("same"), TestFolder::new("same"));
    assert_ne!(a.to_path_buf(), b.to_path_buf());
    assert!(a.is_dir() && b.is_dir());
    let file = a.file("note.txt", "hi");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hi");
    let kept = a.to_path_buf();
    drop(a);
    assert!(!kept.exists());
}

#[test]
fn a_fixture_is_a_private_copy() {
    let (a, b) = (TestFolder::new("fixture"), TestFolder::new("fixture"));
    let (one, two) = (a.fixture("quill/scale1"), b.fixture("quill/scale1"));
    assert_ne!(one, two);
    let shared = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quill/scale1");
    for file in ["duck.json", "duck.png"] {
        assert_eq!(
            std::fs::read(one.join(file)).unwrap(),
            std::fs::read(shared.join(file)).unwrap()
        );
    }
}

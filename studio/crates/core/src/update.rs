//! New versions: is there a newer Duck Forge Studio than this one?
//!
//! When the Studio starts (unless Settings says not to), it asks GitHub for
//! the list of Duck Forge's releases: one plain request for a public page,
//! with nothing about the person or their computer in it, made by the
//! `curl` program that comes with Windows. The newest release whose tag is
//! `studio-vX.Y.Z` is compared with the running version. With no internet,
//! or an answer it doesn't understand, it says nothing at all.
//!
//! It never downloads or installs anything by itself. Only when the person
//! presses Update does it download that release's `DuckForgeStudioSetup.exe`
//! (to the system's temporary folder) and start it; the installer then
//! puts the new version in, keeping the games and settings where they are.
//!
//! This is the Studio's own: none of it is in the engine or in a game.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use serde_json::Value;

/// Where Duck Forge's releases are listed.
pub const RELEASES: &str =
    "https://api.github.com/repos/mrguymrguy647-del/Duck-Forge/releases?per_page=30";

/// Where the list is read from: [`RELEASES`], or the address in
/// `DUCK_FORGE_RELEASES` (the release workflow points it at pretend lists,
/// to try the real download program on Windows).
pub fn releases_address() -> String {
    std::env::var("DUCK_FORGE_RELEASES").unwrap_or_else(|_| RELEASES.to_string())
}

/// The installer's name in a release.
pub const INSTALLER: &str = "DuckForgeStudioSetup.exe";

/// A newer Studio than this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewVersion {
    /// "0.2.0".
    pub version: String,
    /// The release's page: what's new in it.
    pub whats_new: String,
    /// Its installer's address and size in bytes, if it has one.
    pub installer: Option<(String, u64)>,
}

/// `[major, minor, patch]` from "0.2.0", "v0.2.0" or "studio-v0.2.0".
pub fn version_numbers(text: &str) -> Option<[u64; 3]> {
    let text = text.strip_prefix("studio-").unwrap_or(text);
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.');
    let mut numbers = [0; 3];
    for number in &mut numbers {
        *number = parts.next()?.trim().parse().ok()?;
    }
    parts.next().is_none().then_some(numbers)
}

/// The newest Studio release in GitHub's list of releases, if it's newer
/// than `running`. Drafts, pre-releases and other tags (the engine's, say)
/// don't count; nor does anything that can't be read.
pub fn newest(releases: &str, running: &str) -> Option<NewVersion> {
    let running = version_numbers(running)?;
    let list: Value = serde_json::from_str(releases).ok()?;
    let mut best: Option<([u64; 3], &Value)> = None;
    for release in list.as_array()? {
        let Some(tag) = release["tag_name"].as_str() else {
            continue;
        };
        if !tag.starts_with("studio-v")
            || release["draft"].as_bool() == Some(true)
            || release["prerelease"].as_bool() == Some(true)
        {
            continue;
        }
        let Some(numbers) = version_numbers(tag) else {
            continue;
        };
        if best.is_none_or(|(newest, _)| numbers > newest) {
            best = Some((numbers, release));
        }
    }
    let (numbers, release) = best?;
    if numbers <= running {
        return None;
    }
    let installer = release["assets"].as_array().and_then(|assets| {
        assets.iter().find_map(|asset| {
            if asset["name"].as_str() != Some(INSTALLER) {
                return None;
            }
            let address = asset["browser_download_url"].as_str()?.to_string();
            Some((address, asset["size"].as_u64().unwrap_or(0)))
        })
    });
    Some(NewVersion {
        version: format!("{}.{}.{}", numbers[0], numbers[1], numbers[2]),
        whats_new: release["html_url"]
            .as_str()
            .unwrap_or("https://github.com/mrguymrguy647-del/Duck-Forge/releases")
            .to_string(),
        installer,
    })
}

/// How the Studio fetches a page: GitHub's list as text, or a file saved
/// to a path. Tests give it a pretend internet instead.
pub trait Internet: Send + 'static {
    /// The text at `address`.
    fn get(&self, address: &str) -> Result<String, String>;
    /// Saves what's at `address` in the file `to`.
    fn save(&self, address: &str, to: &Path) -> Result<(), String>;
}

/// The real internet, through the `curl` program that comes with Windows
/// (and with macOS and most Linux). It sends only what any download sends.
pub struct Curl;

impl Curl {
    fn command() -> std::process::Command {
        // Windows' own, not another curl that happens to be first on the PATH.
        let windows = std::env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("System32").join("curl.exe"))
            .filter(|curl| cfg!(windows) && curl.is_file());
        #[allow(unused_mut)]
        let mut command = std::process::Command::new(windows.unwrap_or_else(|| "curl".into()));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        command.args(["--silent", "--show-error", "--fail", "--location"]);
        command.stdin(std::process::Stdio::null());
        command
    }

    fn said(output: &std::process::Output) -> String {
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim();
        if said.is_empty() {
            String::from("curl didn't say why")
        } else {
            said.to_string()
        }
    }
}

impl Internet for Curl {
    fn get(&self, address: &str) -> Result<String, String> {
        let output = Curl::command()
            .args(["--connect-timeout", "5", "--max-time", "15"])
            .args(["--header", "Accept: application/vnd.github+json"])
            .arg(address)
            .output()
            .map_err(|error| format!("couldn't start curl: {error}"))?;
        if !output.status.success() {
            return Err(Curl::said(&output));
        }
        String::from_utf8(output.stdout).map_err(|_| String::from("the answer wasn't text"))
    }

    fn save(&self, address: &str, to: &Path) -> Result<(), String> {
        let output = Curl::command()
            .args(["--connect-timeout", "10", "--max-time", "900", "--output"])
            .arg(to)
            .arg(address)
            .output()
            .map_err(|error| format!("couldn't start curl: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Curl::said(&output))
        }
    }
}

/// A check for a new version, running in the background so the Studio
/// starts at once.
pub struct UpdateCheck {
    answer: Receiver<Option<NewVersion>>,
}

impl UpdateCheck {
    /// Starts asking. `running` is this Studio's version.
    pub fn start(internet: impl Internet, running: &str) -> UpdateCheck {
        let (send, answer) = channel();
        let running = running.to_string();
        let address = releases_address();
        std::thread::spawn(move || {
            // No internet, or a page it can't read: nothing to say.
            let found = internet
                .get(&address)
                .ok()
                .and_then(|list| newest(&list, &running));
            let _ = send.send(found);
        });
        UpdateCheck { answer }
    }

    /// `None` while it's still asking; then the answer, once.
    pub fn poll(&mut self) -> Option<Option<NewVersion>> {
        match self.answer.try_recv() {
            Ok(found) => Some(found),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}

/// Downloading a new version's installer, after the person pressed Update.
pub struct Download {
    done: Receiver<Result<PathBuf, String>>,
}

impl Download {
    /// Starts downloading `new`'s installer into `folder`.
    pub fn start(internet: impl Internet, new: &NewVersion, folder: &Path) -> Download {
        let (send, done) = channel();
        let new = new.clone();
        let folder = folder.to_path_buf();
        std::thread::spawn(move || {
            let _ = send.send(download(&internet, &new, &folder));
        });
        Download { done }
    }

    /// `None` while it's downloading; then where the installer is, or why
    /// it couldn't be downloaded.
    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(String::from("the download stopped"))),
        }
    }
}

fn download(internet: &impl Internet, new: &NewVersion, folder: &Path) -> Result<PathBuf, String> {
    let Some((address, size)) = &new.installer else {
        return Err(format!(
            "version {} has no {INSTALLER} to download",
            new.version
        ));
    };
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let file = folder.join(format!("DuckForgeStudioSetup-{}.exe", new.version));
    let _ = std::fs::remove_file(&file);
    internet.save(address, &file)?;
    let got = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if got == 0 || (*size > 0 && got != *size) {
        let _ = std::fs::remove_file(&file);
        return Err(format!(
            "the download was {got} bytes instead of {size}; try again"
        ));
    }
    Ok(file)
}

/// Where a downloaded installer is kept: the system's temporary folder.
pub fn download_folder() -> PathBuf {
    std::env::temp_dir().join("Duck Forge Studio update")
}

/// Starts a downloaded installer. The Studio closes itself after this, so
/// the installer can put the new version in its place.
pub fn start_installer(file: &Path) -> Result<(), String> {
    std::process::Command::new(file)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("couldn't start {}: {error}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// A release, as GitHub lists it.
    fn release(tag: &str, draft: bool, prerelease: bool, installer: bool) -> String {
        let assets = if installer {
            format!(
                r#"[{{"name": "DuckForgeStudio-{tag}-windows.zip", "browser_download_url": "https://example.test/{tag}.zip", "size": 9}},
                   {{"name": "DuckForgeStudioSetup.exe", "browser_download_url": "https://example.test/{tag}/DuckForgeStudioSetup.exe", "size": 5}}]"#
            )
        } else {
            String::from("[]")
        };
        format!(
            r#"{{"tag_name": "{tag}", "draft": {draft}, "prerelease": {prerelease},
                "html_url": "https://example.test/releases/tag/{tag}", "assets": {assets}}}"#
        )
    }

    fn list(releases: &[String]) -> String {
        format!("[{}]", releases.join(","))
    }

    /// A pretend internet: pages by address, and whether it's there at all.
    #[derive(Clone, Default)]
    struct Pretend {
        pages: BTreeMap<String, String>,
        offline: bool,
        asked: Arc<Mutex<Vec<String>>>,
    }

    impl Internet for Pretend {
        fn get(&self, address: &str) -> Result<String, String> {
            self.asked.lock().unwrap().push(address.to_string());
            if self.offline {
                return Err(String::from("Could not resolve host: api.github.com"));
            }
            self.pages
                .get(address)
                .cloned()
                .ok_or_else(|| String::from("404"))
        }
        fn save(&self, address: &str, to: &Path) -> Result<(), String> {
            let text = self.get(address)?;
            std::fs::write(to, text).map_err(|e| e.to_string())
        }
    }

    fn answer(mut check: UpdateCheck) -> Option<NewVersion> {
        let start = Instant::now();
        loop {
            if let Some(answer) = check.poll() {
                return answer;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "no answer");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn versions_are_read_and_compared_as_numbers() {
        assert_eq!(version_numbers("studio-v0.10.2"), Some([0, 10, 2]));
        assert_eq!(version_numbers("0.1.0"), Some([0, 1, 0]));
        assert_eq!(version_numbers("v1.2"), None);
        assert_eq!(version_numbers("studio-v1.2.3-beta"), None);
        assert!(version_numbers("0.10.0") > version_numbers("0.9.9"));
    }

    #[test]
    fn the_newest_studio_release_is_found_and_others_ignored() {
        let releases = list(&[
            release("studio-v0.3.0", true, false, true), // a draft
            release("studio-v0.2.5", false, true, true), // a pre-release
            release("v9.9.9", false, false, true),       // the engine's
            release("studio-v0.2.0", false, false, true),
            release("studio-v0.10.0", false, false, true),
            release("studio-vnonsense", false, false, true),
        ]);
        let new = newest(&releases, "0.1.0").unwrap();
        assert_eq!(new.version, "0.10.0", "0.10 is newer than 0.2");
        assert_eq!(
            new.whats_new,
            "https://example.test/releases/tag/studio-v0.10.0"
        );
        assert_eq!(
            new.installer,
            Some((
                String::from("https://example.test/studio-v0.10.0/DuckForgeStudioSetup.exe"),
                5
            ))
        );
        let without = newest(
            &list(&[release("studio-v0.2.0", false, false, false)]),
            "0.1.0",
        );
        assert_eq!(
            without.unwrap().installer,
            None,
            "Update then opens the page instead"
        );
    }

    #[test]
    fn already_up_to_date_says_nothing() {
        let releases = list(&[release("studio-v0.1.0", false, false, true)]);
        assert_eq!(newest(&releases, "0.1.0"), None, "the same version");
        assert_eq!(
            newest(&releases, "0.2.0"),
            None,
            "a newer one than any release"
        );
        assert_eq!(newest("[]", "0.1.0"), None, "no releases yet");
        assert_eq!(newest("<html>rate limited</html>", "0.1.0"), None);
        assert_eq!(newest(r#"{"message": "Not Found"}"#, "0.1.0"), None);
    }

    #[test]
    fn the_check_asks_only_for_the_list_and_is_silent_offline() {
        let mut internet = Pretend::default();
        internet.pages.insert(
            RELEASES.to_string(),
            list(&[release("studio-v0.2.0", false, false, true)]),
        );
        let found = answer(UpdateCheck::start(internet.clone(), "0.1.0"));
        assert_eq!(found.unwrap().version, "0.2.0");
        // One request, for the public list, with nothing about the person in it.
        assert_eq!(*internet.asked.lock().unwrap(), [RELEASES]);

        let offline = Pretend {
            offline: true,
            ..Pretend::default()
        };
        assert_eq!(answer(UpdateCheck::start(offline, "0.1.0")), None);
        assert_eq!(
            answer(UpdateCheck::start(internet.clone(), "0.2.0")),
            None,
            "up to date"
        );
    }

    #[test]
    fn update_downloads_the_installer_and_checks_its_size() {
        let folder = crate::test_folder::TestFolder::new("update-download");
        let new = NewVersion {
            version: String::from("0.2.0"),
            whats_new: String::from("https://example.test/releases/tag/studio-v0.2.0"),
            installer: Some((String::from("https://example.test/setup.exe"), 5)),
        };
        let mut internet = Pretend::default();
        internet.pages.insert(
            String::from("https://example.test/setup.exe"),
            String::from("MZ..."),
        );
        let wait = |mut download: Download| loop {
            if let Some(done) = download.poll() {
                return done;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let file = wait(Download::start(internet.clone(), &new, &folder)).unwrap();
        assert_eq!(file, folder.join("DuckForgeStudioSetup-0.2.0.exe"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "MZ...");

        // Cut short: not kept, and it says so.
        internet.pages.insert(
            String::from("https://example.test/setup.exe"),
            String::from("MZ"),
        );
        let error = wait(Download::start(internet.clone(), &new, &folder)).unwrap_err();
        assert!(error.contains("2 bytes instead of 5"), "{error}");
        assert!(!file.exists());

        let offline = Pretend {
            offline: true,
            ..Pretend::default()
        };
        assert!(wait(Download::start(offline, &new, &folder)).is_err());
        let no_installer = NewVersion {
            installer: None,
            ..new
        };
        let error = wait(Download::start(internet, &no_installer, &folder)).unwrap_err();
        assert!(error.contains("has no DuckForgeStudioSetup.exe"), "{error}");
    }
}

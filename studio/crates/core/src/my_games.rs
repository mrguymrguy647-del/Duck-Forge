//! Where a person's games live when the Studio was installed (or unzipped):
//! a folder of their own, `Documents\Duck Forge`, never the Studio's.
//!
//! The Studio comes with a copy of Duck Forge, in a `duckforge` folder next
//! to DuckForgeStudio.exe. That copy belongs to the Studio: installing a
//! newer version replaces it and uninstalling removes it. So the Studio
//! never builds or saves anything in it. The first time it starts, it
//! copies it to the person's own games folder, and everything happens
//! there: new games, playthroughs, tune files, checks, clips.
//!
//! A newer Studio brings that folder's Duck Forge up to date the next time
//! it starts: each file that came with the Studio, and that nobody has
//! changed since, is replaced by the new version's. A file the person
//! changed is kept as it is, and what they made is never touched.
//! `.came-with-studio.json` in the folder remembers which files came with
//! the Studio, and what was in each, to tell the two apart. `Cargo.toml` is
//! the one file both write to (a new game adds its lines), so the new one
//! is used with the person's games' lines added back.
//!
//! Building writes gigabytes, so the folder's `.cargo/config.toml` sends
//! builds to a folder of their own outside Documents (which OneDrive may
//! be copying to the internet): `%LOCALAPPDATA%\Duck Forge\build`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::problem::Problem;

/// The file in the games folder that remembers what came with the Studio.
pub const RECORD: &str = ".came-with-studio.json";

/// What's never copied from the Studio's Duck Forge, and never looked at in
/// the person's: built programs, saved pictures, history, Cargo's settings
/// (written here, for this computer) and the record itself.
const NOT_COPIED: &[&str] = &["target", "out", "history", ".cargo", RECORD];

/// What the record says came with the Studio.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Says what this file is, for anyone who opens it.
    pub about: String,
    /// The Studio's version that last copied or updated the folder.
    pub studio: String,
    /// Each file that came with the Studio (with `/` between folders), and
    /// a fingerprint of what was in it.
    pub files: BTreeMap<String, String>,
}

/// What [`prepare`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prepared {
    /// The folder was made, with the Duck Forge that came with the Studio.
    Made,
    /// It was already up to date.
    UpToDate,
    /// A newer (or older) Studio's Duck Forge was put in.
    BroughtUpToDate {
        studio: String,
        replaced: usize,
        added: usize,
        removed: usize,
        /// Files that came with the Studio but were changed in the folder,
        /// so they were kept as they are.
        kept: Vec<String>,
    },
    /// A Duck Forge folder the Studio didn't make (a download, say): it's
    /// used as it is, and never changed.
    NotMadeByTheStudio,
}

impl Prepared {
    /// What happened, in a sentence for Home, if it's worth saying.
    pub fn say(&self, folder: &Path) -> Option<String> {
        match self {
            Prepared::Made => Some(format!(
                "Your games are kept in {}. Installing a new version of the Studio, or \
                 uninstalling it, never changes that folder.",
                folder.display()
            )),
            Prepared::BroughtUpToDate { studio, kept, .. } => {
                let mut said = format!(
                    "Duck Forge in {} was brought up to date for Studio {studio}. Your games \
                     weren't touched.",
                    folder.display()
                );
                if !kept.is_empty() {
                    said += &format!(
                        " You had changed {}, so {} kept as you left {}.",
                        list(kept),
                        if kept.len() == 1 {
                            "it was"
                        } else {
                            "they were"
                        },
                        if kept.len() == 1 { "it" } else { "them" }
                    );
                }
                Some(said)
            }
            Prepared::UpToDate | Prepared::NotMadeByTheStudio => None,
        }
    }
}

fn list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first @ .., last] if first.len() < 4 => format!("{} and {last}", first.join(", ")),
        [first, second, third, rest @ ..] => {
            format!("{first}, {second}, {third} and {} more", rest.len())
        }
        _ => unreachable!(),
    }
}

/// The copy of Duck Forge that came with the Studio: `duckforge` next to
/// its program. `None` when the Studio wasn't installed or unzipped (built
/// from the code with `cargo run`, say).
pub fn came_with_the_studio() -> Option<PathBuf> {
    let program = std::env::current_exe().ok()?;
    let copy = program.parent()?.join("duckforge");
    crate::folder::GamesFolder::open(&copy).ok()?;
    Some(copy)
}

/// Whether `folder` is the copy of Duck Forge that came with the Studio,
/// where nothing must be made (it's replaced by the next version).
pub fn is_the_studios_own(folder: &Path, studios_own: &Path) -> bool {
    match (folder.canonicalize(), studios_own.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => folder == studios_own,
    }
}

/// The person's own games folder: `Duck Forge` in their Documents (where
/// Windows really keeps Documents, even when OneDrive has moved it).
/// `DUCK_FORGE_GAMES` names another folder (the release workflow and tests
/// use it).
pub fn my_games_folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("DUCK_FORGE_GAMES") {
        return Some(PathBuf::from(folder));
    }
    Some(documents()?.join("Duck Forge"))
}

/// Where builds go: a folder of their own outside Documents.
/// `%LOCALAPPDATA%\Duck Forge\build` on Windows, `~/.cache/duck-forge/build`
/// elsewhere. `DUCK_FORGE_BUILD` names another folder.
pub fn build_folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("DUCK_FORGE_BUILD") {
        return Some(PathBuf::from(folder));
    }
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")?;
        return Some(PathBuf::from(local).join("Duck Forge").join("build"));
    }
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".cache")))?;
    Some(cache.join("duck-forge").join("build"))
}

#[cfg(windows)]
fn documents() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};

    let mut path: *mut u16 = std::ptr::null_mut();
    // SAFETY: Windows writes a new string to `path` and we free it with
    // CoTaskMemFree, as SHGetKnownFolderPath's documentation says, whether
    // or not it succeeded. The string ends with a 0, which bounds `len`.
    unsafe {
        let result = SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut path);
        let folder = if result >= 0 && !path.is_null() {
            let mut len = 0;
            while *path.add(len) != 0 {
                len += 1;
            }
            let wide = std::slice::from_raw_parts(path, len);
            Some(PathBuf::from(std::ffi::OsString::from_wide(wide)))
        } else {
            None
        };
        CoTaskMemFree(path as *const _);
        folder.or_else(|| Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join("Documents")))
    }
}

#[cfg(not(windows))]
fn documents() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let documents = home.join("Documents");
    Some(if documents.is_dir() { documents } else { home })
}

/// Makes the games folder from the Studio's own copy of Duck Forge, or
/// brings it up to date if it came from an older one. Never touches the
/// person's games, or anything they changed. `build`, if given, is where
/// the folder's builds go (written in its `.cargo/config.toml`).
pub fn prepare(
    studios_own: &Path,
    games: &Path,
    studio_version: &str,
    build: Option<&Path>,
) -> Result<Prepared, Problem> {
    let couldnt = |what: &str, path: &Path, error: std::io::Error| {
        Problem::new(
            format!("The Studio couldn't {what} {}.", path.display()),
            format!(
                "{error}. Check that the folder isn't open in another program, then start \
                 the Studio again (or choose another games folder on Home)."
            ),
        )
    };
    let shipped = fingerprints(studios_own);
    let record_file = games.join(RECORD);
    let empty = std::fs::read_dir(games).map_or(true, |mut entries| entries.next().is_none());

    let prepared = if empty {
        for name in shipped.keys() {
            copy(studios_own, games, name).map_err(|e| couldnt("copy", &games.join(name), e))?;
        }
        Prepared::Made
    } else {
        let Ok(text) = std::fs::read_to_string(&record_file) else {
            // A folder the Studio didn't make: used as it is, if it's Duck Forge.
            crate::folder::GamesFolder::open(games).map_err(|_| {
                Problem::new(
                    format!(
                        "{} is already there, and it isn't a Duck Forge folder.",
                        games.display()
                    ),
                    "Rename or move that folder and start the Studio again, or choose another \
                     games folder on Home.",
                )
            })?;
            return Ok(Prepared::NotMadeByTheStudio);
        };
        let record: Record = serde_json::from_str(&text).unwrap_or_default();
        if record.files == shipped {
            Prepared::UpToDate
        } else {
            update(studios_own, games, &record, &shipped, studio_version)
                .map_err(|(path, e)| couldnt("update", &path, e))?
        }
    };

    if let Some(build) = build {
        write_cargo_config(games, build).map_err(|e| couldnt("write", &games.join(".cargo"), e))?;
    }
    if prepared != Prepared::UpToDate {
        let record = Record {
            about: String::from(
                "Which files came with Duck Forge Studio, and what was in them, so a newer \
                 Studio can bring them up to date without touching anything you changed or \
                 made. Please don't edit it.",
            ),
            studio: studio_version.to_string(),
            files: shipped,
        };
        let text = serde_json::to_string_pretty(&record).unwrap_or_default();
        std::fs::write(&record_file, text + "\n").map_err(|e| couldnt("write", &record_file, e))?;
    }
    Ok(prepared)
}

/// Puts in a new Studio's Duck Forge: see the module's description.
fn update(
    studios_own: &Path,
    games: &Path,
    record: &Record,
    shipped: &BTreeMap<String, String>,
    studio_version: &str,
) -> Result<Prepared, (PathBuf, std::io::Error)> {
    let (mut replaced, mut added, mut removed) = (0, 0, 0);
    let mut kept = Vec::new();
    for (name, new) in shipped {
        let path = games.join(name);
        let now = std::fs::read(&path).ok().map(|bytes| fingerprint(&bytes));
        let was = record.files.get(name);
        match (was, now) {
            (_, Some(now)) if &now == new => {}
            (Some(was), Some(now)) if *was == now => {
                copy(studios_own, games, name).map_err(|e| (path.clone(), e))?;
                replaced += 1;
            }
            (Some(_), Some(_)) if name == "Cargo.toml" => {
                let mine = std::fs::read_to_string(&path).unwrap_or_default();
                let theirs = std::fs::read_to_string(studios_own.join(name)).unwrap_or_default();
                std::fs::write(&path, with_my_games(&theirs, &mine))
                    .map_err(|e| (path.clone(), e))?;
                replaced += 1;
            }
            // Changed in the folder (or a file of the person's own with
            // the same name): theirs is kept.
            (_, Some(_)) => kept.push(name.clone()),
            // Deleted from the folder: a game (or its playthrough) the
            // person didn't want stays deleted. Duck Forge's own files
            // come back: the games need them.
            (Some(_), None) if is_a_games_file(name) => {}
            (_, None) => {
                copy(studios_own, games, name).map_err(|e| (path.clone(), e))?;
                added += 1;
            }
        }
    }
    // Files the new Duck Forge doesn't have any more go, unless they were changed.
    for (name, was) in &record.files {
        if shipped.contains_key(name) {
            continue;
        }
        let path = games.join(name);
        if std::fs::read(&path).is_ok_and(|bytes| &fingerprint(&bytes) == was) {
            std::fs::remove_file(&path).map_err(|e| (path.clone(), e))?;
            removed += 1;
        }
    }
    Ok(Prepared::BroughtUpToDate {
        studio: studio_version.to_string(),
        replaced,
        added,
        removed,
        kept,
    })
}

/// A game's own files: under `examples/` or `playthroughs/`.
fn is_a_games_file(name: &str) -> bool {
    name.starts_with("examples/") || name.starts_with("playthroughs/")
}

/// The new `Cargo.toml`, with the `[[example]]` entries of the person's
/// games (the ones `tools/new_game` added) put back at the end.
pub fn with_my_games(new: &str, mine: &str) -> String {
    let names_in = |manifest: &str| -> BTreeSet<String> {
        examples(manifest)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    };
    let known = names_in(new);
    let mut merged = new.to_string();
    for (name, block) in examples(mine) {
        if !known.contains(&name) {
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push('\n');
            merged += &block;
        }
    }
    merged
}

/// Each `[[example]]` table in a `Cargo.toml`: its name, and its lines.
fn examples(manifest: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut current: Option<(Option<String>, String)> = None;
    for line in manifest.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if let Some((Some(name), block)) = current.take() {
                found.push((name, block));
            }
            if trimmed == "[[example]]" {
                current = Some((None, line.to_string()));
            }
            continue;
        }
        if let Some((name, block)) = &mut current {
            if trimmed.is_empty() || trimmed.starts_with('#') {
                // Blank lines and comments between tables belong to neither.
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("name") {
                let value = value.trim_start().trim_start_matches('=').trim();
                *name = Some(value.trim_matches('"').to_string());
            }
            block.push_str(line);
        }
    }
    if let Some((Some(name), block)) = current {
        found.push((name, block));
    }
    found
}

/// Writes `.cargo/config.toml`, so every build in the folder (the Studio's,
/// the tools', a terminal's or an agent's) goes to `build`. A config the
/// person wrote themselves is left alone.
fn write_cargo_config(games: &Path, build: &Path) -> std::io::Result<()> {
    const MARK: &str = "# Written by Duck Forge Studio.";
    let file = games.join(".cargo").join("config.toml");
    if let Ok(text) = std::fs::read_to_string(&file)
        && !text.starts_with(MARK)
    {
        return Ok(());
    }
    let quoted = build
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let text = format!(
        "{MARK}\n\
         # Builds go to a folder of their own, outside Documents: they're big, and\n\
         # nothing in them needs keeping (they're made again when needed).\n\
         [build]\n\
         target-dir = \"{quoted}\"\n"
    );
    if std::fs::read_to_string(&file).ok().as_deref() == Some(text.as_str()) {
        return Ok(());
    }
    std::fs::create_dir_all(games.join(".cargo"))?;
    std::fs::write(file, text)
}

/// The folder `cargo` builds in for a games folder: `CARGO_TARGET_DIR`, or
/// the `target-dir` its `.cargo/config.toml` names, or its `target`.
pub fn target_folder(games: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => games.join(dir),
        None => configured_target_folder(games),
    }
}

/// The folder the games folder's own `.cargo/config.toml` builds in (or its
/// `target`), whatever `CARGO_TARGET_DIR` says.
fn configured_target_folder(games: &Path) -> PathBuf {
    let config =
        std::fs::read_to_string(games.join(".cargo").join("config.toml")).unwrap_or_default();
    for line in config.lines() {
        let Some(value) = line.trim().strip_prefix("target-dir") else {
            continue;
        };
        let value = value.trim_start().trim_start_matches('=').trim();
        if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            let path = inner.replace("\\\"", "\"").replace("\\\\", "\\");
            return games.join(path);
        }
        if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            return games.join(inner);
        }
    }
    games.join("target")
}

/// Copies one file (by its `/` name) from the Studio's Duck Forge to the
/// games folder: read and written again, so it's an ordinary file of the
/// person's (not read-only, like an installed one can be).
fn copy(from: &Path, to: &Path, name: &str) -> std::io::Result<()> {
    let bytes = std::fs::read(from.join(name))?;
    let target = to.join(name);
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(&target, bytes)?;
    // On Linux and macOS, tools/*.sh stay programs that can be run.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(from.join(name))?.permissions().mode();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode | 0o600))?;
    }
    Ok(())
}

/// Every file in a Duck Forge folder (but [`NOT_COPIED`]), by its name with
/// `/` between folders, with its fingerprint.
pub fn fingerprints(root: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut folders = vec![(root.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if prefix.is_empty() && NOT_COPIED.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            let full = format!("{prefix}{name}");
            if path.is_dir() {
                folders.push((path, format!("{full}/")));
            } else if let Ok(bytes) = std::fs::read(&path) {
                files.insert(full, fingerprint(&bytes));
            }
        }
    }
    files
}

/// A short fingerprint of a file's bytes (FNV-1a, 64 bits): the same on
/// every computer and every version of Rust.
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_folder::TestFolder;

    /// A tiny Duck Forge, as the Studio might ship it.
    fn studios_own(root: &Path, readme: &str) {
        let write = |name: &str, text: &str| {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "Cargo.toml",
            "[package]\nname = \"duckforge\"\n\n[[example]]\nname = \"dodge\"\ntest = true\n",
        );
        write("src/lib.rs", "// the engine\n");
        write("examples/dodge.rs", "fn main() {}\n");
        write("examples/template/main.rs", "fn main() {}\n");
        write("playthroughs/dodge.json", "{}\n");
        write("tools/verify.bat", "@echo off\r\n");
        write("README.md", readme);
        write("target/debug/big.exe", "never copied");
    }

    fn read(folder: &Path, name: &str) -> String {
        std::fs::read_to_string(folder.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn the_first_start_makes_the_folder_and_sends_builds_elsewhere() {
        let test = TestFolder::new("my-games-made");
        let (own, games, build) = (
            test.join("Studio/duckforge"),
            test.join("Documents/Duck Forge"),
            test.join("Local/build"),
        );
        studios_own(&own, "v1\n");
        let prepared = prepare(&own, &games, "0.1.0", Some(&build)).unwrap();
        assert_eq!(prepared, Prepared::Made);
        assert!(
            prepared
                .say(&games)
                .unwrap()
                .contains("never changes that folder")
        );
        assert_eq!(read(&games, "README.md"), "v1\n");
        assert_eq!(
            read(&games, "tools/verify.bat"),
            "@echo off\r\n",
            "every byte"
        );
        assert!(
            !games.join("target").exists(),
            "built programs aren't copied"
        );
        assert!(crate::folder::GamesFolder::open(&games).is_ok());
        assert_eq!(configured_target_folder(&games), build, "builds go outside the folder");
        let record: Record = serde_json::from_str(&read(&games, RECORD)).unwrap();
        assert_eq!(record.studio, "0.1.0");
        assert!(record.files.contains_key("src/lib.rs"));
        // Starting again changes nothing.
        assert_eq!(
            prepare(&own, &games, "0.1.0", Some(&build)).unwrap(),
            Prepared::UpToDate
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = own.join("tools/verify.sh");
            std::fs::write(&script, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            let again = test.join("again");
            prepare(&own, &again, "0.1.0", None).unwrap();
            let mode = std::fs::metadata(again.join("tools/verify.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "a script can still be run");
        }
    }

    #[test]
    fn a_newer_studio_never_touches_my_games_or_my_changes() {
        let test = TestFolder::new("my-games-update");
        let (own, games) = (test.join("Studio/duckforge"), test.join("Duck Forge"));
        studios_own(&own, "v1\n");
        prepare(&own, &games, "0.1.0", None).unwrap();

        // What the person did: made a game (as tools/new_game does),
        // changed a game that came with Duck Forge, deleted another, and
        // played (out/ and history/).
        std::fs::create_dir_all(games.join("examples/bee_maze")).unwrap();
        std::fs::write(games.join("examples/bee_maze/main.rs"), "// my game\n").unwrap();
        std::fs::write(games.join("examples/bee_maze.tune.json"), "{\"jump\": 3}\n").unwrap();
        std::fs::write(games.join("playthroughs/bee_maze.json"), "{\"mine\": 1}\n").unwrap();
        let mut manifest = read(&games, "Cargo.toml");
        manifest += "\n[[example]]\nname = \"bee_maze\"\ntest = true\n";
        std::fs::write(games.join("Cargo.toml"), &manifest).unwrap();
        std::fs::write(games.join("examples/dodge.rs"), "// I changed dodge\n").unwrap();
        std::fs::remove_dir_all(games.join("examples/template")).unwrap();
        std::fs::create_dir_all(games.join("out/clips")).unwrap();
        std::fs::write(games.join("out/clips/bee_maze-1.gif"), "clip").unwrap();
        // ...and an engine file went missing.
        std::fs::remove_file(games.join("src/lib.rs")).unwrap();

        // The next version: a new README, a new engine file and tool, one
        // tool gone, and a new example.
        studios_own(&own, "v2\n");
        std::fs::write(own.join("src/new.rs"), "// new in v2\n").unwrap();
        std::fs::write(own.join("examples/new_starter.rs"), "fn main() {}\n").unwrap();
        std::fs::remove_file(own.join("tools/verify.bat")).unwrap();
        std::fs::write(own.join("tools/verify.sh"), "#!/bin/sh\n").unwrap();
        let mut new_manifest = read(&own, "Cargo.toml");
        new_manifest += "\n[[example]]\nname = \"new_starter\"\ntest = true\n";
        std::fs::write(own.join("Cargo.toml"), &new_manifest).unwrap();

        let prepared = prepare(&own, &games, "0.2.0", None).unwrap();
        let Prepared::BroughtUpToDate { kept, removed, .. } = &prepared else {
            panic!("{prepared:?}");
        };
        assert_eq!(kept, &["examples/dodge.rs"]);
        assert_eq!(*removed, 1, "the tool that's gone");
        let said = prepared.say(&games).unwrap();
        assert!(
            said.contains("Your games weren't touched") && said.contains("examples/dodge.rs"),
            "{said}"
        );

        // Mine, untouched.
        assert_eq!(read(&games, "examples/bee_maze/main.rs"), "// my game\n");
        assert_eq!(
            read(&games, "examples/bee_maze.tune.json"),
            "{\"jump\": 3}\n"
        );
        assert_eq!(
            read(&games, "playthroughs/bee_maze.json"),
            "{\"mine\": 1}\n"
        );
        assert_eq!(read(&games, "out/clips/bee_maze-1.gif"), "clip");
        assert_eq!(read(&games, "examples/dodge.rs"), "// I changed dodge\n");
        assert!(
            !games.join("examples/template").exists(),
            "deleted stays deleted"
        );
        // Duck Forge's, brought up to date.
        assert_eq!(read(&games, "README.md"), "v2\n");
        assert_eq!(read(&games, "src/new.rs"), "// new in v2\n");
        assert_eq!(
            read(&games, "src/lib.rs"),
            "// the engine\n",
            "an engine file comes back"
        );
        assert_eq!(read(&games, "examples/new_starter.rs"), "fn main() {}\n");
        assert!(!games.join("tools/verify.bat").exists());
        assert_eq!(read(&games, "playthroughs/dodge.json"), "{}\n");
        // Cargo.toml: the new one, with my game's lines.
        let merged = read(&games, "Cargo.toml");
        assert!(merged.starts_with(&new_manifest), "{merged}");
        assert!(
            merged.ends_with("\n\n[[example]]\nname = \"bee_maze\"\ntest = true\n"),
            "{merged}"
        );
        assert_eq!(merged.matches("name = \"dodge\"").count(), 1);

        // Once more with the same version: nothing to do, and my change is
        // still kept next time too.
        assert_eq!(
            prepare(&own, &games, "0.2.0", None).unwrap(),
            Prepared::UpToDate
        );
    }

    #[test]
    fn a_folder_the_studio_didnt_make_is_never_changed() {
        let test = TestFolder::new("my-games-theirs");
        let (own, games) = (test.join("Studio/duckforge"), test.join("Duck Forge"));
        studios_own(&own, "v1\n");
        studios_own(&games, "my own download\n");
        assert_eq!(
            prepare(&own, &games, "0.1.0", Some(&test.join("b"))).unwrap(),
            Prepared::NotMadeByTheStudio
        );
        assert_eq!(read(&games, "README.md"), "my own download\n");
        assert!(!games.join(RECORD).exists() && !games.join(".cargo").exists());

        let other = test.join("Something else");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("letter.txt"), "hi").unwrap();
        let problem = prepare(&own, &other, "0.1.0", None).unwrap_err();
        assert!(
            problem.what.contains("isn't a Duck Forge folder"),
            "{problem}"
        );
        assert_eq!(read(&other, "letter.txt"), "hi");
    }

    #[test]
    fn my_own_cargo_config_is_left_alone() {
        let test = TestFolder::new("my-games-config");
        let (own, games) = (test.join("Studio/duckforge"), test.join("Duck Forge"));
        studios_own(&own, "v1\n");
        prepare(
            &own,
            &games,
            "0.1.0",
            Some(&test.join("Build \"quoted\" here")),
        )
        .unwrap();
        assert_eq!(configured_target_folder(&games), test.join("Build \"quoted\" here"));
        std::fs::write(
            games.join(".cargo/config.toml"),
            "[build]\ntarget-dir = 'mine'\n",
        )
        .unwrap();
        prepare(&own, &games, "0.1.0", Some(&test.join("b"))).unwrap();
        assert_eq!(configured_target_folder(&games), games.join("mine"));
    }

    #[test]
    fn fingerprints_and_examples() {
        assert_eq!(fingerprint(b""), "cbf29ce484222325");
        assert_eq!(fingerprint(b"a"), "af63dc4c8601ec8c");
        let manifest = "[package]\nname = \"duckforge\"\n\n[[example]]\nname = \"a\"\ntest = true\n\n# A comment\n[[example]]\nname = \"b\"\n\n[profile.dev]\nopt-level = 1\n";
        let names: Vec<String> = examples(manifest).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(list(&["a".into(), "b".into()]), "a and b");
        assert_eq!(
            list(&["a".into(), "b".into(), "c".into(), "d".into(), "e".into()]),
            "a, b, c and 2 more"
        );
    }
}

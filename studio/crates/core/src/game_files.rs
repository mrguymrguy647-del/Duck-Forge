//! The Files panel's side of things: a game's own folder as a tree, and
//! what can be done to the files in it, as the commands `files.select`,
//! `files.open`, `files.play`, `files.add`, `files.new_folder`,
//! `files.rename` and `files.delete`.
//!
//! Every path in a command is the file's place inside the game's folder,
//! with `/` between folders (`assets/duck.png`), and can't reach outside
//! it. Deleting moves to the Recycle Bin. A later phase drags files from
//! the panel into the game's scene, so everything here is a command an
//! agent can call too.

use std::path::{Component, Path, PathBuf};

use crate::problem::Problem;
use crate::recycle::RecycleBin;

/// What a file is, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Folder,
    /// Rust code (`.rs`).
    Code,
    /// A picture: PNG, GIF, BMP, JPEG.
    Picture,
    /// A sound: WAV (and, to open elsewhere, OGG, MP3, FLAC).
    Sound,
    /// A level (`level.txt`, or any `.txt` with a `.legend.json` beside it).
    Level,
    /// Other text the game reads: JSON, TOML, plain text.
    Data,
    Other,
}

impl FileKind {
    /// Of a file (not a folder) called `name`, with `siblings` beside it.
    pub fn of(name: &str, siblings: &[String]) -> FileKind {
        let lower = name.to_ascii_lowercase();
        let end = lower.rsplit_once('.').map_or("", |(_, end)| end);
        match end {
            "rs" => FileKind::Code,
            "png" | "gif" | "bmp" | "jpg" | "jpeg" => FileKind::Picture,
            "wav" | "ogg" | "mp3" | "flac" => FileKind::Sound,
            "txt" => {
                let stem = &name[..name.len() - 4];
                if lower.starts_with("level") || siblings.contains(&format!("{stem}.legend.json")) {
                    FileKind::Level
                } else {
                    FileKind::Data
                }
            }
            "json" | "toml" | "md" | "csv" => FileKind::Data,
            _ => FileKind::Other,
        }
    }

    /// In a word, for the panel.
    pub fn word(self) -> &'static str {
        match self {
            FileKind::Folder => "folder",
            FileKind::Code => "code",
            FileKind::Picture => "picture",
            FileKind::Sound => "sound",
            FileKind::Level => "level",
            FileKind::Data => "data",
            FileKind::Other => "file",
        }
    }
}

/// One line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Its place in the game's folder: `assets/duck.png`.
    pub path: String,
    /// Its own name: `duck.png`.
    pub name: String,
    pub kind: FileKind,
    /// How many folders deep: 0 for the game's folder's own files.
    pub depth: usize,
    /// Its size in bytes (0 for a folder).
    pub size: u64,
}

/// Every file and folder in the game's folder, as a tree read top to
/// bottom: in each folder, its folders first, then its files, by name.
pub fn tree(folder: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    add_folder(folder, "", 0, &mut entries);
    entries
}

fn add_folder(folder: &Path, prefix: &str, depth: usize, entries: &mut Vec<FileEntry>) {
    let Ok(read) = std::fs::read_dir(folder) else {
        return;
    };
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            folders.push(name);
        } else {
            files.push((name, entry.metadata().map_or(0, |m| m.len())));
        }
    }
    folders.sort_by_key(|name| name.to_lowercase());
    files.sort_by_key(|(name, _)| name.to_lowercase());
    let siblings: Vec<String> = files.iter().map(|(name, _)| name.clone()).collect();
    for name in folders {
        let path = format!("{prefix}{name}");
        entries.push(FileEntry {
            path: path.clone(),
            name: name.clone(),
            kind: FileKind::Folder,
            depth,
            size: 0,
        });
        add_folder(&folder.join(&name), &format!("{path}/"), depth + 1, entries);
    }
    for (name, size) in files {
        entries.push(FileEntry {
            path: format!("{prefix}{name}"),
            kind: FileKind::of(&name, &siblings),
            name,
            depth,
            size,
        });
    }
}

/// The real path of a place in the game's folder. A place that would be
/// outside it (`../`, a whole path) is refused.
pub fn resolve(folder: &Path, place: &str) -> Result<PathBuf, Problem> {
    let place = place.trim().replace('\\', "/");
    let relative = Path::new(&place);
    // (A colon is a drive, C:, or a hidden part of a file on Windows.)
    let inside = !place.contains(':')
        && relative
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if !inside {
        return Err(Problem::new(
            format!("{place} isn't inside the game's folder."),
            "Name a file in the game's folder, like assets/duck.png.",
        ));
    }
    // Part by part, so on Windows it's written with \ (File Explorer
    // doesn't take a / in the middle of a path).
    let mut path = folder.to_path_buf();
    for part in relative.components() {
        if let Component::Normal(part) = part {
            path.push(part);
        }
    }
    Ok(path)
}

/// A file or folder name a person typed, if it's one Windows allows.
pub fn check_name(name: &str) -> Result<String, Problem> {
    let name = name.trim();
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| "<>:\"/\\|?*".contains(c) || c.is_control());
    if bad {
        return Err(Problem::new(
            format!("\"{name}\" can't be a file's name."),
            "Use letters, numbers, spaces, - and _ (not / \\ : * ? \" < > |).",
        ));
    }
    Ok(name.to_string())
}

fn couldnt(what: &str, path: &Path, error: impl std::fmt::Display) -> Problem {
    Problem::new(
        format!("The Studio couldn't {what} {}.", path.display()),
        "Close any program that has it open, then try again.",
    )
    .with_details(error.to_string())
}

/// The start of every game: deleting or renaming it would leave no game.
fn guard_main(place: &str, doing: &str) -> Result<(), Problem> {
    if place.trim().trim_matches('/') == "main.rs" || place.trim().is_empty() {
        return Err(Problem::new(
            format!("main.rs is where the game starts, so it can't be {doing} here."),
            "Change it in the code instead.",
        ));
    }
    Ok(())
}

/// Copies files (and folders) into the game's folder, or into one of its
/// folders. A name that's taken gets " 2" (and so on): nothing is ever
/// replaced. Returns the new files' places.
pub fn add(folder: &Path, into: &str, files: &[PathBuf]) -> Result<Vec<String>, Problem> {
    let target = resolve(folder, into)?;
    if !target.is_dir() {
        return Err(Problem::new(
            format!("{into} isn't a folder in the game's folder."),
            "Drop the files on the game's folder, or on one of its folders.",
        ));
    }
    let mut added = Vec::new();
    for file in files {
        if !file.exists() {
            return Err(Problem::new(
                format!("{} isn't there.", file.display()),
                "Drop files that are on this computer.",
            ));
        }
        // Not the game's folder into itself.
        if target.starts_with(file) {
            return Err(Problem::new(
                format!("{} can't be copied into itself.", file.display()),
                "Drop it somewhere else.",
            ));
        }
        let to = free_place(&target, file);
        copy_all(file, &to).map_err(|e| couldnt("copy", file, e))?;
        let place = to
            .strip_prefix(folder)
            .unwrap_or(&to)
            .to_string_lossy()
            .replace('\\', "/");
        added.push(place);
    }
    Ok(added)
}

/// `name`, or `name 2.ext`, `name 3.ext`... free in `folder`.
fn free_place(folder: &Path, file: &Path) -> PathBuf {
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let (stem, dot_ext) = match name.rfind('.') {
        Some(i) if i > 0 && file.is_file() => (&name[..i], &name[i..]),
        _ => (&*name, ""),
    };
    let mut candidate = folder.join(&*name);
    let mut n = 2;
    while candidate.exists() {
        candidate = folder.join(format!("{stem} {n}{dot_ext}"));
        n += 1;
    }
    candidate
}

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)?.flatten() {
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        // Read and written again: an ordinary file of the game's (not
        // read-only, without "downloaded from the internet" on it).
        std::fs::write(to, std::fs::read(from)?)
    }
}

/// Makes a new, empty folder. `place` is where: `assets/music`.
pub fn new_folder(folder: &Path, place: &str) -> Result<String, Problem> {
    let path = resolve(folder, place)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    check_name(&name)?;
    if path.exists() {
        return Err(Problem::new(
            format!("There's already something called {place}."),
            "Pick another name.",
        ));
    }
    std::fs::create_dir_all(&path).map_err(|e| couldnt("make", &path, e))?;
    Ok(place.trim().replace('\\', "/"))
}

/// Gives a file or folder a new name, in the same folder. Returns its new
/// place.
pub fn rename(folder: &Path, place: &str, to: &str) -> Result<String, Problem> {
    guard_main(place, "renamed")?;
    let from = resolve(folder, place)?;
    if !from.exists() {
        return Err(Problem::new(
            format!("There's no {place} in the game's folder."),
            "Pick a file in the Files panel.",
        ));
    }
    let name = check_name(to)?;
    let target = from.with_file_name(&name);
    if target.exists() && target != from {
        return Err(Problem::new(
            format!("There's already a {name} there."),
            "Pick another name.",
        ));
    }
    std::fs::rename(&from, &target).map_err(|e| couldnt("rename", &from, e))?;
    Ok(target
        .strip_prefix(folder)
        .unwrap_or(&target)
        .to_string_lossy()
        .replace('\\', "/"))
}

/// Moves a file or folder to the Recycle Bin.
pub fn delete(folder: &Path, place: &str, bin: &RecycleBin) -> Result<(), Problem> {
    guard_main(place, "deleted")?;
    let path = resolve(folder, place)?;
    if !path.exists() {
        return Err(Problem::new(
            format!("There's no {place} in the game's folder."),
            "Pick a file in the Files panel.",
        ));
    }
    bin.throw_away(std::slice::from_ref(&path)).map_err(|why| {
        Problem::new(
            format!("{place} couldn't be moved to the Recycle Bin."),
            "Nothing was deleted. Close any program that has it open, then try again.",
        )
        .with_details(why)
    })
}

/// Opens a file in the program Windows uses for it (Notepad for text, the
/// Photos app for a picture...), or a folder in File Explorer.
pub fn open_with_the_system(path: &Path) -> Result<(), Problem> {
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("explorer");
        command.arg(path);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| couldnt("open", path, e))
}

/// Plays a sound file through the speakers, without waiting for it to end.
/// WAV files play in the Studio itself (on Windows); other sounds open in
/// the program Windows uses for them.
pub fn play_sound(path: &Path) -> Result<(), Problem> {
    let is_wav = path
        .extension()
        .is_some_and(|end| end.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return open_with_the_system(path);
    }
    play_wav(path)
}

#[cfg(windows)]
fn play_wav(path: &Path) -> Result<(), Problem> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is a 0-ended wide string that lives until PlaySoundW
    // returns, which is all it asks; with SND_ASYNC it keeps what it needs.
    let played = unsafe {
        PlaySoundW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
        )
    };
    if played == 0 {
        return Err(Problem::new(
            format!("{} couldn't be played.", path.display()),
            "Is it a WAV file? Try opening it with the Open button.",
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn play_wav(path: &Path) -> Result<(), Problem> {
    // Linux's sound players, if there's one; otherwise the default program.
    for player in ["paplay", "aplay", "afplay"] {
        let started = std::process::Command::new(player)
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if started.is_ok() {
            return Ok(());
        }
    }
    open_with_the_system(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_folder::TestFolder;

    fn game(test: &TestFolder) -> PathBuf {
        let folder = test.join("examples/bee_maze");
        for (name, text) in [
            ("main.rs", "fn main() {}"),
            ("bot.rs", "// bot"),
            ("level.txt", "p..#"),
            ("level.legend.json", "{}"),
            ("caves.txt", "p#"),
            ("caves.legend.json", "{}"),
            ("notes.txt", "todo"),
            ("assets/duck.png", "png"),
            ("assets/jump.wav", "wav"),
            ("assets/Big Tiles.json", "{}"),
        ] {
            let path = folder.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        folder
    }

    #[test]
    fn the_tree_has_folders_first_and_knows_what_each_file_is() {
        let test = TestFolder::new("files-tree");
        let folder = game(&test);
        let lines: Vec<(String, &str, usize)> = tree(&folder)
            .into_iter()
            .map(|e| (e.path, e.kind.word(), e.depth))
            .collect();
        let expect = |path: &str, kind: &'static str, depth| (path.to_string(), kind, depth);
        assert_eq!(
            lines,
            [
                expect("assets", "folder", 0),
                expect("assets/Big Tiles.json", "data", 1),
                expect("assets/duck.png", "picture", 1),
                expect("assets/jump.wav", "sound", 1),
                expect("bot.rs", "code", 0),
                expect("caves.legend.json", "data", 0),
                expect("caves.txt", "level", 0),
                expect("level.legend.json", "data", 0),
                expect("level.txt", "level", 0),
                expect("main.rs", "code", 0),
                expect("notes.txt", "data", 0),
            ]
        );
    }

    #[test]
    fn dropped_files_are_copied_in_and_never_replace_one() {
        let test = TestFolder::new("files-add");
        let folder = game(&test);
        let desktop = test.join("Desktop");
        std::fs::create_dir_all(desktop.join("Music")).unwrap();
        std::fs::write(desktop.join("duck.png"), "new duck").unwrap();
        std::fs::write(desktop.join("Music/song.wav"), "la").unwrap();
        let added = add(
            &folder,
            "assets",
            &[desktop.join("duck.png"), desktop.join("Music")],
        )
        .unwrap();
        assert_eq!(added, ["assets/duck 2.png", "assets/Music"]);
        assert_eq!(
            std::fs::read_to_string(folder.join("assets/duck.png")).unwrap(),
            "png"
        );
        assert_eq!(
            std::fs::read_to_string(folder.join("assets/duck 2.png")).unwrap(),
            "new duck"
        );
        assert!(folder.join("assets/Music/song.wav").is_file());
        assert!(desktop.join("duck.png").is_file(), "copied, not moved");
        assert_eq!(
            add(&folder, "", &[desktop.join("duck.png")]).unwrap(),
            ["duck.png"]
        );
        assert!(add(&folder, "nope", &[desktop.join("duck.png")]).is_err());
        assert!(add(&folder, "", &[desktop.join("gone.png")]).is_err());
        assert!(
            add(&folder.join("assets"), "", std::slice::from_ref(&folder))
                .unwrap_err()
                .what
                .contains("into itself")
        );
    }

    #[test]
    fn folders_are_made_files_renamed_and_deleted_to_the_bin() {
        let test = TestFolder::new("files-change");
        let folder = game(&test);
        let bin = RecycleBin::Folder(test.join("bin"));
        assert_eq!(new_folder(&folder, "assets/music").unwrap(), "assets/music");
        assert!(folder.join("assets/music").is_dir());
        assert!(
            new_folder(&folder, "assets/music").is_err(),
            "already there"
        );
        assert_eq!(
            rename(&folder, "assets/jump.wav", "hop.wav").unwrap(),
            "assets/hop.wav"
        );
        assert!(folder.join("assets/hop.wav").is_file());
        assert!(
            rename(&folder, "assets/hop.wav", "duck.png")
                .unwrap_err()
                .what
                .contains("already a duck.png")
        );
        assert!(
            rename(&folder, "assets/hop.wav", "a/b.wav").is_err(),
            "a name, not a path"
        );
        delete(&folder, "assets/hop.wav", &bin).unwrap();
        assert!(!folder.join("assets/hop.wav").exists());
        assert!(test.join("bin/hop.wav").is_file(), "in the Recycle Bin");
        delete(&folder, "assets", &bin).unwrap();
        assert!(test.join("bin/assets/duck.png").is_file());
        // main.rs is the game.
        assert!(
            delete(&folder, "main.rs", &bin)
                .unwrap_err()
                .what
                .contains("where the game starts")
        );
        assert!(rename(&folder, "main.rs", "start.rs").is_err());
        assert!(delete(&folder, "", &bin).is_err(), "not the whole folder");
    }

    #[test]
    fn nothing_outside_the_games_folder_can_be_reached() {
        let test = TestFolder::new("files-outside");
        let folder = game(&test);
        let bin = RecycleBin::Folder(test.join("bin"));
        for outside in [
            "../other/main.rs",
            "..",
            "/etc/passwd",
            "C:\\Windows",
            "assets/../../x",
        ] {
            assert!(resolve(&folder, outside).is_err(), "{outside}");
        }
        assert!(delete(&folder, "../bee_maze/bot.rs", &bin).is_err());
        assert!(folder.join("bot.rs").is_file());
        assert_eq!(
            resolve(&folder, "assets\\duck.png").unwrap(),
            folder.join("assets/duck.png")
        );
        assert!(check_name("my: song").is_err() && check_name("song 2.wav").is_ok());
    }
}

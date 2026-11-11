//! The text files a game reads (its level and legend, say), noted as it
//! reads them, so they can change while the game runs.
//!
//! [`TileMap::load_level`](crate::tilemap::TileMap::load_level) and
//! [`TileMap::load`](crate::tilemap::TileMap::load) read through here. The
//! engine then knows which files matter:
//!
//! - in a window, it looks at them twice a second and, when one changes on
//!   disk, calls the game's [`reload`](crate::engine::Game::reload) hook;
//! - Duck Forge Studio sends a changed file's new text over its link;
//! - a recording keeps each file's text as it was on frame 1 (its
//!   `"files"`), and every change on the frame it happened, so a replay
//!   gives the game the same text on the same frame, whatever is on disk
//!   now.
//!
//! What's noted belongs to the thread that read it: the engine runs a
//! game, and its tests, one thread each.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

/// The environment variable a tool (Duck Forge Studio's "Play from
/// here") sets to start a game with other text for some of its files: it
/// names a JSON file like `{"examples/mygame/level.txt": "..."}`. Those
/// texts are read instead of what's on disk, and a recording keeps them.
pub const GIVE_FILES_VARIABLE: &str = "DUCKFORGE_GIVE_FILES";

#[derive(Default)]
struct Files {
    /// The variable above has been looked at.
    looked_at_variable: bool,
    /// Text to give instead of what's on disk: from the Studio, or from a
    /// recording being replayed.
    given: BTreeMap<String, String>,
    /// Every file read, with the text it had then (the first time).
    first: BTreeMap<String, String>,
    /// The text the game was last given for each file.
    last: BTreeMap<String, String>,
    /// What was last seen on disk for each file, to notice it changing.
    on_disk: BTreeMap<String, String>,
}

thread_local! {
    static FILES: RefCell<Files> = RefCell::new(Files::default());
}

/// A path as the engine names it: forward slashes, no leading `./`. The
/// same file always has the same name, on Windows too.
pub(crate) fn name_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("./").unwrap_or(&text).to_string()
}

/// Reads a text file the game needs, and notes it. If the Studio or a
/// replay has given newer text for it, that's what's read.
pub(crate) fn read_text(path: &Path) -> io::Result<String> {
    take_given_from_variable();
    let name = name_of(path);
    let given = FILES.with(|files| files.borrow().given.get(&name).cloned());
    let text = match given {
        Some(text) => text,
        None => {
            let text = std::fs::read_to_string(path)?;
            FILES.with(|files| {
                files
                    .borrow_mut()
                    .on_disk
                    .insert(name.clone(), text.clone());
            });
            text
        }
    };
    FILES.with(|files| {
        let mut files = files.borrow_mut();
        files
            .first
            .entry(name.clone())
            .or_insert_with(|| text.clone());
        files.last.insert(name, text.clone());
    });
    Ok(text)
}

/// The first time a file is read: the texts a tool gave in
/// [`GIVE_FILES_VARIABLE`], if any.
fn take_given_from_variable() {
    let looked =
        FILES.with(|files| std::mem::replace(&mut files.borrow_mut().looked_at_variable, true));
    if looked {
        return;
    }
    let Some(file) = std::env::var_os(GIVE_FILES_VARIABLE) else {
        return;
    };
    let texts: Option<BTreeMap<String, String>> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    match texts {
        Some(texts) => FILES.with(|files| files.borrow_mut().given.extend(texts)),
        None => eprintln!(
            "duckforge: {GIVE_FILES_VARIABLE} names {}, which isn't a JSON object of file texts; \
             reading the files from disk",
            std::path::Path::new(&file).display()
        ),
    }
}

/// From now on, reading `name` gives `text`.
pub(crate) fn give(name: &str, text: &str) {
    FILES.with(|files| {
        files
            .borrow_mut()
            .given
            .insert(name.to_string(), text.to_string());
    });
}

/// A recording's files, as they were on frame 1: they're read instead of
/// what's on disk, and they count as the files' first text. Returns the
/// ones the game has already read with other text, so it can be asked to
/// read them again.
pub(crate) fn start_with(texts: &BTreeMap<String, String>) -> Vec<String> {
    FILES.with(|files| {
        let mut files = files.borrow_mut();
        let mut differ = Vec::new();
        for (name, text) in texts {
            if files.last.get(name).is_some_and(|last| last != text) {
                differ.push(name.clone());
            }
            files.given.insert(name.clone(), text.clone());
            files.first.insert(name.clone(), text.clone());
        }
        differ
    })
}

/// Every file read so far, with its text the first time (or as a replay
/// started it): a recording's `"files"`.
pub(crate) fn first_texts() -> BTreeMap<String, String> {
    FILES.with(|files| files.borrow().first.clone())
}

/// The names of the files read so far.
pub(crate) fn names() -> Vec<String> {
    FILES.with(|files| files.borrow().last.keys().cloned().collect())
}

/// Files changed on disk since they were last looked at: their names and
/// new text. Only files read from disk are looked at.
#[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window looks
pub(crate) fn changed_on_disk() -> Vec<(String, String)> {
    let seen: Vec<(String, String)> =
        FILES.with(|files| files.borrow().on_disk.clone().into_iter().collect());
    let mut changed = Vec::new();
    for (name, before) in seen {
        // A file being saved can be missing or half-written for a moment:
        // it's looked at again next time.
        let Ok(now) = std::fs::read_to_string(&name) else {
            continue;
        };
        if now != before {
            FILES.with(|files| {
                files.borrow_mut().on_disk.insert(name.clone(), now.clone());
            });
            changed.push((name, now));
        }
    }
    changed
}

/// Forgets the text given instead of what's on disk: the run that needed
/// it is over.
pub(crate) fn forget_given() {
    FILES.with(|files| files.borrow_mut().given.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_noted_and_can_be_given_newer_text() {
        let folder = std::env::temp_dir().join(format!("duckforge-files-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("level.txt");
        std::fs::write(&path, "p..#").unwrap();
        let name = name_of(&path);

        assert_eq!(read_text(&path).unwrap(), "p..#");
        assert!(names().contains(&name));
        give(&name, "p.=#");
        assert_eq!(read_text(&path).unwrap(), "p.=#", "the given text wins");
        assert_eq!(first_texts()[&name], "p..#", "the first text is kept");

        // Changed on disk: noticed once.
        std::fs::write(&path, "p###").unwrap();
        assert_eq!(changed_on_disk(), [(name.clone(), String::from("p###"))]);
        assert!(changed_on_disk().is_empty());

        forget_given();
        assert_eq!(read_text(&path).unwrap(), "p###");
        let differ = start_with(&BTreeMap::from([(name.clone(), String::from("p..#"))]));
        assert_eq!(
            differ,
            std::slice::from_ref(&name),
            "the game read other text"
        );
        assert_eq!(read_text(&path).unwrap(), "p..#");
        forget_given();
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn names_are_the_same_on_every_system() {
        assert_eq!(
            name_of(Path::new("./examples/a/level.txt")),
            "examples/a/level.txt"
        );
        assert_eq!(
            name_of(Path::new("examples\\a\\level.txt")),
            "examples/a/level.txt"
        );
    }
}

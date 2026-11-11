//! Undo and Redo: this session's edits, each one step that names the files
//! it changed, with their text before and after.
//!
//! Everything the Studio edits is a plain text file (the level, the tune
//! file), so one kind of step covers them all: undoing writes the
//! "before" text back, redoing writes the "after". A paint stroke is one
//! step however many cells it covers. A change made outside the Studio
//! (an editor, an agent) becomes a step too, so Undo never skips over it
//! by surprise.

use std::path::PathBuf;

/// One file's text before and after a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

/// One step for Undo: what it was, in a few words, and the files it changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub title: String,
    pub files: Vec<FileEdit>,
    /// Edits with the same stroke (one press of the mouse) join one step.
    pub stroke: Option<u64>,
}

/// The steps that can be undone, and the ones undone that can be redone.
#[derive(Debug, Default)]
pub struct Undo {
    done: Vec<Step>,
    undone: Vec<Step>,
}

/// How many steps Undo keeps.
const MOST_STEPS: usize = 500;

impl Undo {
    /// Notes a step. One with the same stroke as the last joins it (its
    /// files keep their first "before"). Anything undone can't be redone
    /// after a new step.
    pub fn push(&mut self, step: Step) {
        if step.files.iter().all(|edit| edit.before == edit.after) {
            return;
        }
        self.undone.clear();
        if let (Some(stroke), Some(last)) = (step.stroke, self.done.last_mut())
            && last.stroke == Some(stroke)
        {
            for edit in step.files {
                match last.files.iter_mut().find(|e| e.path == edit.path) {
                    Some(earlier) => earlier.after = edit.after,
                    None => last.files.push(edit),
                }
            }
            last.title = step.title;
            return;
        }
        self.done.push(step);
        if self.done.len() > MOST_STEPS {
            self.done.remove(0);
        }
    }

    /// The step to undo: its files go back to their "before".
    pub fn undo(&mut self) -> Option<Step> {
        let step = self.done.pop()?;
        self.undone.push(step.clone());
        Some(step)
    }

    /// The step to redo: its files go to their "after" again.
    pub fn redo(&mut self) -> Option<Step> {
        let step = self.undone.pop()?;
        self.done.push(step.clone());
        Some(step)
    }

    /// What Undo would undo, and Redo redo, in words.
    pub fn next_undo(&self) -> Option<&str> {
        self.done.last().map(|step| step.title.as_str())
    }

    pub fn next_redo(&self) -> Option<&str> {
        self.undone.last().map(|step| step.title.as_str())
    }

    /// Every step that can be undone, newest last.
    pub fn steps(&self) -> &[Step] {
        &self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(title: &str, before: &str, after: &str, stroke: Option<u64>) -> Step {
        Step {
            title: title.to_string(),
            files: vec![FileEdit {
                path: PathBuf::from("level.txt"),
                before: before.to_string(),
                after: after.to_string(),
            }],
            stroke,
        }
    }

    #[test]
    fn a_stroke_is_one_step_and_redo_ends_with_a_new_edit() {
        let mut undo = Undo::default();
        undo.push(step("Painted 1 tile", "a", "b", Some(1)));
        undo.push(step("Painted 2 tiles", "b", "c", Some(1)));
        undo.push(step("Nothing", "c", "c", None));
        assert_eq!(undo.steps().len(), 1);
        assert_eq!(undo.next_undo(), Some("Painted 2 tiles"));

        let undone = undo.undo().unwrap();
        assert_eq!(
            (
                undone.files[0].before.as_str(),
                undone.files[0].after.as_str()
            ),
            ("a", "c")
        );
        assert_eq!(undo.next_redo(), Some("Painted 2 tiles"));
        assert_eq!(undo.redo().unwrap().files[0].after, "c");

        undo.undo();
        undo.push(step("Erased", "a", "d", None));
        assert_eq!(undo.next_redo(), None, "a new edit ends redo");
    }
}

// ---- History: snapshots in plain folders ----

/// One snapshot of a game: `history/<game>/0007 Painted 14 tiles/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub number: u32,
    pub title: String,
    /// When it was taken, in seconds since 1970.
    pub at: u64,
    pub folder: PathBuf,
}

/// A game's history: snapshots of its files, taken by the Studio.
///
/// Each snapshot folder holds `files.txt` (every file the game had then,
/// with a short fingerprint of its contents), `about.json` (its title and
/// time) and, under `files/`, copies of the files that changed since the
/// snapshot before. So a snapshot is small, and any of them can be put
/// back: each file comes from the newest snapshot at or before it that
/// has a copy.
///
/// It covers the game's folder, its tune and feel files, and its
/// playthroughs. No git is needed or touched.
#[derive(Debug, Clone)]
pub struct History {
    /// The games folder.
    root: PathBuf,
    /// `history/<game>`.
    pub folder: PathBuf,
    game: String,
    game_folder: Option<PathBuf>,
    main: PathBuf,
}

/// How long snapshots are all kept; older ones are thinned to the last of
/// each day.
const KEEP_ALL_FOR: u64 = 7 * 24 * 60 * 60;

impl History {
    pub fn new(root: &std::path::Path, game: &crate::folder::Game) -> History {
        History {
            root: root.to_path_buf(),
            folder: root.join("history").join(&game.name),
            game: game.name.clone(),
            game_folder: game.folder.clone(),
            main: game.main.clone(),
        }
    }

    /// Every file a snapshot covers, from the games folder, with `/`.
    pub fn covered(&self) -> Vec<String> {
        let mut files = Vec::new();
        match &self.game_folder {
            Some(own) => all_files(own, &mut files),
            None => files.push(self.main.clone()),
        }
        let examples = self.root.join("examples");
        for end in ["tune.json", "feel.json"] {
            let file = examples.join(format!("{}.{end}", self.game));
            if file.is_file() {
                files.push(file);
            }
        }
        if let Ok(entries) = std::fs::read_dir(self.root.join("playthroughs")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let ours = name == format!("{}.json", self.game)
                    || (name.starts_with(&format!("{}_", self.game)) && name.ends_with(".json"));
                if ours {
                    files.push(entry.path());
                }
            }
        }
        let mut names: Vec<String> = files
            .iter()
            .filter_map(|file| file.strip_prefix(&self.root).ok())
            .map(|file| file.to_string_lossy().replace('\\', "/"))
            .collect();
        names.sort();
        names
    }

    /// The snapshots, oldest first.
    pub fn snapshots(&self) -> Vec<Snapshot> {
        let mut snapshots: Vec<Snapshot> = std::fs::read_dir(&self.folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let folder = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                let number: u32 = name.split(' ').next()?.parse().ok()?;
                let about: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(folder.join("about.json")).ok()?)
                        .ok()?;
                Some(Snapshot {
                    number,
                    title: about["title"].as_str().unwrap_or_default().to_string(),
                    at: about["at"].as_u64().unwrap_or(0),
                    folder,
                })
            })
            .collect();
        snapshots.sort_by_key(|snapshot| snapshot.number);
        snapshots
    }

    /// Every file's fingerprint in a snapshot.
    fn listed(snapshot: &Snapshot) -> std::collections::BTreeMap<String, String> {
        std::fs::read_to_string(snapshot.folder.join("files.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (print, name) = line.split_once(' ')?;
                Some((name.to_string(), print.to_string()))
            })
            .collect()
    }

    /// Takes a snapshot called `title`, if anything changed since the last
    /// one. Returns it, or `None` if nothing changed.
    pub fn take(&self, title: &str) -> Result<Option<Snapshot>, crate::problem::Problem> {
        let snapshots = self.snapshots();
        let before = snapshots.last().map(History::listed).unwrap_or_default();
        let now: std::collections::BTreeMap<String, String> = self
            .covered()
            .into_iter()
            .filter_map(|name| {
                let bytes = std::fs::read(self.root.join(&name)).ok()?;
                Some((name, fingerprint(&bytes)))
            })
            .collect();
        if now == before {
            return Ok(None);
        }
        let number = snapshots.last().map_or(1, |last| last.number + 1);
        let folder = self
            .folder
            .join(format!("{number:04} {}", safe_title(title)));
        let cant = |error: std::io::Error| {
            crate::problem::Problem::new(
                "The Studio couldn't save a snapshot in the game's history.",
                "Check there's space on the disk. Your files are fine.",
            )
            .with_details(error.to_string())
        };
        std::fs::create_dir_all(folder.join("files")).map_err(cant)?;
        for (name, print) in &now {
            if before.get(name) != Some(print) {
                let copy = folder.join("files").join(name);
                if let Some(parent) = copy.parent() {
                    std::fs::create_dir_all(parent).map_err(cant)?;
                }
                std::fs::copy(self.root.join(name), copy).map_err(cant)?;
            }
        }
        let list: String = now
            .iter()
            .map(|(name, print)| format!("{print} {name}\n"))
            .collect();
        std::fs::write(folder.join("files.txt"), list).map_err(cant)?;
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let about = serde_json::json!({"title": title, "at": at}).to_string();
        std::fs::write(folder.join("about.json"), about).map_err(cant)?;
        self.thin(at);
        Ok(Some(Snapshot {
            number,
            title: title.to_string(),
            at,
            folder,
        }))
    }

    /// Puts the game's files back as they were in snapshot `number`. A
    /// snapshot of now is taken first, so going back can be undone.
    /// Returns the files that changed.
    pub fn go_back_to(&self, number: u32) -> Result<Vec<PathBuf>, crate::problem::Problem> {
        self.take("Before going back")?;
        let snapshots = self.snapshots();
        let Some(index) = snapshots.iter().position(|s| s.number == number) else {
            return Err(crate::problem::Problem::new(
                format!("There's no snapshot {number} in the history."),
                "Pick one from the History list.",
            ));
        };
        let wanted = History::listed(&snapshots[index]);
        let mut changed = Vec::new();
        let cant = |error: std::io::Error| {
            crate::problem::Problem::new(
                "The Studio couldn't put the files back.",
                "Check no other program has them open, then try again.",
            )
            .with_details(error.to_string())
        };
        for (name, print) in &wanted {
            let path = self.root.join(name);
            let now = std::fs::read(&path).ok().map(|bytes| fingerprint(&bytes));
            if now.as_ref() == Some(print) {
                continue;
            }
            // The newest copy at or before the snapshot.
            let copy = snapshots[..=index]
                .iter()
                .rev()
                .map(|s| s.folder.join("files").join(name))
                .find(|copy| copy.is_file());
            if let Some(copy) = copy {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(cant)?;
                }
                std::fs::copy(copy, &path).map_err(cant)?;
                changed.push(path);
            }
        }
        // Files that weren't there then go (the snapshot just taken keeps them).
        for name in self.covered() {
            if !wanted.contains_key(&name) {
                let path = self.root.join(&name);
                std::fs::remove_file(&path).map_err(cant)?;
                changed.push(path);
            }
        }
        Ok(changed)
    }

    /// Snapshots older than a week are thinned to the last of each day.
    /// The copies of a removed one move to the next, so every snapshot
    /// left can still be put back.
    fn thin(&self, now: u64) {
        let snapshots = self.snapshots();
        for pair in snapshots.windows(2) {
            let (this, next) = (&pair[0], &pair[1]);
            let old = now.saturating_sub(this.at) > KEEP_ALL_FOR;
            let same_day = this.at / 86_400 == next.at / 86_400;
            if !(old && same_day) {
                continue;
            }
            let mut copies = Vec::new();
            all_files(&this.folder.join("files"), &mut copies);
            for copy in copies {
                let Ok(name) = copy.strip_prefix(this.folder.join("files")) else {
                    continue;
                };
                let into = next.folder.join("files").join(name);
                if !into.exists() {
                    if let Some(parent) = into.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::rename(&copy, &into);
                }
            }
            let _ = std::fs::remove_dir_all(&this.folder);
        }
    }
}

/// A short fingerprint of a file's contents (FNV-1a, 64 bits), to notice
/// changes. Not for security: only to tell versions apart.
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A title that's safe as a folder name on every system.
fn safe_title(title: &str) -> String {
    let safe: String = title
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let safe = safe.trim().trim_end_matches('.').to_string();
    safe.chars().take(60).collect()
}

/// Every file under `folder` (not `target` or `out` folders).
fn all_files(folder: &std::path::Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name != "target" && name != "out" {
                all_files(&path, files);
            }
        } else {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn snapshots_keep_changes_and_going_back_puts_files_back() {
        let root = std::env::temp_dir().join(format!("studio-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("examples/bee/assets")).unwrap();
        std::fs::create_dir_all(root.join("playthroughs")).unwrap();
        let write = |name: &str, text: &str| std::fs::write(root.join(name), text).unwrap();
        write("examples/bee/main.rs", "fn main() {}");
        write("examples/bee/level.txt", "p..#\n");
        write("examples/bee/assets/tiles.png", "picture");
        write("examples/bee.tune.json", "{}");
        write("playthroughs/bee.json", "{\"events\": []}");
        write("playthroughs/beetle.json", "not ours");
        let game = crate::folder::Game {
            name: String::from("bee"),
            title: String::from("Bee"),
            main: root.join("examples/bee/main.rs"),
            folder: Some(root.join("examples/bee")),
            came_with: false,
        };
        let history = History::new(&root, &game);
        assert_eq!(history.covered().len(), 5, "{:?}", history.covered());

        let first = history.take("When it was first opened").unwrap().unwrap();
        assert_eq!(first.number, 1);
        assert!(
            history.take("Nothing new").unwrap().is_none(),
            "nothing changed"
        );
        write("examples/bee/level.txt", "p.=#\n");
        write("examples/bee/new.txt", "added");
        let second = history.take("Painted 1 tile").unwrap().unwrap();
        assert_eq!(second.number, 2);
        assert!(second.folder.join("files/examples/bee/level.txt").is_file());
        assert!(
            !second.folder.join("files/examples/bee/main.rs").exists(),
            "only what changed"
        );
        let titles: Vec<String> = history
            .snapshots()
            .iter()
            .map(|s| s.title.clone())
            .collect();
        assert_eq!(titles, ["When it was first opened", "Painted 1 tile"]);

        let changed = history.go_back_to(1).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("examples/bee/level.txt")).unwrap(),
            "p..#\n"
        );
        assert!(
            !root.join("examples/bee/new.txt").exists(),
            "it wasn't there then"
        );
        assert_eq!(changed.len(), 2);
        assert_eq!(
            history.snapshots().len(),
            2,
            "now was already snapshot 2, so no new one was needed"
        );
        history.go_back_to(2).unwrap();
        assert_eq!(
            history.snapshots().len(),
            3,
            "a snapshot of before going back"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("examples/bee/level.txt")).unwrap(),
            "p.=#\n"
        );
        assert!(
            root.join("examples/bee/new.txt").exists(),
            "going back is undone"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

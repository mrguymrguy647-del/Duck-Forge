//! Tuning: numbers a game reads by name, so they can be changed without
//! rebuilding.
//!
//! In code, a game asks for a number and gives its default:
//!
//! ```
//! # use duckforge::prelude::*;
//! # fn update(ctx: &mut Context) {
//! let jump_speed = ctx.tune("jump_speed", 330.0);
//! # }
//! ```
//!
//! The value comes from, in order:
//!
//! 1. `--set jump_speed=400` on the command line (repeatable),
//! 2. a tune file named on the command line with `--tune FILE`,
//! 3. during a replay, the value the playthrough was recorded with (its
//!    `"tune"` block), so it plays out as it was recorded,
//! 4. the game's tune file: `examples/<game>.tune.json`, a JSON object
//!    like `{"jump_speed": 400, "gravity": 900}`,
//! 5. the default in the code.
//!
//! In a window, the tune file is read again whenever it changes, so you
//! can tweak a running game. `log.json` records every value used and where
//! it came from, and `--write-tune` writes a tune file listing every name
//! the game asked for, with its current value.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

/// Where a tuned value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Default,
    File,
    Set,
    /// The `"tune"` block of the playthrough being replayed.
    Recording,
    /// Changed while the game ran: Duck Forge Studio's Feel, the tune
    /// file edited in a window, or such a change being replayed.
    Changed,
}

impl Source {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Source::Default => "default",
            Source::File => "file",
            Source::Set => "set",
            Source::Recording => "recording",
            Source::Changed => "changed while running",
        }
    }
}

/// The tune values for one run: from the file, from `--set`, and the ones
/// the game has asked for so far.
#[derive(Debug, Default)]
pub(crate) struct Tunes {
    /// The tune file: the one given with `--tune`, or the default one.
    path: Option<PathBuf>,
    /// Values from the file.
    file: HashMap<String, f32>,
    /// Values from `--set`.
    set: HashMap<String, f32>,
    /// The tune file was named on the command line (`--tune FILE`): it
    /// wins over a recording's values.
    file_on_command_line: bool,
    /// The values the playthrough being replayed was recorded with.
    recorded: HashMap<String, f32>,
    /// Every name the game asked for: the value it got, from where, and
    /// the default written in the code.
    /// (A `RefCell` lets `ctx.tune` take `&self`, so it can be used inside
    /// another call on `ctx`, like `ctx.report("x", ctx.tune("x", 1.0))`.)
    used: RefCell<BTreeMap<String, (f32, Source, f32)>>,
    /// When the file was last changed, to notice edits.
    modified: Option<SystemTime>,
    /// Values changed while the game runs. They win over everything.
    changed: HashMap<String, f32>,
    /// The values in use just before the first change, for a recording's
    /// `"tune"` block: a replay starts with them, then makes the changes.
    before_changes: Option<BTreeMap<String, f32>>,
}

impl Tunes {
    /// Tune values from the file at `path` (or the default file, if there
    /// is one) and the `--set` values. A missing default file is fine (all
    /// defaults); a missing file named with `--tune` is an error.
    pub(crate) fn load(path: Option<&Path>, set: &[(String, f32)]) -> Result<Tunes, String> {
        Tunes::load_from(path, path.is_some(), set)
    }

    /// Like [`load`](Tunes::load), for the file a game names in
    /// `Config::tune_file`: it's fine for it not to exist yet (the game
    /// then uses the values in its code).
    pub(crate) fn load_named(path: &Path, set: &[(String, f32)]) -> Result<Tunes, String> {
        Tunes::load_from(Some(path), false, set)
    }

    /// The tune values for a run: from `--tune FILE` if given (which must
    /// exist), else the file the game names in its `Config` (which may not
    /// exist yet), else `examples/<game>.tune.json`; then `--set` on top.
    pub(crate) fn for_run(
        command_line: Option<&Path>,
        config: Option<&Path>,
        set: &[(String, f32)],
    ) -> Result<Tunes, String> {
        let mut tunes = match (command_line, config) {
            (Some(file), _) => Tunes::load(Some(file), set)?,
            (None, Some(named)) => Tunes::load_named(named, set)?,
            (None, None) => Tunes::load(None, set)?,
        };
        tunes.file_on_command_line = command_line.is_some();
        Ok(tunes)
    }

    /// Replays use the values a playthrough was recorded with (its
    /// `"tune"` block) over the game's tune file and its code, so a
    /// recording made with `--set start_time_of_day=0.47` replays the same
    /// way without it. `--set` and `--tune FILE` still win.
    pub(crate) fn use_recording(&mut self, recorded: &BTreeMap<String, f32>) {
        self.recorded = recorded.iter().map(|(n, &v)| (n.clone(), v)).collect();
    }

    fn load_from(
        path: Option<&Path>,
        explicit: bool,
        set: &[(String, f32)],
    ) -> Result<Tunes, String> {
        let path = path.map(Path::to_path_buf).or_else(default_path);
        let mut tunes = Tunes {
            set: set.iter().cloned().collect(),
            ..Tunes::default()
        };
        if let Some(path) = path {
            if path.exists() {
                tunes.file = read_file(&path)?;
                tunes.modified = modified(&path);
            } else if explicit {
                return Err(format!(
                    "couldn't find the tune file {} (from --tune); check the path, or leave \
                     --tune out to use examples/<game>.tune.json",
                    path.display()
                ));
            }
            tunes.path = Some(path);
        }
        Ok(tunes)
    }

    /// The value for `name`: from `--set`, `--tune FILE`, the recording
    /// being replayed, the game's tune file, or `default`, in that order.
    pub(crate) fn get(&self, name: &str, default: f32) -> f32 {
        let (value, source) = match self.changed.get(name) {
            Some(&v) => (v, Source::Changed),
            None => self.underlying(name, default),
        };
        // Only allocate the first time a name is used.
        let mut used = self.used.borrow_mut();
        match used.get_mut(name) {
            Some(entry) => *entry = (value, source, default),
            None => {
                used.insert(name.to_string(), (value, source, default));
            }
        }
        value
    }

    /// Changes `name` to `value` from now on, over every other source.
    pub(crate) fn change(&mut self, name: &str, value: f32) {
        if self.before_changes.is_none() {
            self.before_changes = Some(
                self.used()
                    .into_iter()
                    .map(|(name, value, _)| (name, value))
                    .collect(),
            );
        }
        self.changed.insert(name.to_string(), value);
        // The game sees it next time it asks; tell the summary now.
        if let Some(entry) = self.used.borrow_mut().get_mut(name) {
            entry.0 = value;
            entry.1 = Source::Changed;
        }
    }

    /// The values used, as they were before anything changed while the
    /// game ran.
    pub(crate) fn at_start(&self) -> BTreeMap<String, f32> {
        let mut values: BTreeMap<String, f32> = self
            .used()
            .into_iter()
            .map(|(name, value, _)| (name, value))
            .collect();
        if let Some(before) = &self.before_changes {
            for (name, value) in values.iter_mut() {
                if let Some(old) = before.get(name) {
                    *value = *old;
                } else if self.changed.contains_key(name) {
                    // First asked for after it was changed: what it would
                    // have been.
                    let default = self.code_default(name).unwrap_or(*value);
                    *value = self.underlying(name, default).0;
                }
            }
        }
        values
    }

    /// Values the tune file now gives that differ from what the game has:
    /// after the file was edited, these are the changes to make.
    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window reloads
    pub(crate) fn differences_from_file(&self) -> BTreeMap<String, f32> {
        self.used
            .borrow()
            .iter()
            .filter_map(|(name, &(value, _, default))| {
                let (wanted, _) = self.underlying(name, default);
                (wanted != value).then(|| (name.clone(), wanted))
            })
            .collect()
    }

    /// The value for `name` without the changes made while running.
    fn underlying(&self, name: &str, default: f32) -> (f32, Source) {
        let from_file = self.file.get(name).copied();
        if let Some(&v) = self.set.get(name) {
            (v, Source::Set)
        } else if let (true, Some(v)) = (self.file_on_command_line, from_file) {
            (v, Source::File)
        } else {
            let (v, source) = match from_file {
                Some(v) => (v, Source::File),
                None => (default, Source::Default),
            };
            match self.recorded.get(name) {
                // Only a value that changes something counts as the
                // recording's (the same value stays the file's or the code's).
                Some(&recorded) if recorded != v => (recorded, Source::Recording),
                _ => (v, source),
            }
        }
    }

    /// Reads the file again if it changed since it was last read. Returns
    /// what happened, for the window to print: `None` if nothing changed.
    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window reloads
    pub(crate) fn reload_if_changed(&mut self) -> Option<Result<String, String>> {
        let path = self.path.clone()?;
        let now = modified(&path);
        if now.is_none() || now == self.modified {
            return None;
        }
        self.modified = now;
        Some(read_file(&path).map(|values| {
            let count = values.len();
            self.file = values;
            format!("reloaded {} ({count} values)", path.display())
        }))
    }

    /// Every name the game asked for, with its value and where it came from.
    pub(crate) fn used(&self) -> Vec<(String, f32, Source)> {
        self.used
            .borrow()
            .iter()
            .map(|(name, &(value, source, _))| (name.clone(), value, source))
            .collect()
    }

    /// The default the code gave for `name` (the second argument of
    /// `ctx.tune`), if the game asked for it.
    pub(crate) fn code_default(&self, name: &str) -> Option<f32> {
        self.used.borrow().get(name).map(|&(_, _, default)| default)
    }

    /// `--set` names the game never asked for: probably typos.
    pub(crate) fn unused_sets(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .set
            .keys()
            .filter(|name| !self.used.borrow().contains_key(*name))
            .cloned()
            .collect();
        names.sort();
        names
    }

    /// Names in the tune file the game never asked for: left over from an
    /// older version (or a copied game), or misspelt.
    pub(crate) fn unused_file_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .file
            .keys()
            .filter(|name| !self.used.borrow().contains_key(*name))
            .cloned()
            .collect();
        names.sort();
        names
    }

    /// The tune file's path, if there is one.
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The tune section of `log.json`:
    /// `{"gravity": {"value": 900, "from": "file"}, ...}`.
    pub(crate) fn to_json(&self) -> String {
        let entries: Vec<String> = self
            .used()
            .into_iter()
            .map(|(name, value, source)| {
                format!(
                    "{}: {{\"value\": {}, \"from\": \"{}\"}}",
                    Value::from(name),
                    number(value),
                    source.name()
                )
            })
            .collect();
        format!("{{{}}}", entries.join(", "))
    }

    /// Writes every name the game asked for, with its current value, to
    /// the tune file (keeping values in the file the game didn't ask for
    /// this time). Returns the path written.
    pub(crate) fn write_file(&self) -> Result<PathBuf, String> {
        let path = self.path.clone().ok_or_else(|| {
            String::from("--write-tune doesn't know where to write; give the file with --tune FILE")
        })?;
        let mut all: BTreeMap<String, f32> = self.file.clone().into_iter().collect();
        for (name, value, _) in self.used() {
            all.insert(name, value);
        }
        let lines: Vec<String> = all
            .iter()
            .map(|(name, &value)| format!("  {}: {}", Value::from(name.as_str()), number(value)))
            .collect();
        let text = format!("{{\n{}\n}}\n", lines.join(",\n"));
        if let Some(folder) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(folder)
                .map_err(|err| format!("couldn't create {}: {err}", folder.display()))?;
        }
        fs::write(&path, text)
            .map_err(|err| format!("couldn't write {}: {err}", path.display()))?;
        Ok(path)
    }
}

/// A number for JSON, without a trailing `.0` and without float noise:
/// 0.3 stays 0.3, 400.0 becomes 400.
fn number(value: f32) -> String {
    let text = format!("{value}");
    if value.is_finite() {
        text
    } else {
        String::from("0")
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Reads a tune file: a JSON object of names and numbers.
fn read_file(path: &Path) -> Result<HashMap<String, f32>, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("couldn't read the tune file {}: {err}", path.display()))?;
    let json: Value = serde_json::from_str(&text).map_err(|err| {
        format!(
            "the tune file {} isn't valid JSON ({err}); it should look like {{\"jump_speed\": 330, \"gravity\": 900}}",
            path.display()
        )
    })?;
    let Value::Object(map) = json else {
        return Err(format!(
            "the tune file {} should be a JSON object like {{\"jump_speed\": 330}}",
            path.display()
        ));
    };
    let mut values = HashMap::new();
    for (name, value) in map {
        let Some(number) = value.as_f64() else {
            return Err(format!(
                "in the tune file {}, {name:?} is {value}, but tune values must be numbers",
                path.display()
            ));
        };
        values.insert(name, number as f32);
    }
    Ok(values)
}

/// `examples/<game>.tune.json`, where `<game>` is this program's name
/// (`target/debug/examples/dodge` is `dodge`; test programs, named like
/// `dodge-1a2b3c4d5e6f7a8b`, lose the `-` and the hex number).
pub(crate) fn default_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_string_lossy().into_owned();
    Some(PathBuf::from("examples").join(format!("{}.tune.json", game_name(&stem))))
}

/// This program's game name: its file name without Cargo's test hash.
pub(crate) fn game_name_of_this_program() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_string_lossy().into_owned();
    Some(game_name(&stem).to_string())
}

/// A program's name without the hash Cargo adds to test programs.
fn game_name(stem: &str) -> &str {
    match stem.rsplit_once('-') {
        Some((name, hash)) if hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {
            name
        }
        _ => stem,
    }
}

/// Reads `name=value` from `--set`.
pub(crate) fn parse_set(text: &str) -> Result<(String, f32), String> {
    let problem = || format!("--set needs NAME=NUMBER, like --set jump_speed=400, not {text:?}");
    let (name, value) = text.split_once('=').ok_or_else(problem)?;
    let value: f32 = value.trim().parse().map_err(|_| problem())?;
    let name = name.trim();
    if name.is_empty() || !value.is_finite() {
        return Err(problem());
    }
    Ok((name.to_string(), value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, text: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("duckforge-tune-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_recording_beats_the_games_file_but_not_the_command_line() {
        let file = temp_file("order.tune.json", r#"{"a": 1, "b": 1, "c": 1}"#);
        let recorded: BTreeMap<String, f32> = [("a", 5.0), ("b", 5.0), ("d", 5.0), ("e", 2.0)]
            .map(|(n, v)| (n.to_string(), v))
            .into();
        let set = [(String::from("a"), 9.0)];

        // The game's own file (named in its Config): the recording wins
        // over it and over the code, and --set wins over everything.
        let mut tunes = Tunes::for_run(None, Some(&file), &set).unwrap();
        tunes.use_recording(&recorded);
        assert_eq!(tunes.get("a", 0.0), 9.0, "--set");
        assert_eq!(tunes.get("b", 0.0), 5.0, "the recording over the file");
        assert_eq!(tunes.get("c", 0.0), 1.0, "not recorded: the file");
        assert_eq!(tunes.get("d", 0.0), 5.0, "the recording over the code");
        assert_eq!(tunes.get("e", 2.0), 2.0, "the same as the code");
        let sources: BTreeMap<String, Source> =
            tunes.used().into_iter().map(|(n, _, s)| (n, s)).collect();
        assert_eq!(sources["b"], Source::Recording);
        assert_eq!(sources["d"], Source::Recording);
        assert_eq!(
            sources["e"],
            Source::Default,
            "a recorded value that changes nothing"
        );

        // A file given with --tune wins over the recording.
        let mut tunes = Tunes::for_run(Some(&file), None, &[]).unwrap();
        tunes.use_recording(&recorded);
        assert_eq!(tunes.get("b", 0.0), 1.0);
        assert_eq!(tunes.get("d", 0.0), 5.0, "not in the file: the recording");
    }

    #[test]
    fn set_beats_file_beats_default() {
        let path = temp_file("order.tune.json", r#"{"speed": 2, "gravity": 900.5}"#);
        let set = [(String::from("speed"), 3.0)];
        let tunes = Tunes::load(Some(&path), &set).unwrap();
        assert_eq!(tunes.get("speed", 1.0), 3.0);
        assert_eq!(tunes.get("gravity", 1.0), 900.5);
        assert_eq!(tunes.get("lives", 3.0), 3.0);
        assert_eq!(
            tunes.to_json(),
            r#"{"gravity": {"value": 900.5, "from": "file"}, "lives": {"value": 3, "from": "default"}, "speed": {"value": 3, "from": "set"}}"#
        );
    }

    #[test]
    fn mistakes_get_one_sentence() {
        let missing = Tunes::load(Some(Path::new("no/such.tune.json")), &[]).unwrap_err();
        assert!(
            missing.contains("couldn't find the tune file no/such.tune.json"),
            "{missing}"
        );
        let text = temp_file("text.tune.json", r#"{"speed": "fast"}"#);
        let error = Tunes::load(Some(&text), &[]).unwrap_err();
        assert!(
            error.contains(r#""speed" is "fast", but tune values must be numbers"#),
            "{error}"
        );
        let broken = temp_file("broken.tune.json", "{speed: 2");
        assert!(
            Tunes::load(Some(&broken), &[])
                .unwrap_err()
                .contains("isn't valid JSON")
        );
        assert_eq!(parse_set("jump = 4.5"), Ok((String::from("jump"), 4.5)));
        assert!(parse_set("jump").unwrap_err().contains("NAME=NUMBER"));
        assert!(parse_set("jump=high").is_err());
        assert!(parse_set("=3").is_err());
    }

    #[test]
    fn unused_sets_are_typos() {
        let set = [
            (String::from("jump_sped"), 3.0),
            (String::from("jump"), 1.0),
        ];
        let tunes = Tunes::load(Some(&temp_file("empty.tune.json", "{}")), &set).unwrap();
        tunes.get("jump", 0.0);
        assert_eq!(tunes.unused_sets(), ["jump_sped"]);
    }

    #[test]
    fn writing_keeps_old_names_and_adds_new_ones() {
        let path = temp_file("write.tune.json", r#"{"old": 1, "speed": 2}"#);
        let tunes = Tunes::load(Some(&path), &[]).unwrap();
        tunes.get("speed", 9.0);
        tunes.get("new", 0.25);
        tunes.write_file().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"new\": 0.25,\n  \"old\": 1,\n  \"speed\": 2\n}\n"
        );
    }

    #[test]
    fn edits_are_noticed() {
        let path = temp_file("live.tune.json", r#"{"speed": 2}"#);
        let mut tunes = Tunes::load(Some(&path), &[]).unwrap();
        assert!(tunes.reload_if_changed().is_none(), "nothing changed yet");
        tunes.modified = Some(SystemTime::UNIX_EPOCH); // pretend it was read long ago
        fs::write(&path, r#"{"speed": 5}"#).unwrap();
        assert!(
            tunes
                .reload_if_changed()
                .unwrap()
                .unwrap()
                .contains("1 values")
        );
        assert_eq!(tunes.get("speed", 0.0), 5.0);
    }

    #[test]
    fn game_names_lose_the_test_hash() {
        assert_eq!(game_name("dodge"), "dodge");
        assert_eq!(game_name("dodge-1a2b3c4d5e6f7a8b"), "dodge");
        assert_eq!(
            game_name("duckling_parade-0123456789abcdef"),
            "duckling_parade"
        );
        assert_eq!(game_name("my-game"), "my-game");
    }
}

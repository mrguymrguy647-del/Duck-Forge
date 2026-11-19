//! A game's sounds as the Inspector lists them, the facts Duck Echo
//! writes beside each one, the recipes that made them, and Duck Echo
//! Studio, which opens a recipe to change it.
//!
//! The Sound box lists `.wav` sounds, `.ogg` music ("(music)") and Duck
//! Echo's sets of variants as one entry each ("step (6 variants)"): the
//! numbered `.wav`s a set names are its, and aren't listed one by one.
//! Under the box, a facts line: how long, how loud, where the hit is,
//! whether it loops, and the recipe that made it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::console::CommandLine;
use crate::problem::Problem;

/// Where to get Duck Echo Studio.
pub const DUCK_ECHO_PAGE: &str = "https://github.com/mrguymrguy647-del/Duck-Echo/releases";

/// What a sound file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundKind {
    /// A `.wav`.
    Sound,
    /// An `.ogg`: music, which loops.
    Music,
    /// A Duck Echo `.variants.json`: one of its sounds each time.
    Set { variants: usize },
}

/// One entry of the Sound box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundChoice {
    /// As a scene names it: `assets/step.variants.json`.
    pub path: String,
    /// As the box shows it: `assets/step (6 variants)`.
    pub label: String,
    pub kind: SoundKind,
}

/// The game's sounds, music and sets, as scene files name them, sorted:
/// the numbered sounds a set names are left out (the set stands for them).
pub fn choices(folder: &Path) -> Vec<SoundChoice> {
    let mut files = Vec::new();
    collect(folder, folder, 0, &mut files);
    // The sounds each set names, in the set's own folder.
    let mut in_sets = Vec::new();
    let mut sets = BTreeMap::new();
    for path in files.iter().filter(|p| p.ends_with(".variants.json")) {
        let names = set_files(&folder.join(path));
        let here = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        for name in names.iter().flatten() {
            in_sets.push(if here.is_empty() {
                name.clone()
            } else {
                format!("{here}/{name}")
            });
        }
        sets.insert(path.clone(), names.map(|n| n.len()));
    }
    let mut out: Vec<SoundChoice> = files
        .into_iter()
        .filter(|path| !in_sets.contains(path))
        .filter_map(|path| {
            let lower = path.to_ascii_lowercase();
            if let Some(stem) = path.strip_suffix(".variants.json") {
                let (kind, label) = match sets.get(&path).copied().flatten() {
                    Some(n) => (
                        SoundKind::Set { variants: n },
                        format!("{stem} ({n} variant{})", if n == 1 { "" } else { "s" }),
                    ),
                    None => (
                        SoundKind::Set { variants: 0 },
                        format!("{stem} (a set that can't be read)"),
                    ),
                };
                Some(SoundChoice { path, label, kind })
            } else if lower.ends_with(".ogg") {
                Some(SoundChoice {
                    label: format!("{path} (music)"),
                    path,
                    kind: SoundKind::Music,
                })
            } else if lower.ends_with(".wav") {
                Some(SoundChoice {
                    label: path.clone(),
                    path,
                    kind: SoundKind::Sound,
                })
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Sound files and sets under `here`, as `assets/thud.wav`.
fn collect(root: &Path, here: &Path, depth: usize, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(here) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if depth < 3 && !name.starts_with('.') && name != "target" {
                collect(root, &path, depth + 1, found);
            }
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".wav") || lower.ends_with(".ogg") || lower.ends_with(".variants.json") {
            found.push(place_of(root, &path));
        }
    }
}

/// `path` as the game's folder names it: `assets/thud.wav`.
fn place_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The files a Duck Echo set lists, or `None` if it can't be read.
fn set_files(list: &Path) -> Option<Vec<String>> {
    let value = read_json(list)?;
    let files = value["files"].as_array()?;
    files
        .iter()
        .map(|f| f["file"].as_str().map(String::from))
        .collect()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// What Duck Echo wrote about a sound (its facts file), or about a set
/// (its list): ranges for a set, one value for one sound.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    /// How many variants, for a set.
    pub variants: Option<usize>,
    /// How long, in seconds: the shortest and the longest.
    pub seconds: Option<(f64, f64)>,
    /// How loud, in LUFS: the quietest and the loudest.
    pub lufs: Option<(f64, f64)>,
    /// Where the hit is, in seconds from the start: earliest and latest.
    pub hit: Option<(f64, f64)>,
    /// Whether it loops (music does).
    pub loops: Option<bool>,
    /// The recipe that made it, as the game's folder names it.
    pub recipe: Option<String>,
}

/// Widens `range` to hold `value`.
fn widen(range: &mut Option<(f64, f64)>, value: Option<f64>) {
    if let Some(v) = value.filter(|v| v.is_finite()) {
        *range = Some(match *range {
            Some((lo, hi)) => (lo.min(v), hi.max(v)),
            None => (v, v),
        });
    }
}

/// The facts of the sound, music or set at `path` in the game's `folder`:
/// from `assets/thud.wav.json` beside a sound, or from a set's list (and
/// its first sound's facts, for whether it loops). `None` if Duck Echo
/// wrote none.
pub fn facts(folder: &Path, path: &str) -> Option<Facts> {
    let file = folder.join(path);
    if path.ends_with(".variants.json") {
        let list = read_json(&file)?;
        let mut facts = Facts {
            recipe: list["recipe"].as_str().map(String::from),
            ..Facts::default()
        };
        let files = list["files"].as_array()?;
        facts.variants = Some(files.len());
        for f in files {
            widen(&mut facts.seconds, f["duration_seconds"].as_f64());
            widen(&mut facts.lufs, f["loudness_lufs"].as_f64());
            widen(&mut facts.hit, f["hit_seconds"].as_f64());
        }
        if let Some(range) = list["loudness_range_lufs"].as_array() {
            for v in range {
                widen(&mut facts.lufs, v.as_f64());
            }
        }
        // Whether it loops: the first sound's own facts say.
        let first = files.first().and_then(|f| f["file"].as_str());
        if let (Some(first), Some(dir)) = (first, file.parent()) {
            facts.loops = read_json(&dir.join(format!("{first}.json")))
                .and_then(|one| one["loops"].as_bool());
        }
        return Some(facts);
    }
    let said = read_json(&PathBuf::from(format!("{}.json", file.display())))?;
    if said["kind"].as_str() != Some("duck-echo-asset") {
        return None;
    }
    let mut facts = Facts {
        loops: said["loops"].as_bool(),
        recipe: said["recipe"].as_str().map(String::from),
        ..Facts::default()
    };
    widen(&mut facts.seconds, said["duration_seconds"].as_f64());
    widen(&mut facts.lufs, said["loudness_lufs"].as_f64());
    widen(&mut facts.hit, said["hit_seconds"].as_f64());
    Some(facts)
}

impl Facts {
    /// The facts line under the Sound box: `0.20 s  -16.0 LUFS  hit at
    /// 0.014 s  plays once  from art/thud.toml`.
    pub fn line(&self) -> String {
        let range = |(lo, hi): (f64, f64), digits: usize, unit: &str| {
            if (hi - lo).abs() < 0.5 * 10f64.powi(-(digits as i32)) {
                format!("{lo:.digits$}{unit}")
            } else {
                format!("{lo:.digits$} to {hi:.digits$}{unit}")
            }
        };
        let mut parts = Vec::new();
        if let Some(n) = self.variants {
            parts.push(format!("{n} variant{}", if n == 1 { "" } else { "s" }));
        }
        if let Some(seconds) = self.seconds {
            parts.push(range(seconds, 2, " s"));
        }
        if let Some(lufs) = self.lufs {
            parts.push(range(lufs, 1, " LUFS"));
        }
        if let Some(hit) = self.hit {
            parts.push(format!("hit at {}", range(hit, 3, " s")));
        }
        match self.loops {
            Some(true) => parts.push(String::from("loops")),
            Some(false) => parts.push(String::from("plays once")),
            None => {}
        }
        if let Some(recipe) = &self.recipe {
            parts.push(format!("from {recipe}"));
        }
        parts.join("  ")
    }
}

/// Whether the file at `path` is a Duck Echo recipe: a sound (`layers`)
/// or a song (`tempo` and `tracks`), with its `version` at the top.
pub fn is_recipe(path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
    {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    // Only the lines before the first [table] are the top level.
    let mut version = false;
    let mut body = false;
    for line in text.lines().map(str::trim) {
        let key = line.split('=').next().unwrap_or("").trim();
        if line.starts_with('[') {
            body |= line.starts_with("[[layers]]") || line.starts_with("[[tracks]]");
            if !version {
                return false;
            }
            continue;
        }
        if key == "version" {
            version = true;
        }
        body |= key == "layers" || key == "tempo" || key == "tracks";
    }
    version && body
}

/// The recipe that made the sound, music or set at `path` (as the game's
/// folder names it, `art/step.toml`): the one its facts name, or else
/// `art/<name>.toml` if that is a Duck Echo recipe. `None` if there's
/// none in the game's folder. A recipe is its own recipe.
pub fn recipe_of(folder: &Path, path: &str) -> Option<String> {
    if path.to_ascii_lowercase().ends_with(".toml") {
        return is_recipe(&folder.join(path)).then(|| path.to_string());
    }
    if let Some(recipe) = facts(folder, path).and_then(|f| f.recipe)
        && folder.join(&recipe).is_file()
    {
        return Some(recipe);
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name
        .strip_suffix(".variants.json")
        .or_else(|| name.rsplit_once('.').map(|(stem, _)| stem))?;
    let guess = format!("art/{stem}.toml");
    is_recipe(&folder.join(&guess)).then_some(guess)
}

/// Which recipe made which sound, for the Files panel: each sound's
/// recipe, and each recipe's sounds (a set, not its numbered sounds).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recipes {
    pub of_sound: BTreeMap<String, String>,
    pub sounds_of: BTreeMap<String, Vec<String>>,
}

/// The recipes of the game in `folder` and the sounds they made.
pub fn recipes(folder: &Path) -> Recipes {
    let mut found = Recipes::default();
    let mut all = Vec::new();
    collect(folder, folder, 0, &mut all);
    let listed: Vec<String> = choices(folder).into_iter().map(|c| c.path).collect();
    for path in &all {
        if let Some(recipe) = recipe_of(folder, path) {
            found.of_sound.insert(path.clone(), recipe.clone());
            if listed.contains(path) {
                found
                    .sounds_of
                    .entry(recipe)
                    .or_default()
                    .push(path.clone());
            }
        }
    }
    // A recipe that made nothing yet is a recipe still.
    if let Ok(entries) = std::fs::read_dir(folder.join("art")) {
        for entry in entries.flatten() {
            if is_recipe(&entry.path()) {
                let place = place_of(folder, &entry.path());
                found.sounds_of.entry(place).or_default();
            }
        }
    }
    found
}

/// Where Duck Forge Studio looks for Duck Echo Studio. `Places::here`
/// is this computer; tests make their own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    pub windows: bool,
    /// The folder Duck Forge Studio's own program is in.
    pub studio_folder: Option<PathBuf>,
    /// The folders on the PATH.
    pub path: Vec<PathBuf>,
    /// Windows: `%LOCALAPPDATA%` (where it installs for one person).
    pub local_app_data: Option<PathBuf>,
    /// Windows: `%ProgramFiles%` and `%ProgramFiles(x86)%`.
    pub program_files: Vec<PathBuf>,
    /// The person's home folder.
    pub home: Option<PathBuf>,
}

impl Places {
    /// This computer's places.
    pub fn here() -> Places {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Places {
            windows: cfg!(windows),
            studio_folder: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf)),
            path: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            local_app_data: var("LOCALAPPDATA"),
            program_files: ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(var)
                .collect(),
            home: var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }),
        }
    }

    /// Every place Duck Echo Studio could be, in the order they're tried:
    /// beside Duck Forge Studio, where its installer puts it (Windows) or
    /// where an AppImage is usually kept (Linux), then the PATH.
    pub fn duck_echo_candidates(&self) -> Vec<PathBuf> {
        let names: &[&str] = if self.windows {
            &["DuckEchoStudio.exe"]
        } else {
            &[
                "DuckEchoStudio.AppImage",
                "DuckEchoStudio",
                "duck-echo-studio",
            ]
        };
        let mut folders: Vec<PathBuf> = Vec::new();
        if let Some(here) = &self.studio_folder {
            folders.push(here.clone());
            folders.push(here.join("Duck Echo Studio"));
            if let Some(up) = here.parent() {
                folders.push(up.join("Duck Echo Studio"));
            }
        }
        if self.windows {
            if let Some(local) = &self.local_app_data {
                folders.push(local.join("Programs").join("Duck Echo Studio"));
            }
            for programs in &self.program_files {
                folders.push(programs.join("Duck Echo Studio"));
            }
        } else if let Some(home) = &self.home {
            for inside in ["Applications", ".local/bin", "bin", "Downloads", "Desktop"] {
                folders.push(home.join(inside));
            }
        }
        folders.extend(self.path.iter().cloned());
        let mut out = Vec::new();
        for folder in folders {
            for name in names {
                let candidate = folder.join(name);
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
        out
    }
}

/// Whether `path` is a program this computer can start: a file, and on
/// Linux one allowed to run.
fn runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Duck Echo Studio's program: the one chosen in the Studio (if it's
/// there), or the first found in `places`.
pub fn find_duck_echo_in(chosen: Option<&Path>, places: &Places) -> Option<PathBuf> {
    if let Some(chosen) = chosen.filter(|path| path.is_file()) {
        return Some(chosen.to_path_buf());
    }
    places
        .duck_echo_candidates()
        .into_iter()
        .find(|path| runnable(path))
}

/// Duck Echo Studio's program on this computer.
pub fn find_duck_echo(chosen: Option<&Path>) -> Option<PathBuf> {
    find_duck_echo_in(chosen, &Places::here())
}

/// What to say when Duck Echo Studio can't be found: plainly, with what
/// to do and its download page. An AppImage that's there but not allowed
/// to run yet is named.
pub fn no_duck_echo(places: &Places) -> Problem {
    let stuck = places
        .duck_echo_candidates()
        .into_iter()
        .find(|path| path.is_file());
    let next = match (&stuck, places.windows) {
        (Some(file), false) => format!(
            "{} is there, but isn't allowed to run yet: right-click it, Properties, \
             Permissions, \"Allow executing file as program\". Or choose another.",
            file.display()
        ),
        (_, true) => String::from(
            "Install it from its download page (it goes in your own Programs folder), or \
             choose DuckEchoStudio.exe if it's somewhere else.",
        ),
        (None, false) => String::from(
            "Download DuckEchoStudio.AppImage from its download page into the Applications \
             folder in your home folder, allow it to run (Properties, Permissions), or \
             choose where it is.",
        ),
    };
    Problem::new("Where is Duck Echo Studio?", next)
        .with_link("Get Duck Echo Studio", DUCK_ECHO_PAGE)
}

/// The command that opens `recipe` (as the game's folder names it,
/// `art/step.toml`) of the game in `game` in Duck Echo Studio:
/// `DuckEchoStudio --open <game>/art/step.toml`, run in the game's folder.
pub fn open_in_duck_echo(program: &Path, game: &Path, recipe: &str) -> CommandLine {
    let mut file = game.to_path_buf();
    for part in recipe.split('/').filter(|p| !p.is_empty()) {
        file.push(part);
    }
    let program_word = program.to_string_lossy().into_owned();
    let shown_program = if program_word.contains(' ') {
        format!("\"{program_word}\"")
    } else {
        program_word
    };
    CommandLine::new(program, &["--open", file.to_string_lossy().as_ref()], game)
        .shown_as(format!("{shown_program} --open {recipe}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_folder::TestFolder;

    fn duck_craft() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../examples/duck_craft")
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn the_sound_box_lists_music_and_sets_not_their_sounds() {
        let found = choices(&duck_craft());
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"assets/step (6 variants)"), "{labels:?}");
        assert!(
            labels.contains(&"assets/music_day.ogg (music)"),
            "{labels:?}"
        );
        assert!(labels.contains(&"assets/thud.wav"), "{labels:?}");
        assert!(
            !labels.iter().any(|l| l.contains("step_1")),
            "a set's sounds aren't listed one by one: {labels:?}"
        );
        let step = found
            .iter()
            .find(|c| c.label.starts_with("assets/step"))
            .unwrap();
        assert_eq!(step.path, "assets/step.variants.json");
        assert_eq!(step.kind, SoundKind::Set { variants: 6 });
        let day = found
            .iter()
            .find(|c| c.path == "assets/music_day.ogg")
            .unwrap();
        assert_eq!(day.kind, SoundKind::Music);
    }

    #[test]
    fn facts_lines_say_length_loudness_hit_loops_and_recipe() {
        let game = duck_craft();
        assert_eq!(
            facts(&game, "assets/thud.wav").unwrap().line(),
            "0.20 s  -16.0 LUFS  hit at 0.014 s  plays once  from art/thud.toml"
        );
        assert_eq!(
            facts(&game, "assets/step.variants.json").unwrap().line(),
            "6 variants  0.05 s  -18.9 to -18.2 LUFS  hit at 0.004 to 0.007 s  plays once  \
             from art/step.toml"
        );
        assert_eq!(
            facts(&game, "assets/music_day.ogg").unwrap().line(),
            "22.86 s  -18.0 LUFS  hit at 5.725 s  loops  from art/music_day.toml"
        );
        assert_eq!(facts(&game, "assets/hum.wav"), None, "no facts file");
    }

    #[test]
    fn recipes_are_found_by_their_facts_or_their_name() {
        let game = duck_craft();
        assert_eq!(
            recipe_of(&game, "assets/thud.wav").as_deref(),
            Some("art/thud.toml")
        );
        assert_eq!(
            recipe_of(&game, "assets/step.variants.json").as_deref(),
            Some("art/step.toml")
        );
        assert_eq!(
            recipe_of(&game, "art/step.toml").as_deref(),
            Some("art/step.toml")
        );
        assert_eq!(recipe_of(&game, "assets/hum.wav"), None);
        assert!(is_recipe(&game.join("art/music_night.toml")), "a song");
        assert!(!is_recipe(&game.join("art/blocks.json")));
        let all = recipes(&game);
        assert_eq!(all.of_sound["assets/step_3.wav"], "art/step.toml");
        assert_eq!(
            all.sounds_of["art/step.toml"],
            vec!["assets/step.variants.json"]
        );
        assert_eq!(all.sounds_of["art/thud.toml"], vec!["assets/thud.wav"]);
    }

    #[test]
    fn a_toml_is_a_recipe_only_with_a_version_and_a_sound_or_song() {
        let folder = TestFolder::new("sounds-recipes");
        let write = |name: &str, text: &str| {
            std::fs::write(folder.join(name), text).unwrap();
            folder.join(name)
        };
        assert!(is_recipe(&write(
            "a.toml",
            "version = 0\nname = \"a\"\n[[layers]]\nkind = \"noise\"\n"
        )));
        assert!(is_recipe(&write(
            "b.toml",
            "version = 0\ntempo = 90\n[[tracks]]\n"
        )));
        assert!(!is_recipe(&write(
            "c.toml",
            "[package]\nname = \"game\"\nversion = \"0.1.0\"\n"
        )));
        assert!(!is_recipe(&write("d.toml", "version = 0\nname = \"x\"\n")));
        assert!(!is_recipe(&write("e.txt", "version = 0\ntempo = 90\n")));
    }

    #[test]
    fn duck_echo_is_looked_for_where_it_installs() {
        let windows = Places {
            windows: true,
            studio_folder: Some(PathBuf::from("C:/DF")),
            local_app_data: Some(PathBuf::from("C:/Users/ann/AppData/Local")),
            program_files: vec![PathBuf::from("C:/Program Files")],
            path: vec![PathBuf::from("C:/tools")],
            home: None,
        };
        let tried = windows.duck_echo_candidates();
        let want = [
            Path::new("C:/DF").join("DuckEchoStudio.exe"),
            Path::new("C:/Users/ann/AppData/Local")
                .join("Programs")
                .join("Duck Echo Studio")
                .join("DuckEchoStudio.exe"),
            Path::new("C:/Program Files")
                .join("Duck Echo Studio")
                .join("DuckEchoStudio.exe"),
            Path::new("C:/tools").join("DuckEchoStudio.exe"),
        ];
        for w in &want {
            assert!(tried.contains(w), "{w:?} in {tried:?}");
        }
        let linux = Places {
            windows: false,
            home: Some(PathBuf::from("/home/ann")),
            path: vec![PathBuf::from("/usr/local/bin")],
            ..Places::default()
        };
        let tried = linux.duck_echo_candidates();
        assert_eq!(
            tried[0],
            Path::new("/home/ann/Applications/DuckEchoStudio.AppImage")
        );
        assert!(tried.contains(&PathBuf::from("/usr/local/bin/duck-echo-studio")));
        assert!(tried.contains(&PathBuf::from("/usr/local/bin/DuckEchoStudio")));
    }

    #[test]
    fn it_is_found_on_the_path_or_beside_and_a_chosen_one_comes_first() {
        let folder = TestFolder::new("sounds-find");
        let bin = folder.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = if cfg!(windows) {
            "DuckEchoStudio.exe"
        } else {
            "duck-echo-studio"
        };
        let program = bin.join(name);
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let places = Places {
            windows: cfg!(windows),
            path: vec![bin.clone()],
            ..Places::default()
        };
        assert_eq!(find_duck_echo_in(None, &places), Some(program.clone()));
        let chosen = folder.join("mine");
        std::fs::write(&chosen, "").unwrap();
        assert_eq!(find_duck_echo_in(Some(&chosen), &places), Some(chosen));
        // A chosen program that has gone: the one found instead.
        assert_eq!(
            find_duck_echo_in(Some(&folder.join("gone")), &places),
            Some(program)
        );
        assert_eq!(find_duck_echo_in(None, &Places::default()), None);
    }

    #[test]
    fn a_missing_duck_echo_says_what_to_do_plainly() {
        let windows = no_duck_echo(&Places {
            windows: true,
            ..Places::default()
        });
        assert_eq!(windows.what, "Where is Duck Echo Studio?");
        assert!(windows.next.contains("download page"), "{}", windows.next);
        assert!(
            windows.next.contains("DuckEchoStudio.exe"),
            "{}",
            windows.next
        );
        let link = windows.link.clone().unwrap();
        assert_eq!(link.1, DUCK_ECHO_PAGE);
        let linux = no_duck_echo(&Places::default());
        assert!(
            linux.next.contains("DuckEchoStudio.AppImage"),
            "{}",
            linux.next
        );
        assert_eq!(linux.link.unwrap().1, DUCK_ECHO_PAGE);
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_not_allowed_to_run_is_named() {
        let folder = TestFolder::new("sounds-appimage");
        let apps = folder.join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("DuckEchoStudio.AppImage"), "").unwrap();
        let places = Places {
            home: Some(folder.to_path_buf()),
            ..Places::default()
        };
        assert_eq!(find_duck_echo_in(None, &places), None);
        let said = no_duck_echo(&places);
        assert!(
            said.next.contains("isn't allowed to run yet"),
            "{}",
            said.next
        );
        assert!(
            said.next.contains("DuckEchoStudio.AppImage"),
            "{}",
            said.next
        );
    }

    #[test]
    fn the_command_line_is_exactly_right() {
        // On this computer: the program, then --open and the recipe's
        // whole path, run in the game's folder.
        #[cfg(windows)]
        let (program, game, file, shown) = (
            PathBuf::from(
                r"C:\Users\ann\AppData\Local\Programs\Duck Echo Studio\DuckEchoStudio.exe",
            ),
            PathBuf::from(r"C:\Users\ann\Documents\Duck Forge\examples\duck_craft"),
            r"C:\Users\ann\Documents\Duck Forge\examples\duck_craft\art\step.toml",
            r#""C:\Users\ann\AppData\Local\Programs\Duck Echo Studio\DuckEchoStudio.exe" --open art/step.toml"#,
        );
        #[cfg(not(windows))]
        let (program, game, file, shown) = (
            PathBuf::from("/home/ann/Applications/DuckEchoStudio.AppImage"),
            PathBuf::from("/home/ann/Duck Forge/examples/duck_craft"),
            "/home/ann/Duck Forge/examples/duck_craft/art/step.toml",
            "/home/ann/Applications/DuckEchoStudio.AppImage --open art/step.toml",
        );
        let line = open_in_duck_echo(&program, &game, "art/step.toml");
        assert_eq!(line.program, program.as_os_str());
        assert_eq!(
            line.args,
            vec![std::ffi::OsString::from("--open"), file.into()]
        );
        assert_eq!(line.folder, game);
        assert_eq!(line.shown, shown);
    }
}

//! A game's scripts, as the Studio sees them: `scripts.rs` (the list the
//! Studio keeps beside `main.rs`), the name a new script gets, its
//! skeleton, and what the running game says each script's settings are.
//!
//! `scripts.rs` is written whole, always the same way, like a scene file:
//!
//! ```text
//! #[path = "scripts/bee.rs"]
//! mod bee;
//!
//! pub fn all() -> Scripts {
//!     Scripts::new()
//!         .add::<bee::Bee>("bee")
//! }
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::problem::Problem;

/// The file beside `main.rs` that lists the game's scripts.
pub const SCRIPTS_FILE: &str = "scripts.rs";

/// One script in `scripts.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptEntry {
    /// Its name in scenes: an object's `"kind"`.
    pub name: String,
    /// Its file, from the game's folder: `scripts/bee.rs`.
    pub path: String,
    /// The module `scripts.rs` gives it: `bee`.
    pub module: String,
    /// The type in its file: `Bee`.
    pub type_name: String,
}

/// The game's `scripts.rs`, read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptsFile {
    pub entries: Vec<ScriptEntry>,
}

impl ScriptsFile {
    /// Reads the game's `scripts.rs`. `None` if it hasn't one.
    pub fn read(game_folder: &Path) -> Option<ScriptsFile> {
        let text = std::fs::read_to_string(game_folder.join(SCRIPTS_FILE)).ok()?;
        Some(ScriptsFile::parse(&text))
    }

    /// The scripts a `scripts.rs` lists. Lines it doesn't know are passed
    /// over, so one written by hand in the same shape reads too.
    pub fn parse(text: &str) -> ScriptsFile {
        let mut paths: Vec<(String, String)> = Vec::new(); // (module, path)
        let mut waiting_path: Option<String> = None;
        let mut entries = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(path) = line
                .strip_prefix("#[path = \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
            {
                waiting_path = Some(path.replace('\\', "/"));
            } else if let Some(module) = line
                .strip_prefix("mod ")
                .or_else(|| line.strip_prefix("pub mod "))
                .and_then(|rest| rest.strip_suffix(';'))
            {
                let path = waiting_path
                    .take()
                    .unwrap_or_else(|| format!("{module}.rs"));
                paths.push((module.trim().to_string(), path));
            }
        }
        // Each `.add::<bee::Bee>("bee")`, wherever it is (one a line, or
        // several on one).
        for (at, _) in text.match_indices(".add::<") {
            let rest = &text[at + ".add::<".len()..];
            let Some((path, rest)) = rest.split_once(">(\"") else {
                continue;
            };
            let Some(name) = rest.split('"').next() else {
                continue;
            };
            let Some((module, type_name)) = path.trim().rsplit_once("::") else {
                continue;
            };
            let file = paths
                .iter()
                .find(|(m, _)| m == module)
                .map_or_else(|| format!("{module}.rs"), |(_, path)| path.clone());
            entries.push(ScriptEntry {
                name: name.to_string(),
                path: file,
                module: module.to_string(),
                type_name: type_name.to_string(),
            });
        }
        ScriptsFile { entries }
    }

    /// The whole file, as the Studio writes it.
    pub fn text(&self) -> String {
        let mut text = String::from(
            "// Duck Forge Studio keeps this file: it lists this game's scripts.\n\
             // New script, Rename and Delete in Files change it. Writing it by hand\n\
             // is fine too, one line each in the same shape.\n\
             \n\
             use duckforge::prelude::Scripts;\n\
             \n",
        );
        for entry in &self.entries {
            text.push_str(&format!(
                "#[path = \"{}\"]\nmod {};\n\n",
                entry.path, entry.module
            ));
        }
        text.push_str("#[rustfmt::skip]\npub fn all() -> Scripts {\n    Scripts::new()");
        for entry in &self.entries {
            text.push_str(&format!(
                "\n        .add::<{}::{}>(\"{}\")",
                entry.module, entry.type_name, entry.name
            ));
        }
        text.push_str("\n}\n");
        text
    }

    /// The script called `name`.
    pub fn get(&self, name: &str) -> Option<&ScriptEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// The script whose file is `path` (from the game's folder).
    pub fn by_path(&self, path: &str) -> Option<&ScriptEntry> {
        let path = path.replace('\\', "/");
        self.entries.iter().find(|entry| entry.path == path)
    }
}

/// Kinds Duck Forge already has: a script can't take their names.
pub const PLAIN_KINDS: [&str; 7] = [
    "picture",
    "tilemap",
    "background",
    "sound",
    "area",
    "text",
    "button",
];

/// Words Rust keeps for itself.
const RUST_WORDS: [&str; 51] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv",
    "typeof", "unsized", "virtual", "yield", "try", "gen",
];

/// Names Duck Forge already gives things a script's code can use: a
/// script with one of these as its type would hide that thing.
const ENGINE_NAMES: [&str; 62] = [
    "Anchor",
    "Animation",
    "Background",
    "Billboard",
    "Blit",
    "BlockId",
    "BlockKind",
    "BlockPalette",
    "BlockWorld",
    "Camera2D",
    "Camera3D",
    "Canvas",
    "Color",
    "Config",
    "Contacts",
    "Context",
    "Corner",
    "FaceLight",
    "FailedCheck",
    "FeelPreset",
    "Fog",
    "Game",
    "Hit",
    "Hop",
    "Id",
    "Input",
    "InputScript",
    "Jump",
    "Key",
    "Legend",
    "Letter",
    "LetterKind",
    "Lighting",
    "LoopId",
    "Mesh",
    "MouseButton",
    "Object",
    "Placed",
    "PlayTest",
    "Reach",
    "Rect",
    "Renderer",
    "ReportValue",
    "Rng",
    "Scene",
    "SceneGame",
    "Script",
    "Scripts",
    "Settings",
    "Simulation",
    "Sound",
    "SpawnChanges",
    "Sprite",
    "SpriteSheet",
    "TileKind",
    "TileMap",
    "Tileset",
    "Timestep",
    "Transform",
    "Vec2",
    "Vec3",
    "Wav",
];

/// A new script's name, from what the person typed ("Big bee"): its name
/// in scenes and its file's name (`big_bee`), and its type's (`BigBee`).
/// Says plainly why a name can't be used.
pub fn new_name(typed: &str) -> Result<(String, String), Problem> {
    let typed = typed.trim().trim_end_matches(".rs");
    let mut words: Vec<String> = Vec::new();
    for word in typed.split(|c: char| c.is_whitespace() || c == '_' || c == '-') {
        if word.is_empty() {
            continue;
        }
        if !word.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(Problem::new(
                format!("A script's name can't have {:?} in it.", strange(word)),
                "Use letters and numbers, with spaces between words, like \"big bee\".",
            ));
        }
        // "BigBee" is two words too.
        let mut current = String::new();
        let chars: Vec<char> = word.chars().collect();
        for (index, c) in chars.iter().enumerate() {
            let starts_word = c.is_ascii_uppercase()
                && index > 0
                && (chars[index - 1].is_ascii_lowercase()
                    || chars.get(index + 1).is_some_and(char::is_ascii_lowercase));
            if starts_word && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            current.push(c.to_ascii_lowercase());
        }
        if !current.is_empty() {
            words.push(current);
        }
    }
    if words.is_empty() {
        return Err(Problem::new(
            "A script needs a name.",
            "Type one, like \"bee\" or \"moving platform\".",
        ));
    }
    if words[0].starts_with(|c: char| c.is_ascii_digit()) {
        return Err(Problem::new(
            "A script's name can't start with a number.",
            "Put a word first, like \"bee 2\".",
        ));
    }
    let name = words.join("_");
    let type_name: String = words
        .iter()
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or(String::new(), |first| {
                first.to_ascii_uppercase().to_string() + letters.as_str()
            })
        })
        .collect();
    if RUST_WORDS.contains(&name.as_str()) {
        return Err(Problem::new(
            format!(
                "\"{name}\" is a word Rust keeps for itself, so a script can't be called that."
            ),
            format!("Try another, like \"{name} script\"."),
        ));
    }
    if PLAIN_KINDS.contains(&name.as_str()) {
        return Err(Problem::new(
            format!("Duck Forge already has a kind called \"{name}\"."),
            format!("Call the script something else, like \"my {name}\"."),
        ));
    }
    if ENGINE_NAMES.contains(&type_name.as_str()) {
        return Err(Problem::new(
            format!(
                "Duck Forge already has something called {type_name}, which a script's code \
                 uses, so a script can't be called \"{name}\"."
            ),
            format!("Call it something else, like \"my {name}\"."),
        ));
    }
    Ok((name, type_name))
}

/// The first letter of a word a name can't have.
fn strange(word: &str) -> char {
    word.chars()
        .find(|c| !c.is_ascii_alphanumeric())
        .unwrap_or('?')
}

/// A new script's file, for someone who has never seen Rust.
pub fn skeleton(name: &str, type_name: &str) -> String {
    format!(
        r#"//! {name}: a script. Drag it from Files onto an object to give that object
//! this script. Its settings are the boxes the Inspector shows for it.

use duckforge::prelude::*;

#[derive(Default)]
pub struct {type_name} {{
    // Each setting is a box in the Inspector. Add your own the same way.
    speed: f32,
    // Things it remembers while the game runs go here too, without a box.
}}

impl Script for {type_name} {{
    /// The settings: a name, where it goes, and its value at the start.
    fn settings(&mut self, s: &mut Settings) {{
        s.number("speed", &mut self.speed, 0.0).help("Pixels a second, to the right");
    }}

    /// Sixty times a second, while the game plays.
    fn update(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {{
        // An example to change or remove: it moves right at `speed`.
        // Give speed a value in the Inspector and press Tab to see it.
        scene[me].at.x += self.speed * ctx.dt();
    }}

    // It can also have a start, which runs once, before the first update:
    //
    //     fn start(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {{
    //     }}
    //
    // docs/AI_GUIDE.md, section 14, has everything a script can do.
}}
"#
    )
}

/// What connecting `main.rs` to `scripts.rs` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connected {
    /// It was connected already.
    Already,
    /// The lines were added.
    Added,
    /// `mod scripts;` was added (if it wasn't there), but where the scene
    /// is loaded couldn't be told: the person adds this, at this line.
    Tell { line: usize },
}

/// Connects a game's `main.rs` to its `scripts.rs`: `mod scripts;` once,
/// and `.with_scripts(scripts::all())` where the scene is made, if it can
/// be told where (one `SceneGame::new(...)`, or one `Scene::load(...)`).
/// Gives the new text.
pub fn connect_main(main: &str) -> (String, Connected) {
    let mut text = main.to_string();
    let mut changed = false;
    if !text.lines().any(|line| line.trim() == "mod scripts;") {
        text = add_mod_line(&text);
        changed = true;
    }
    if text.contains("scripts::all()") {
        return (
            text,
            if changed {
                Connected::Added
            } else {
                Connected::Already
            },
        );
    }
    for call in ["SceneGame::new(", "Scene::load("] {
        let found: Vec<usize> = text.match_indices(call).map(|(at, _)| at).collect();
        if found.len() != 1 {
            continue;
        }
        let open = found[0] + call.len() - 1;
        let Some(close) = matching_bracket(&text, open) else {
            continue;
        };
        let mut end = close + 1;
        // Past what makes the scene out of a Result.
        let rest = &text[end..];
        if rest.starts_with('?') {
            end += 1;
        } else if let Some(after) = [".unwrap()", ".expect("]
            .iter()
            .find(|word| rest.starts_with(**word))
        {
            if *after == ".unwrap()" {
                end += after.len();
            } else {
                let Some(close) = matching_bracket(&text, end + after.len() - 1) else {
                    continue;
                };
                end = close + 1;
            }
        }
        text.insert_str(end, ".with_scripts(scripts::all())");
        return (text, Connected::Added);
    }
    let line = text
        .lines()
        .position(|line| line.contains("Scene::load(") || line.contains("SceneGame::new("))
        .map_or(1, |index| index + 1);
    (text, Connected::Tell { line })
}

/// `mod scripts;`, after the last `use` line at the top of the file (or
/// after its first comments).
fn add_mod_line(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut after = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if line.starts_with("use ") && line.trim_end().ends_with(';') {
            after = Some(index);
        } else if line.starts_with("fn ")
            || line.starts_with("struct ")
            || line.starts_with("impl ")
            || line.starts_with("const ")
            || line.starts_with("pub ")
        {
            break;
        } else if after.is_none() && (trimmed.starts_with("//") || trimmed.is_empty()) {
            continue;
        }
    }
    let at = after.map_or_else(
        || {
            lines
                .iter()
                .position(|line| !line.trim_start().starts_with("//") && !line.trim().is_empty())
                .unwrap_or(lines.len())
        },
        |index| index + 1,
    );
    let mut out: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
    out.insert(
        at,
        String::from(
            "\n// The game's scripts: code that belongs to one object. Duck Forge Studio\n\
             // keeps their list in scripts.rs.\n\
             mod scripts;",
        ),
    );
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// The `)` that closes the `(` at `open`.
fn matching_bracket(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

// ---- What the running game says ----

/// One setting of a script, as the game says it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SettingInfo {
    pub name: String,
    /// `number`, `whole`, `flag`, `words`, `choice`, `colour`, `picture`
    /// or `sound`.
    pub kind: String,
    pub default: serde_json::Value,
    #[serde(default)]
    pub range: Option<[f32; 2]>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub expert: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}

impl SettingInfo {
    /// What the Inspector calls it.
    pub fn title(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| self.name.replace('_', " "))
    }
}

/// One script, as the game says it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScriptInfo {
    pub name: String,
    #[serde(rename = "type", default)]
    pub type_name: String,
    #[serde(default)]
    pub settings: Vec<SettingInfo>,
}

/// The game's answer to "scripts".
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub struct GameScripts {
    /// Whether its scenes were given scripts at all.
    pub given: bool,
    pub scripts: Vec<ScriptInfo>,
}

impl GameScripts {
    pub fn get(&self, name: &str) -> Option<&ScriptInfo> {
        self.scripts.iter().find(|script| script.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_a_file_and_a_type() {
        assert_eq!(
            new_name("bee").unwrap(),
            ("bee".to_string(), "Bee".to_string())
        );
        assert_eq!(
            new_name("  Big bee ").unwrap(),
            ("big_bee".to_string(), "BigBee".to_string())
        );
        assert_eq!(
            new_name("MovingPlatform").unwrap(),
            ("moving_platform".to_string(), "MovingPlatform".to_string())
        );
        assert_eq!(new_name("bee 2").unwrap().1, "Bee2");
        assert_eq!(new_name("door.rs").unwrap().0, "door");
        for (typed, says) in [
            ("", "needs a name"),
            ("2 bees", "can't start with a number"),
            ("bee!", "can't have '!'"),
            ("move", "Rust keeps for itself"),
            ("sound", "already has a kind called \"sound\""),
            ("scene", "already has something called Scene"),
            ("key", "already has something called Key"),
        ] {
            let problem = new_name(typed).unwrap_err();
            assert!(problem.what.contains(says), "{typed}: {}", problem.what);
        }
    }

    #[test]
    fn scripts_rs_is_read_and_written_the_same_way() {
        let file = ScriptsFile {
            entries: vec![
                ScriptEntry {
                    name: "bee".into(),
                    path: "scripts/bee.rs".into(),
                    module: "bee".into(),
                    type_name: "Bee".into(),
                },
                ScriptEntry {
                    name: "big_bee".into(),
                    path: "big_bee.rs".into(),
                    module: "big_bee".into(),
                    type_name: "BigBee".into(),
                },
            ],
        };
        let text = file.text();
        assert!(text.contains(
            "#[path = \"scripts/bee.rs\"]\nmod bee;\n\n#[path = \"big_bee.rs\"]\nmod big_bee;"
        ));
        assert!(text.contains(
            ".add::<bee::Bee>(\"bee\")\n        .add::<big_bee::BigBee>(\"big_bee\")\n}"
        ));
        assert_eq!(ScriptsFile::parse(&text), file);
        assert_eq!(ScriptsFile::parse(&text.replace('\n', "\r\n")), file);
        assert_eq!(
            ScriptsFile::parse(&ScriptsFile::default().text()),
            ScriptsFile::default()
        );
        // As rustfmt would put it, on one line.
        let one_line = text
            .replace("Scripts::new()\n        .add", "Scripts::new().add")
            .replace(")\n        .add", ").add");
        assert_eq!(ScriptsFile::parse(&one_line), file);
        assert!(text.contains("#[rustfmt::skip]\npub fn all()"));
        assert_eq!(
            file.by_path("scripts\\bee.rs").map(|e| e.name.as_str()),
            Some("bee")
        );
    }

    #[test]
    fn main_rs_is_connected_where_the_scene_is_made() {
        let scene_game = "use duckforge::prelude::*;\n\nfn main() {\n    SceneGame::new(\"examples/bee_maze\").run();\n}\n";
        let (text, done) = connect_main(scene_game);
        assert_eq!(done, Connected::Added);
        assert!(
            text.contains("use duckforge::prelude::*;\n\n// The game's scripts"),
            "{text}"
        );
        assert!(text.contains("mod scripts;\n\nfn main"), "{text}");
        assert!(
            text.contains(
                "SceneGame::new(\"examples/bee_maze\").with_scripts(scripts::all()).run();"
            )
        );
        assert_eq!(connect_main(&text), (text.clone(), Connected::Already));

        let loads = "//! A game.\n\nuse std::path::Path;\nuse duckforge::prelude::*;\n\nstruct G { scene: Scene }\nfn new() -> G {\n    G { scene: Scene::load(Path::new(\"a.json\")).expect(\"it's there\") }\n}\n";
        let (text, done) = connect_main(loads);
        assert_eq!(done, Connected::Added);
        assert!(text.contains(
            "Scene::load(Path::new(\"a.json\")).expect(\"it's there\").with_scripts(scripts::all()) }"
        ), "{text}");
        let (text, _) = connect_main("use x;\nfn f() -> R { Ok(Scene::load(p)?) }\n");
        assert!(
            text.contains("Scene::load(p)?.with_scripts(scripts::all())"),
            "{text}"
        );

        // Two scenes loaded: it can't tell which, so it says where.
        let two = "use x;\n\nfn a() { Scene::load(\"a\"); }\nfn b() { Scene::load(\"b\"); }\n";
        let (text, done) = connect_main(two);
        assert_eq!(done, Connected::Tell { line: 7 });
        assert!(text.contains("mod scripts;"));
    }

    #[test]
    fn the_skeleton_names_its_type() {
        let text = skeleton("big_bee", "BigBee");
        assert!(text.starts_with("//! big_bee: a script."));
        assert!(text.contains("pub struct BigBee {"));
        assert!(text.contains("impl Script for BigBee {"));
        assert!(text.contains("s.number(\"speed\", &mut self.speed, 0.0)"));
    }

    #[test]
    fn the_games_answer_reads() {
        let answer = r#"{"given": true, "scripts": [{"name": "bob", "type": "Bob", "settings": [
            {"name": "how_far", "kind": "number", "default": 3.0, "range": [0.0, 32.0], "label": "Bobs up and down by"}]}]}"#;
        let scripts: GameScripts = serde_json::from_str(answer).unwrap();
        assert!(scripts.given);
        let bob = scripts.get("bob").unwrap();
        assert_eq!(bob.type_name, "Bob");
        assert_eq!(bob.settings[0].range, Some([0.0, 32.0]));
        assert_eq!(bob.settings[0].title(), "Bobs up and down by");
    }
}

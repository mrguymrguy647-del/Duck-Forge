//! A scene file: reading it, saying what's wrong with it, and writing it
//! the way Duck Forge always writes it (one object per line).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use serde_json::{Map, Value};

use super::object::{
    COMMON_KEYS, FIRST_MADE_ID, Id, Object, colour_of, colour_word, pair, point_of,
};
use crate::color::Color;
use crate::error::LoadError;
use crate::math::Vec2;

/// The newest kind of scene file this engine reads (`"duckforge_scene"`).
pub const SCENE_FILE_VERSION: u64 = 1;

/// One of a scene's layers. Layers are drawn in the scene's order; inside
/// a layer, objects are drawn in the file's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    /// It doesn't move with the camera: scores, lives, buttons.
    pub stays_on_screen: bool,
}

/// The layers a scene has when its file names none.
pub(crate) fn usual_layers() -> Vec<Layer> {
    ["far", "back", "world", "front", "hud"]
        .into_iter()
        .map(|name| Layer {
            name: name.to_string(),
            stays_on_screen: name == "hud",
        })
        .collect()
}

/// A flat scene, or one in 3D. (3D scenes are read and kept, and drawn
/// from Phase 5b.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Space {
    #[default]
    Flat,
    ThreeD,
}

/// Something wrong with a scene file, in plain words, with its line if it
/// has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub line: Option<usize>,
    pub text: String,
}

impl Problem {
    /// As one sentence about `file`: `scenes/level1.json, line 9: ...`.
    pub fn in_file(&self, file: &str) -> String {
        match self.line {
            Some(line) => format!("{file}, line {line}: {}", self.text),
            None => format!("{file}: {}", self.text),
        }
    }
}

/// A scene as its file has it: the objects and what's around them, before
/// anything is loaded or played. [`Scene`](super::Scene) plays one; tools
/// read, change and write one.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneFile {
    pub space: Space,
    /// The scene's size in pixels, if it says.
    pub size: Option<Vec2>,
    /// The colour behind everything, if it says.
    pub background: Option<Color>,
    pub layers: Vec<Layer>,
    /// The scene that comes after it.
    pub next: Option<String>,
    /// Its objects, in the file's order (the drawing order). Every one has
    /// an id here, given or not.
    pub objects: Vec<Object>,
    /// Its rules, one sentence each (read from step 10).
    pub rules: Vec<String>,
    /// Top-level keys the engine doesn't know, kept as they are.
    other: BTreeMap<String, Value>,
    /// How many objects had no id of their own in the text.
    pub ids_missing: usize,
    /// What's wrong with it. The scene is still read: each problem's key
    /// or object is as if it weren't there.
    pub problems: Vec<Problem>,
}

impl SceneFile {
    /// Reads a scene's text. `Err` only when it can't be read as a scene
    /// at all (it isn't JSON, or it's from a newer Duck Forge); everything
    /// smaller is in [`problems`](SceneFile::problems).
    pub fn parse(text: &str) -> Result<SceneFile, Problem> {
        let value: Value = serde_json::from_str(text).map_err(|error| Problem {
            line: Some(error.line()),
            text: format!(
                "this isn't JSON a scene can be read from ({})",
                json_words(&error)
            ),
        })?;
        let Value::Object(top) = value else {
            return Err(Problem {
                line: Some(1),
                text: String::from(
                    "a scene file is one { ... } with \"objects\": [ ... ] inside it",
                ),
            });
        };
        let mut problems = Vec::new();
        let mut say = |line: Option<usize>, text: String| problems.push(Problem { line, text });
        match top.get("duckforge_scene").map(Value::as_u64) {
            Some(Some(version)) if version > SCENE_FILE_VERSION => {
                return Err(Problem {
                    line: None,
                    text: format!(
                        "this scene was made by a newer Duck Forge (it says \
                         \"duckforge_scene\": {version}, and this one reads up to \
                         {SCENE_FILE_VERSION}); get the newest Duck Forge to open it"
                    ),
                });
            }
            Some(Some(1..)) => {}
            _ => say(
                None,
                format!(
                    "it should start with \"duckforge_scene\": {SCENE_FILE_VERSION}; \
                     it's read as a scene anyway"
                ),
            ),
        }

        let mut scene = SceneFile {
            space: Space::Flat,
            size: None,
            background: None,
            layers: usual_layers(),
            next: None,
            objects: Vec::new(),
            rules: Vec::new(),
            other: BTreeMap::new(),
            ids_missing: 0,
            problems: Vec::new(),
        };
        for (key, value) in &top {
            match key.as_str() {
                "duckforge_scene" | "objects" => {}
                "space" => match value.as_str() {
                    Some("2d") => scene.space = Space::Flat,
                    Some("3d") => {
                        scene.space = Space::ThreeD;
                        say(
                            None,
                            String::from(
                                "3D scenes are read and kept, but can't be drawn until Phase 5b",
                            ),
                        );
                    }
                    _ => say(
                        None,
                        format!("\"space\" should be \"2d\" or \"3d\"; it's {value}"),
                    ),
                },
                "size" => match point_of(value) {
                    Some(size) if size.x > 0.0 && size.y > 0.0 => scene.size = Some(size),
                    _ => say(
                        None,
                        format!(
                            "\"size\" should be the scene's width and height in pixels, like [1280, 240]; it's {value}"
                        ),
                    ),
                },
                "background" => match value.as_str().and_then(colour_of) {
                    Some(colour) => scene.background = Some(colour),
                    None => say(
                        None,
                        format!("\"background\" should be a colour like \"#6ab4f0\"; it's {value}"),
                    ),
                },
                "layers" => match layers_of(value) {
                    Some(layers) if !layers.is_empty() => scene.layers = layers,
                    _ => say(
                        None,
                        format!(
                            "\"layers\" should be a list of names, like [\"back\", \"world\", \
                             {{\"name\": \"hud\", \"stays_on_screen\": true}}]; it's {value}"
                        ),
                    ),
                },
                "next" => match value.as_str() {
                    Some(next) => scene.next = Some(next.to_string()),
                    None => say(
                        None,
                        format!("\"next\" should be a scene's name in quotes; it's {value}"),
                    ),
                },
                "rules" => match value.as_array().map(|rules| {
                    rules
                        .iter()
                        .map(|rule| rule.as_str().map(str::to_string))
                        .collect::<Option<Vec<String>>>()
                }) {
                    Some(Some(rules)) => scene.rules = rules,
                    _ => say(
                        None,
                        String::from("\"rules\" should be a list of sentences in quotes"),
                    ),
                },
                _ => {
                    scene.other.insert(key.clone(), value.clone());
                }
            }
        }

        // The objects: each on its line, if the file is written that way.
        let lines = object_lines(text);
        let line_of = |index: usize| lines.get(index).copied();
        let empty = Vec::new();
        let listed = match top.get("objects") {
            None => {
                say(
                    None,
                    String::from("it has no \"objects\": [ ... ], so the scene is empty"),
                );
                &empty
            }
            Some(Value::Array(listed)) => listed,
            Some(_) => {
                say(
                    None,
                    String::from("\"objects\" should be a list: [ {...}, {...} ]"),
                );
                &empty
            }
        };
        let mut ids_seen = BTreeSet::new();
        for (index, value) in listed.iter().enumerate() {
            let line = line_of(index);
            let Value::Object(map) = value else {
                say(
                    line,
                    format!("an object is written {{ ... }}; this is {value}, so it's left out"),
                );
                continue;
            };
            let mut wrong = Vec::new();
            let mut object = if scene.space == Space::Flat {
                Object::from_json(map, &mut wrong)
            } else {
                // In 3D a place, a turn and a scale have three numbers:
                // they're kept as they are until Phase 5b reads them.
                let mut flat = map.clone();
                let kept: Vec<(String, Value)> = ["at", "turn", "scale"]
                    .into_iter()
                    .filter_map(|key| Some((key.to_string(), flat.remove(key)?)))
                    .collect();
                let mut object = Object::from_json(&flat, &mut wrong);
                object.settings.extend(kept);
                object
            };
            let called = object.name.clone();
            for text in wrong {
                say(line, format!("{called:?}: {text}"));
            }
            if object.id.0 != 0 && !ids_seen.insert(object.id.0) {
                say(
                    line,
                    format!(
                        "{called:?} has the id {}, which an object above it has too; \
                         it's given a new one",
                        object.id
                    ),
                );
                object.id = Id(0);
            }
            if !scene.layers.iter().any(|layer| layer.name == object.layer) {
                say(
                    line,
                    format!(
                        "{called:?} is on the layer {:?}, which the scene doesn't have \
                         (it has {}); it's drawn on \"world\"",
                        object.layer,
                        scene
                            .layers
                            .iter()
                            .map(|layer| layer.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
            scene.objects.push(object);
        }
        // Objects without an id get one, the same way every time: the
        // numbers after the highest id in the file, going down the file.
        let mut next = ids_seen.last().copied().unwrap_or(0) + 1;
        for object in &mut scene.objects {
            if object.id.0 == 0 {
                object.id = Id(next.min(FIRST_MADE_ID - 1));
                next += 1;
                scene.ids_missing += 1;
            }
        }
        scene.problems = problems;
        Ok(scene)
    }

    /// Reads a scene file from disk (not noted as a file the game reads:
    /// that's [`Scene::load`](super::Scene::load)).
    pub fn load(path: impl AsRef<Path>) -> Result<SceneFile, LoadError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "scene", &error))?;
        SceneFile::parse(&text).map_err(|problem| cannot_read(path, &problem))
    }

    /// Writes the scene to a file, as [`to_text`](SceneFile::to_text) has
    /// it.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::write(path, self.to_text())
    }

    /// The scene's text as Duck Forge always writes it: one object per
    /// line, keys in the same order, and only what differs from the usual.
    /// So a change to one object is a change to one line.
    pub fn to_text(&self) -> String {
        let mut text = format!("{{\n  \"duckforge_scene\": {SCENE_FILE_VERSION}");
        let mut key = |name: &str, value: String| {
            text.push_str(&format!(",\n  \"{name}\": {value}"));
        };
        if self.space == Space::ThreeD {
            key("space", String::from("\"3d\""));
        }
        if let Some(size) = self.size {
            key("size", compact(&pair(size)));
        }
        if let Some(background) = self.background {
            key("background", format!("\"{}\"", colour_word(background)));
        }
        if self.layers != usual_layers() {
            let layers: Vec<Value> = self
                .layers
                .iter()
                .map(|layer| {
                    if layer.stays_on_screen {
                        let mut map = Map::new();
                        map.insert(String::from("name"), Value::from(layer.name.clone()));
                        map.insert(String::from("stays_on_screen"), Value::from(true));
                        Value::Object(map)
                    } else {
                        Value::from(layer.name.clone())
                    }
                })
                .collect();
            key("layers", compact_in_order(&Value::Array(layers), &["name"]));
        }
        if let Some(next) = &self.next {
            key("next", compact(&Value::from(next.clone())));
        }
        for (name, value) in &self.other {
            key(name, compact(value));
        }
        let lines = |items: Vec<String>| {
            if items.is_empty() {
                String::from("[]")
            } else {
                format!("[\n    {}\n  ]", items.join(",\n    "))
            }
        };
        key(
            "objects",
            lines(
                self.objects
                    .iter()
                    .map(|object| {
                        compact_in_order(&Value::Object(object.to_json(false)), &COMMON_KEYS)
                    })
                    .collect(),
            ),
        );
        if !self.rules.is_empty() {
            key(
                "rules",
                lines(
                    self.rules
                        .iter()
                        .map(|rule| compact(&Value::from(rule.clone())))
                        .collect(),
                ),
            );
        }
        text.push_str("\n}\n");
        text
    }

    /// The object with this id.
    pub fn object(&self, id: Id) -> Option<&Object> {
        self.objects.iter().find(|object| object.id == id)
    }

    /// An id no object of the file has: one more than the highest.
    pub fn free_id(&self) -> Id {
        Id(self
            .objects
            .iter()
            .map(|object| object.id.0)
            .max()
            .unwrap_or(0)
            + 1)
    }

    /// Top-level keys the engine doesn't know (kept when it's written).
    pub fn other_keys(&self) -> impl Iterator<Item = &str> {
        self.other.keys().map(String::as_str)
    }
}

/// Writes the ids the engine gives into a scene file, so they stay the
/// same when objects are added or moved in the file. The file is written
/// again the way Duck Forge writes it. Returns how many objects had none.
/// (`cargo run --example write_ids -- FILE` does this from the command
/// line: `tools/write_ids.rs`.)
pub fn write_ids(path: impl AsRef<Path>) -> Result<usize, LoadError> {
    let path = path.as_ref();
    let scene = SceneFile::load(path)?;
    if scene.ids_missing > 0 {
        scene.save(path).map_err(|error| {
            LoadError::new(
                error.kind(),
                format!("couldn't write the scene file {}: {error}", path.display()),
            )
        })?;
    }
    Ok(scene.ids_missing)
}

/// "This scene can't be read", as one sentence with the file and line.
pub(crate) fn cannot_read(path: &Path, problem: &Problem) -> LoadError {
    LoadError::new(
        io::ErrorKind::InvalidData,
        problem.in_file(&crate::files::name_of(path)),
    )
}

/// What serde_json says, without its own "at line 3 column 5" (the line is
/// said once, in front).
fn json_words(error: &serde_json::Error) -> String {
    let text = error.to_string();
    let words = text.split(" at line ").next().unwrap_or(&text).to_string();
    format!("{words}, at column {}", error.column())
}

/// A scene's layers from its list: names, or `{"name": ..., "stays_on_screen": true}`.
fn layers_of(value: &Value) -> Option<Vec<Layer>> {
    value
        .as_array()?
        .iter()
        .map(|layer| match layer {
            Value::String(name) => Some(Layer {
                name: name.clone(),
                stays_on_screen: false,
            }),
            Value::Object(map) => Some(Layer {
                name: map.get("name")?.as_str()?.to_string(),
                stays_on_screen: map
                    .get("stays_on_screen")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }),
            _ => None,
        })
        .collect()
}

/// The line each object of `"objects": [ ... ]` starts on, from 1, found
/// by walking the text (so it works however the file is laid out).
fn object_lines(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut line, mut depth, mut i) = (1usize, 0usize, 0usize);
    // Where the list of objects starts: the first `[` after a top-level
    // "objects" key.
    let mut in_objects = false;
    let mut expect_list = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => line += 1,
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    if bytes.get(i) == Some(&b'\n') {
                        line += 1;
                    }
                    i += 1;
                }
                if depth == 1 && !in_objects && text.get(start..i) == Some("objects") {
                    expect_list = true;
                }
            }
            b'{' | b'[' => {
                if in_objects && depth == 2 {
                    lines.push(line);
                }
                if expect_list && bytes[i] == b'[' && depth == 1 {
                    in_objects = true;
                    expect_list = false;
                }
                depth += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if in_objects && depth == 1 {
                    return lines;
                }
            }
            // A value "objects" (not the key) is followed by a comma.
            b',' if depth == 1 => expect_list = false,
            b',' | b':' | b' ' | b'\t' | b'\r' => {}
            _ => {
                // A bare value (a number, true) where an object should be.
                if in_objects && depth == 2 && (i == 0 || !is_value_byte(bytes[i - 1])) {
                    lines.push(line);
                }
            }
        }
        i += 1;
    }
    lines
}

fn is_value_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')
}

/// JSON on one line, with a space after each comma and colon.
pub(crate) fn compact(value: &Value) -> String {
    compact_in_order(value, &[])
}

/// The same, with an object's keys in `first`'s order, then the rest in
/// alphabetical order.
pub(crate) fn compact_in_order(value: &Value, first: &[&str]) -> String {
    match value {
        Value::Array(items) => {
            let items: Vec<String> = items
                .iter()
                .map(|item| compact_in_order(item, first))
                .collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by_key(|key| {
                (
                    first.iter().position(|k| k == key).unwrap_or(first.len()),
                    key.as_str(),
                )
            });
            let items: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}: {}",
                        Value::from(key.clone()),
                        compact_in_order(&map[key], &[])
                    )
                })
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vec2;

    const SCENE: &str = r##"{
  "duckforge_scene": 1,
  "size": [1280, 240],
  "background": "#6ab4f0",
  "next": "level2",
  "made_with": "a note the engine doesn't know",
  "objects": [
    {"id": 1, "name": "sky", "kind": "background", "picture": "assets/sky.png", "layer": "far", "parallax": 0.2},
    {"id": 3, "name": "ground", "kind": "tilemap", "level": "level.txt"},
    {"id": 4, "name": "duck", "at": [40, 208], "picture": "assets/hero.json"},
    {"id": 7, "name": "sign", "at": [374, 208], "turn": 15, "scale": [2, 1], "picture": "assets/sign.png"}
  ],
  "rules": [
    "when duck touches coin: add 1 to score"
  ]
}
"##;

    #[test]
    fn a_scene_reads_and_writes_back_the_same_text() {
        let scene = SceneFile::parse(SCENE).unwrap();
        assert!(scene.problems.is_empty(), "{:?}", scene.problems);
        assert_eq!(scene.size, Some(vec2(1280.0, 240.0)));
        assert_eq!(scene.background, Some(Color::from_hex(0x6ab4f0)));
        assert_eq!(scene.next.as_deref(), Some("level2"));
        assert_eq!(scene.objects.len(), 4);
        assert_eq!(scene.objects[3].turn, 15.0);
        assert_eq!(scene.objects[0].number("parallax"), Some(0.2));
        assert_eq!(scene.rules.len(), 1);
        assert_eq!(scene.other_keys().collect::<Vec<_>>(), ["made_with"]);
        assert_eq!(scene.ids_missing, 0);
        assert_eq!(scene.to_text(), SCENE, "one object per line, as it was");
        assert_eq!(scene.free_id(), Id(8));
    }

    #[test]
    fn windows_line_endings_read_the_same() {
        let windows = SCENE.replace('\n', "\r\n");
        assert_eq!(
            SceneFile::parse(&windows).unwrap(),
            SceneFile::parse(SCENE).unwrap()
        );
    }

    #[test]
    fn objects_without_an_id_get_one_the_same_way_every_time() {
        let text = r#"{"duckforge_scene": 1, "objects": [
            {"name": "a"}, {"id": 5, "name": "b"}, {"name": "c"}, {"id": 2, "name": "d"}, {"name": "e"}]}"#;
        let ids = |scene: &SceneFile| -> Vec<u64> {
            scene.objects.iter().map(|o| o.id().number()).collect()
        };
        let scene = SceneFile::parse(text).unwrap();
        // After the highest id in the file (5), going down the file.
        assert_eq!(ids(&scene), [6, 5, 7, 2, 8]);
        assert_eq!(scene.ids_missing, 3);
        assert_eq!(
            ids(&SceneFile::parse(text).unwrap()),
            [6, 5, 7, 2, 8],
            "every time"
        );
        // Written into the file, they stay when an object is added above.
        let written = scene.to_text();
        let again = SceneFile::parse(&written).unwrap();
        assert_eq!(again.ids_missing, 0);
        assert_eq!(ids(&again), [6, 5, 7, 2, 8]);
        let with_one_more = written.replacen(
            "    {\"id\": 6,",
            "    {\"name\": \"new\"},\n    {\"id\": 6,",
            1,
        );
        assert_eq!(
            ids(&SceneFile::parse(&with_one_more).unwrap()),
            [9, 6, 5, 7, 2, 8]
        );
    }

    #[test]
    fn write_ids_puts_them_in_the_file() {
        let folder = crate::test_folder::TestFolder::new("scene-write-ids");
        let path = folder.join("level1.json");
        std::fs::write(
            &path,
            "{\"duckforge_scene\": 1, \"objects\": [{\"name\": \"a\"}, {\"name\": \"b\", \"speed\": 3}]}",
        )
        .unwrap();
        assert_eq!(write_ids(&path).unwrap(), 2);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("{\"id\": 1, \"name\": \"a\"}"), "{text}");
        assert!(
            text.contains("{\"id\": 2, \"name\": \"b\", \"speed\": 3}"),
            "{text}"
        );
        // Nothing to do the second time, and the file isn't touched.
        assert_eq!(write_ids(&path).unwrap(), 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn mistakes_are_said_in_plain_words_with_their_line() {
        let text = "{\n  \"duckforge_scene\": 1,\n  \"background\": \"blue\",\n  \"objects\": [\n    {\"id\": 1, \"name\": \"duck\", \"at\": 40},\n    7,\n    {\"id\": 1, \"name\": \"twin\", \"layer\": \"sky\"}\n  ]\n}\n";
        let scene = SceneFile::parse(text).unwrap();
        let said: Vec<String> = scene
            .problems
            .iter()
            .map(|p| p.in_file("scenes/level1.json"))
            .collect();
        let said = said.join("\n");
        assert!(
            said.contains("scenes/level1.json: \"background\" should be a colour like \"#6ab4f0\""),
            "{said}"
        );
        assert!(
            said.contains("scenes/level1.json, line 5: \"duck\": \"at\" should be two numbers"),
            "{said}"
        );
        assert!(
            said.contains("line 6: an object is written { ... }; this is 7"),
            "{said}"
        );
        assert!(
            said.contains("line 7: \"twin\" has the id 1, which an object above it has too"),
            "{said}"
        );
        assert!(
            said.contains("line 7: \"twin\" is on the layer \"sky\", which the scene doesn't have"),
            "{said}"
        );
        // The rest is read: two objects, the twin with a new id.
        assert_eq!(scene.objects.len(), 2);
        assert_eq!(scene.objects[1].id(), Id(2));
    }

    #[test]
    fn what_isnt_a_scene_at_all_is_an_error_with_its_line() {
        let broken =
            SceneFile::parse("{\n  \"objects\": [\n    {\"name\": \"duck\",}\n  ]\n}").unwrap_err();
        assert_eq!(broken.line, Some(3));
        assert!(
            broken.text.contains("isn't JSON a scene can be read from"),
            "{}",
            broken.text
        );
        let newer = SceneFile::parse("{\"duckforge_scene\": 2, \"objects\": []}").unwrap_err();
        assert!(newer.text.contains("newer Duck Forge"), "{}", newer.text);
        assert!(SceneFile::parse("[1, 2]").is_err());
    }

    #[test]
    fn a_3d_scene_is_kept_as_it_is() {
        let text = "{\n  \"duckforge_scene\": 1,\n  \"space\": \"3d\",\n  \"objects\": [\n    {\"id\": 2, \"name\": \"crate\", \"kind\": \"mesh\", \"at\": [-1.5, 0.5, 6], \"turn\": [0, 20, 0], \"model\": \"assets/crate.glb\", \"shape\": \"cube\"}\n  ]\n}\n";
        let scene = SceneFile::parse(text).unwrap();
        assert_eq!(scene.space, Space::ThreeD);
        assert!(scene.problems.iter().any(|p| p.text.contains("Phase 5b")));
        assert_eq!(
            scene.to_text(),
            text,
            "three numbers and the model survive a save"
        );
    }

    #[test]
    fn layers_can_be_named_and_one_can_stay_on_screen() {
        let text = "{\n  \"duckforge_scene\": 1,\n  \"layers\": [\"sky\", \"world\", {\"name\": \"top\", \"stays_on_screen\": true}],\n  \"objects\": []\n}\n";
        let scene = SceneFile::parse(text).unwrap();
        assert_eq!(scene.layers.len(), 3);
        assert!(scene.layers[2].stays_on_screen);
        assert_eq!(scene.to_text(), text);
    }

    #[test]
    fn lines_are_found_however_the_file_is_laid_out() {
        assert_eq!(object_lines(SCENE), [8, 9, 10, 11]);
        let spread = "{\"objects\":\n[\n{\n\"name\": \"a\",\n\"at\": [1, 2]\n},\n{\"name\": \"[b\\\"]\"}, {\"name\": \"c\"}\n]}";
        assert_eq!(object_lines(spread), [3, 7, 7]);
        assert_eq!(
            object_lines("{\"other\": [{\"objects\": 1}], \"objects\": [{}]}"),
            [1]
        );
    }
}

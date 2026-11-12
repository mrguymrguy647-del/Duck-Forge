//! The scene the editor shows: its file as text and as objects, the tools
//! of the view, snapping, and the rows of the Scene list.
//!
//! A scene file says how the scene starts. The Studio changes it by its
//! objects' keys (`{"at": [96, 208]}`), writes the whole file again, and
//! sends the frozen game the new text. Where objects are right now comes
//! from the game, with every frozen picture.

use std::path::{Path, PathBuf};

use duckforge::prelude::Vec2;
use duckforge::scene::{Object, SceneFile};
use duckforge::studio_link::protocol::{DrawnObject, DrawnScene, Origin};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::problem::Problem;

/// One scene file, as the editor has it.
#[derive(Debug, Clone)]
pub struct SceneDoc {
    /// Where the file is.
    pub path: PathBuf,
    /// Its name as the game names it: from the Duck Forge folder, with
    /// forward slashes (`examples/bee_maze/scenes/level1.json`).
    pub name: String,
    /// Its text on disk, as last seen.
    pub text: String,
    /// What it says. `None` if it can't be read at all.
    pub file: Option<SceneFile>,
    /// Why it can't be read, or the first mistake that leaves something
    /// of it out: in plain words, with the line.
    pub problem: Option<String>,
}

impl SceneDoc {
    /// Reads `text`, the file's text.
    pub fn read(path: &Path, name: &str, text: &str) -> SceneDoc {
        let short = short_name(name);
        let (file, problem) = match SceneFile::parse(text) {
            Ok(file) => {
                let problem = file.would_be_lost().map(|problem| problem.in_file(short));
                (Some(file), problem)
            }
            Err(problem) => (None, Some(problem.in_file(short))),
        };
        SceneDoc {
            path: path.to_path_buf(),
            name: name.to_string(),
            text: text.to_string(),
            file,
            problem,
        }
    }

    /// The file's own name: `pond.json`.
    pub fn short_name(&self) -> &str {
        short_name(&self.name)
    }

    /// The object with this id, as the file has it.
    pub fn object(&self, id: u64) -> Option<&Object> {
        self.file
            .as_ref()?
            .objects
            .iter()
            .find(|object| object.id().number() == id)
    }

    /// The line of the file the object with this id is on.
    pub fn line_of(&self, id: u64) -> Option<usize> {
        let file = self.file.as_ref()?;
        let object = self.object(id)?;
        file.line_of(object.id())
    }

    /// The file's text with these keys set on each of these objects, as
    /// the engine writes a scene. The file must be readable and free of
    /// mistakes: writing it again would drop the lines it can't read.
    pub fn with_set(&self, changes: &[(u64, Map<String, Value>)]) -> Result<String, Problem> {
        let short = self.short_name();
        let Some(file) = &self.file else {
            return Err(Problem::new(
                format!("The Studio can't read {short}."),
                "Fix it first: open it from Files. Undo takes back a change that broke it.",
            )
            .with_details(self.problem.clone().unwrap_or_default()));
        };
        if let Some(problem) = &self.problem {
            return Err(Problem::new(
                format!("{short} has a mistake, so the Studio won't write it again."),
                "Fix it first: open it from Files. Then change the object.",
            )
            .with_details(problem.clone()));
        }
        let was = file;
        let mut file = file.clone();
        for (id, set) in changes {
            let Some(object) = file
                .objects
                .iter()
                .find(|object| object.id().number() == *id)
            else {
                return Err(Problem::new(
                    format!("{short} has no object with id {id}."),
                    "Pick one from the Scene list. (An object the game made while playing isn't in the file.)",
                ));
            };
            let (id, name) = (object.id(), describe(object));
            file.set(id, set).map_err(|wrong| {
                Problem::new(
                    format!("{name} can't be changed like that."),
                    "Check the value, then try again.",
                )
                .with_details(wrong)
            })?;
        }
        if file.objects == was.objects {
            // Nothing to write: the file keeps its own spacing.
            return Ok(self.text.clone());
        }
        Ok(file.to_text())
    }
}

/// `pond.json` from `examples/bee_maze/scenes/pond.json`.
fn short_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// An object in a few words, for a message: its name, or "the coin (id 7)".
pub fn describe(object: &Object) -> String {
    if object.name.is_empty() {
        format!("the {} (id {})", object.kind, object.id())
    } else {
        object.name.clone()
    }
}

/// Whether two names are the same scene file, however their slashes lean
/// and whether or not one starts at the Duck Forge folder.
pub fn same_file(a: &str, b: &str) -> bool {
    let (a, b) = (a.replace('\\', "/"), b.replace('\\', "/"));
    let (a, b) = (a.trim_start_matches("./"), b.trim_start_matches("./"));
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}

/// The view's tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// Click to select; a drag on an object moves it too.
    #[default]
    Select,
    /// Drag the selected objects, or an arrow to keep to one direction.
    Move,
}

/// What a frozen picture shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Show {
    /// The game as it is on the frozen frame.
    #[default]
    Frozen,
    /// The scene as its file has it, with nothing moved.
    Start,
}

/// How the view is set: its tool, snapping, the grid, what it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneView {
    pub tool: Tool,
    pub snap: bool,
    /// The grid's squares, in the scene's pixels.
    pub snap_step: f32,
    pub grid: bool,
    pub show: Show,
}

/// The grid's squares unless set otherwise.
pub const USUAL_SNAP: f32 = 8.0;

impl Default for SceneView {
    fn default() -> SceneView {
        SceneView {
            tool: Tool::Select,
            snap: true,
            snap_step: USUAL_SNAP,
            grid: true,
            show: Show::Frozen,
        }
    }
}

impl SceneView {
    /// Where a dragged object goes: on the grid if snapping is on (and
    /// `free`, the Alt key, isn't held), on a whole pixel either way.
    pub fn place(&self, at: Vec2, free: bool) -> Vec2 {
        if self.snap && !free {
            snapped(at, self.snap_step)
        } else {
            Vec2::new(at.x.round(), at.y.round())
        }
    }
}

/// `at` on the nearest corner of a grid of `step` pixels.
pub fn snapped(at: Vec2, step: f32) -> Vec2 {
    if step <= 0.0 {
        return at;
    }
    Vec2::new((at.x / step).round() * step, (at.y / step).round() * step)
}

/// How an object in the Scene list stands to the file and the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// In the file, and in the game (or the game isn't frozen to say).
    There,
    /// In the file, but the game has removed it (a coin that was taken).
    Gone,
    /// Not in the file: the game made it while playing.
    Made,
}

/// One row of the Scene list.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneRow {
    pub id: u64,
    /// Its name, or its kind if it has none.
    pub label: String,
    pub kind: String,
    pub layer: String,
    pub state: RowState,
    /// It isn't where it starts (it shows twice in the view).
    pub moved: bool,
    /// For a tile map: what's painted on it, like "4 coins, 1 flag".
    pub painted: Option<String>,
    pub selected: bool,
}

/// The Scene list: the file's objects in the file's order (the drawing
/// order), then whatever the game has made while playing. `drawn` is what
/// the frozen game says about the same scene, if it's frozen.
pub fn rows(doc: &SceneDoc, drawn: Option<&DrawnScene>, selected: &[u64]) -> Vec<SceneRow> {
    let Some(file) = &doc.file else {
        return Vec::new();
    };
    let label = |name: &str, kind: &str| {
        if name.is_empty() {
            kind.to_string()
        } else {
            name.to_string()
        }
    };
    let mut rows: Vec<SceneRow> = file
        .objects
        .iter()
        .map(|object| {
            let id = object.id().number();
            let now = drawn.and_then(|drawn| drawn.object(id));
            SceneRow {
                id,
                label: label(&object.name, &object.kind),
                kind: object.kind.clone(),
                layer: object.layer.clone(),
                state: if now.is_some_and(|now| now.gone) {
                    RowState::Gone
                } else {
                    RowState::There
                },
                moved: now.is_some_and(|now| !now.gone && now.now.contains_key("at")),
                painted: (object.kind == "tilemap")
                    .then(|| drawn.map(painted_on))
                    .flatten()
                    .filter(|painted| !painted.is_empty()),
                selected: selected.contains(&id),
            }
        })
        .collect();
    if let Some(drawn) = drawn {
        rows.extend(
            drawn
                .objects
                .iter()
                .filter(|object| object.origin == Origin::Made)
                .map(|object| SceneRow {
                    id: object.id,
                    label: label(&object.name, &object.kind),
                    kind: object.kind.clone(),
                    layer: object.layer.clone(),
                    state: RowState::Made,
                    moved: false,
                    painted: None,
                    selected: selected.contains(&object.id),
                }),
        );
    }
    rows
}

/// What's painted on the scene's tile maps and still there, by kind:
/// "3 coins, 1 flag".
fn painted_on(drawn: &DrawnScene) -> String {
    let mut kinds: Vec<(&str, usize)> = Vec::new();
    for object in &drawn.objects {
        if object.origin != Origin::Painted || object.gone {
            continue;
        }
        match kinds.iter_mut().find(|(kind, _)| *kind == object.kind) {
            Some((_, count)) => *count += 1,
            None => kinds.push((&object.kind, 1)),
        }
    }
    kinds
        .iter()
        .map(|(kind, count)| {
            if *count == 1 {
                format!("1 {kind}")
            } else {
                format!("{count} {kind}s")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The object under a point of the game's picture: the one drawn last
/// (on top) whose picture covers it. Painted things and tile maps aren't
/// picked: they belong to the tile map's own tools. With `under`, the
/// next one down from that object, so clicking a pile again goes through
/// it.
pub fn object_at(drawn: &DrawnScene, point: Vec2, under: Option<u64>) -> Option<u64> {
    let hits: Vec<u64> = drawn
        .objects
        .iter()
        .rev()
        .filter(|object| can_be_picked(object))
        .filter(|object| object.rect.is_some_and(|rect| rect.contains(point)))
        .map(|object| object.id)
        .collect();
    let after = under
        .and_then(|under| hits.iter().position(|id| *id == under))
        .map_or(0, |index| (index + 1) % hits.len().max(1));
    hits.get(after).copied()
}

/// The object whose start outline (where it starts, when it has moved)
/// covers a point of the game's picture.
pub fn start_at(drawn: &DrawnScene, point: Vec2) -> Option<u64> {
    drawn
        .objects
        .iter()
        .rev()
        .filter(|object| can_be_picked(object))
        .find(|object| object.start_rect.is_some_and(|rect| rect.contains(point)))
        .map(|object| object.id)
}

fn can_be_picked(object: &DrawnObject) -> bool {
    object.origin != Origin::Painted
}

/// Where an object is right now, as the frozen game says: its changed
/// "at" if it has moved, or else where the file starts it.
pub fn at_now(doc: &SceneDoc, drawn: Option<&DrawnScene>, id: u64) -> Option<Vec2> {
    let moved = drawn
        .and_then(|drawn| drawn.object(id))
        .and_then(|object| object.now.get("at"))
        .and_then(point_of);
    moved.or_else(|| doc.object(id).map(|object| object.at))
}

/// `[x, y]` as a point.
pub fn point_of(value: &Value) -> Option<Vec2> {
    let pair = value.as_array()?;
    Some(Vec2::new(
        pair.first()?.as_f64()? as f32,
        pair.get(1)?.as_f64()? as f32,
    ))
}

/// A point as the scene file writes it: whole numbers without a `.0`.
pub fn pair(at: Vec2) -> Value {
    let number = |n: f32| {
        if n.fract() == 0.0 && n.abs() < 1e9 {
            Value::from(n as i64)
        } else {
            Value::from((f64::from(n) * 1000.0).round() / 1000.0)
        }
    };
    Value::Array(vec![number(at.x), number(at.y)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckforge::prelude::Rect;

    const POND: &str = r#"{
  "objects": [
    {"id": 1, "name": "level", "kind": "tilemap", "level": "level.txt"},
    {"id": 2, "name": "sign", "at": [120, 208], "picture": "assets/sign.png"},
    {"id": 3, "kind": "coin", "at": [200, 180]},
    {"id": 4, "name": "duck", "kind": "player", "at": [40, 208], "layer": "front"}
  ]
}
"#;

    fn doc() -> SceneDoc {
        SceneDoc::read(
            Path::new("scenes/pond.json"),
            "examples/pond/scenes/pond.json",
            POND,
        )
    }

    fn set(text: &str) -> Map<String, Value> {
        serde_json::from_str(text).unwrap()
    }

    fn drawn_object(id: u64, kind: &str, rect: Option<Rect>) -> DrawnObject {
        DrawnObject {
            id,
            name: String::new(),
            kind: kind.to_string(),
            layer: String::from("things"),
            origin: Origin::File,
            stays_on_screen: false,
            rect,
            start_rect: None,
            gone: false,
            now: Map::new(),
        }
    }

    #[test]
    fn a_change_is_written_as_the_engine_writes_a_scene() {
        let doc = doc();
        let text = doc
            .with_set(&[(2, set(r#"{"at": [96, 200], "layer": "back"}"#))])
            .unwrap();
        let again = SceneDoc::read(&doc.path, &doc.name, &text);
        let sign = again.object(2).unwrap();
        assert_eq!(sign.at, Vec2::new(96.0, 200.0));
        assert_eq!(sign.layer, "back");
        assert_eq!(again.object(4).unwrap().at, Vec2::new(40.0, 208.0));
        assert_eq!(
            again.file.as_ref().unwrap().to_text(),
            text,
            "written the one way the engine writes it"
        );
        assert_eq!(again.line_of(2), Some(5), "one object on each line");
    }

    #[test]
    fn a_wrong_value_or_a_missing_object_is_said_and_nothing_is_written() {
        let doc = doc();
        let wrong = doc.with_set(&[(2, set(r#"{"at": "left"}"#))]).unwrap_err();
        assert_eq!(wrong.what, "sign can't be changed like that.");
        assert!(wrong.details.contains("\"at\""), "{}", wrong.details);
        let missing = doc.with_set(&[(99, set(r#"{"at": [1, 2]}"#))]).unwrap_err();
        assert!(missing.what.contains("no object with id 99"), "{missing:?}");
    }

    #[test]
    fn a_file_with_a_mistake_isnt_written_again() {
        let text = POND.replace(r#""at": [200, 180]"#, r#""at": "over there""#);
        let doc = SceneDoc::read(Path::new("pond.json"), "scenes/pond.json", &text);
        assert!(
            doc.problem.as_ref().unwrap().contains("line 5"),
            "{:?}",
            doc.problem
        );
        let problem = doc.with_set(&[(2, set(r#"{"at": [1, 2]}"#))]).unwrap_err();
        assert!(problem.what.contains("has a mistake"), "{problem:?}");

        let broken = SceneDoc::read(Path::new("pond.json"), "scenes/pond.json", "{ not json");
        assert!(broken.file.is_none());
        assert!(broken.with_set(&[]).is_err());
    }

    #[test]
    fn snapping_puts_a_point_on_the_grid_and_alt_leaves_it_free() {
        let view = SceneView::default();
        assert_eq!(
            view.place(Vec2::new(43.4, 205.0), false),
            Vec2::new(40.0, 208.0)
        );
        assert_eq!(
            view.place(Vec2::new(43.4, 205.0), true),
            Vec2::new(43.0, 205.0)
        );
        let no_snap = SceneView {
            snap: false,
            ..view
        };
        assert_eq!(
            no_snap.place(Vec2::new(43.6, 205.2), false),
            Vec2::new(44.0, 205.0)
        );
        assert_eq!(
            snapped(Vec2::new(-5.0, 23.0), 16.0),
            Vec2::new(0.0, 16.0),
            "to the nearest corner, on either side of nothing"
        );
    }

    #[test]
    fn the_list_has_the_files_objects_then_what_the_game_made() {
        let doc = doc();
        let plain = rows(&doc, None, &[4]);
        let labels: Vec<&str> = plain.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["level", "sign", "coin", "duck"]);
        assert!(plain[3].selected && !plain[0].selected);
        assert!(plain.iter().all(|row| row.state == RowState::There));

        let mut duck = drawn_object(4, "player", Some(Rect::new(90.0, 190.0, 16.0, 18.0)));
        duck.now = set(r#"{"at": [98, 208]}"#);
        let mut coin = drawn_object(3, "coin", None);
        coin.gone = true;
        let mut made = drawn_object(1 << 32, "puff", Some(Rect::new(0.0, 0.0, 4.0, 4.0)));
        made.origin = Origin::Made;
        let painted = |id: u64, kind: &str, gone: bool| {
            let mut thing = drawn_object(id, kind, Some(Rect::new(0.0, 0.0, 8.0, 8.0)));
            thing.origin = Origin::Painted;
            thing.gone = gone;
            thing
        };
        let drawn = DrawnScene {
            file: String::from("examples/pond/scenes/pond.json"),
            scale: 1.0,
            offset: Vec2::ZERO,
            as_it_starts: false,
            objects: vec![
                drawn_object(1, "tilemap", None),
                drawn_object(2, "object", Some(Rect::new(112.0, 190.0, 16.0, 18.0))),
                duck,
                painted((1 << 48) + 1, "coin", false),
                painted((1 << 48) + 2, "coin", false),
                painted((1 << 48) + 3, "coin", true),
                painted((1 << 48) + 4, "flag", false),
                made,
                coin,
            ],
        };
        let frozen = rows(&doc, Some(&drawn), &[]);
        assert_eq!(frozen.len(), 5);
        assert_eq!(frozen[0].painted.as_deref(), Some("2 coins, 1 flag"));
        assert_eq!(frozen[2].state, RowState::Gone, "the coin the duck took");
        assert!(frozen[3].moved, "the duck has moved since the start");
        assert_eq!(frozen[4].state, RowState::Made);
        assert_eq!(frozen[4].label, "puff");

        assert_eq!(
            at_now(&doc, Some(&drawn), 4),
            Some(Vec2::new(98.0, 208.0)),
            "where the game has it"
        );
        assert_eq!(at_now(&doc, Some(&drawn), 2), Some(Vec2::new(120.0, 208.0)));
        assert_eq!(at_now(&doc, None, 4), Some(Vec2::new(40.0, 208.0)));
    }

    #[test]
    fn a_click_picks_the_object_on_top_and_then_the_one_under_it() {
        let mut bush = drawn_object(5, "object", Some(Rect::new(100.0, 100.0, 40.0, 20.0)));
        bush.start_rect = Some(Rect::new(10.0, 10.0, 40.0, 20.0));
        let mut painted = drawn_object(1 << 48, "coin", Some(Rect::new(0.0, 0.0, 300.0, 300.0)));
        painted.origin = Origin::Painted;
        let drawn = DrawnScene {
            file: String::new(),
            scale: 1.0,
            offset: Vec2::ZERO,
            as_it_starts: false,
            objects: vec![
                drawn_object(2, "object", Some(Rect::new(96.0, 96.0, 20.0, 20.0))),
                bush,
                painted,
            ],
        };
        let both = Vec2::new(105.0, 105.0);
        assert_eq!(object_at(&drawn, both, None), Some(5), "the one on top");
        assert_eq!(object_at(&drawn, both, Some(5)), Some(2), "then under it");
        assert_eq!(object_at(&drawn, both, Some(2)), Some(5), "and round again");
        assert_eq!(object_at(&drawn, Vec2::new(130.0, 110.0), Some(5)), Some(5));
        assert_eq!(
            object_at(&drawn, Vec2::new(250.0, 250.0), None),
            None,
            "a painted thing is the tile map's"
        );
        assert_eq!(start_at(&drawn, Vec2::new(20.0, 20.0)), Some(5));
        assert_eq!(start_at(&drawn, both), None);
    }

    #[test]
    fn names_of_the_same_file_match_however_they_are_written() {
        assert!(same_file(
            "examples/pond/scenes/pond.json",
            "examples\\pond\\scenes\\pond.json"
        ));
        assert!(same_file(
            "scenes/pond.json",
            "examples/pond/scenes/pond.json"
        ));
        assert!(!same_file("scenes/pond.json", "scenes/big_pond.json"));
    }

    #[test]
    fn points_are_written_as_the_scene_file_writes_them() {
        assert_eq!(pair(Vec2::new(96.0, -8.0)).to_string(), "[96,-8]");
        assert_eq!(pair(Vec2::new(1.5, 2.0)).to_string(), "[1.5,2]");
        assert_eq!(
            point_of(&pair(Vec2::new(3.0, 4.0))),
            Some(Vec2::new(3.0, 4.0))
        );
    }
}

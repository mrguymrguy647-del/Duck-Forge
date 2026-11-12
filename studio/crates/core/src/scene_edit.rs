//! The scene the editor shows: its file as text and as objects, the tools
//! of the view, snapping, and the rows of the Scene list.
//!
//! A scene file says how the scene starts. The Studio changes it by its
//! objects' keys (`{"at": [96, 208]}`), writes the whole file again, and
//! sends the frozen game the new text. Where objects are right now comes
//! from the game, with every frozen picture.

use std::path::{Path, PathBuf};

use duckforge::prelude::{Rect, Vec2};
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

    /// The file's text after `change` has been made to its objects, as
    /// the engine writes a scene, and what `change` said. The file must be
    /// readable and free of mistakes: writing it again would drop the
    /// lines it can't read. If nothing changed, the text is as it was.
    pub fn edited<R>(
        &self,
        change: impl FnOnce(&mut SceneFile) -> Result<R, Problem>,
    ) -> Result<(String, R), Problem> {
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
        let mut changed = file.clone();
        let said = change(&mut changed)?;
        if changed.objects == file.objects {
            // Nothing to write: the file keeps its own spacing.
            return Ok((self.text.clone(), said));
        }
        Ok((changed.to_text(), said))
    }

    /// The file's text with these keys set on each of these objects.
    pub fn with_set(&self, changes: &[(u64, Map<String, Value>)]) -> Result<String, Problem> {
        let short = self.short_name().to_string();
        let (text, ()) = self.edited(|file| {
            for (id, set) in changes {
                let object = in_file(file, *id, &short)?;
                let (id, name) = (object.id(), describe(object));
                file.set(id, set).map_err(|wrong| {
                    Problem::new(
                        format!("{name} can't be changed like that."),
                        "Check the value, then try again.",
                    )
                    .with_details(wrong)
                })?;
            }
            Ok(())
        })?;
        Ok(text)
    }
}

/// The object with this id in `file`, or a plain problem saying it isn't.
pub fn in_file<'a>(file: &'a SceneFile, id: u64, short: &str) -> Result<&'a Object, Problem> {
    file.objects
        .iter()
        .find(|object| object.id().number() == id)
        .ok_or_else(|| {
            Problem::new(
                format!("{short} has no object with id {id}."),
                "Pick one from the Scene list. (An object the game made while playing isn't in the file.)",
            )
        })
}

/// An object's line, ready to be added again as a new object: its keys as
/// the file has them, without its id.
pub fn line_without_id(object: &Object) -> Map<String, Value> {
    let mut line = object.to_json(false);
    line.remove("id");
    line
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
    /// Drag round the ring to turn the selected objects.
    Turn,
    /// Drag a corner to make the selected objects bigger or smaller.
    Size,
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
        .filter(|object| {
            shape(object).is_some_and(|(rect, pivot, turn)| covers(rect, pivot, turn, point))
        })
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
        .find(|object| {
            start_shape(object).is_some_and(|(rect, pivot, turn)| covers(rect, pivot, turn, point))
        })
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

/// The four corners of a picture placed at `rect` on the canvas and
/// turned `turn` degrees clockwise round `pivot`, in the order top-left,
/// top-right, bottom-right, bottom-left (before turning).
pub fn corners(rect: Rect, pivot: Vec2, turn: f32) -> [Vec2; 4] {
    let (sin, cos) = turn.to_radians().sin_cos();
    let spin = |p: Vec2| {
        let d = p - pivot;
        Vec2::new(
            pivot.x + d.x * cos - d.y * sin,
            pivot.y + d.x * sin + d.y * cos,
        )
    };
    [
        spin(Vec2::new(rect.x, rect.y)),
        spin(Vec2::new(rect.x + rect.w, rect.y)),
        spin(Vec2::new(rect.x + rect.w, rect.y + rect.h)),
        spin(Vec2::new(rect.x, rect.y + rect.h)),
    ]
}

/// Whether `point` is on a picture placed at `rect` and turned `turn`
/// degrees round `pivot`.
pub fn covers(rect: Rect, pivot: Vec2, turn: f32, point: Vec2) -> bool {
    let (sin, cos) = (-turn).to_radians().sin_cos();
    let d = point - pivot;
    let back = Vec2::new(
        pivot.x + d.x * cos - d.y * sin,
        pivot.y + d.x * sin + d.y * cos,
    );
    rect.contains(back)
}

/// The smallest upright box round a turned picture.
pub fn bounds(rect: Rect, pivot: Vec2, turn: f32) -> Rect {
    let points = corners(rect, pivot, turn);
    let (mut low, mut high) = (points[0], points[0]);
    for p in &points[1..] {
        low = Vec2::new(low.x.min(p.x), low.y.min(p.y));
        high = Vec2::new(high.x.max(p.x), high.y.max(p.y));
    }
    Rect::new(low.x, low.y, high.x - low.x, high.y - low.y)
}

/// Where an object is drawn now: its picture's place before turning, the
/// point it turns round, and its turn. `None` if it has no place.
pub fn shape(object: &DrawnObject) -> Option<(Rect, Vec2, f32)> {
    let rect = object.rect?;
    Some((rect, object.pivot.unwrap_or(rect_middle(rect)), object.turn))
}

/// The same for where it starts, when that's somewhere else.
pub fn start_shape(object: &DrawnObject) -> Option<(Rect, Vec2, f32)> {
    let rect = object.start_rect?;
    Some((
        rect,
        object.start_pivot.unwrap_or(rect_middle(rect)),
        object.start_turn,
    ))
}

fn rect_middle(rect: Rect) -> Vec2 {
    Vec2::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

/// The objects a box drawn on the picture (from `a` to `b`, canvas
/// pixels) touches, in drawing order. Painted things, tile maps and
/// backgrounds aren't picked.
pub fn in_box(drawn: &DrawnScene, a: Vec2, b: Vec2) -> Vec<u64> {
    let area = Rect::new(
        a.x.min(b.x),
        a.y.min(b.y),
        (a.x - b.x).abs(),
        (a.y - b.y).abs(),
    );
    drawn
        .objects
        .iter()
        .filter(|object| can_be_picked(object))
        .filter(|object| {
            shape(object).is_some_and(|(rect, pivot, turn)| {
                let round = bounds(rect, pivot, turn);
                round.x < area.x + area.w
                    && area.x < round.x + round.w
                    && round.y < area.y + area.h
                    && area.y < round.y + round.h
            })
        })
        .map(|object| object.id)
        .collect()
}

/// How far a turn snaps, in degrees.
pub const TURN_STEP: f32 = 15.0;

/// The turn of an object turned by dragging from `pressed` to `pointer`
/// round `pivot`, having started at `start` degrees. Snapped to 15
/// degrees unless `free`; whole degrees either way, between -180 and 180.
pub fn turned(start: f32, pivot: Vec2, pressed: Vec2, pointer: Vec2, free: bool) -> f32 {
    let angle = |p: Vec2| (p.y - pivot.y).atan2(p.x - pivot.x).to_degrees();
    let turn = start + angle(pointer) - angle(pressed);
    let turn = if free {
        turn.round()
    } else {
        (turn / TURN_STEP).round() * TURN_STEP
    };
    tidy_turn(turn)
}

/// A turn written between -180 and 180 degrees.
pub fn tidy_turn(turn: f32) -> f32 {
    let turn = (turn + 180.0).rem_euclid(360.0) - 180.0;
    if turn == -180.0 { 180.0 } else { turn }
}

/// How many times bigger, across and down, an object gets when the corner
/// of its picture that was at `handle` is dragged to `pointer` (canvas
/// pixels). It grows round its `pivot`, its `at`, which stays where it
/// is: so a crate standing on the ground still stands there. A corner
/// level with the pivot (the bottom corners of something standing on its
/// feet) changes only the width. With `keep_shape` (Shift), both by the
/// same amount.
pub fn resized(
    shape: (Rect, Vec2, f32),
    handle: Vec2,
    pointer: Vec2,
    keep_shape: bool,
) -> (f32, f32) {
    let (_, pivot, turn) = shape;
    let (sin, cos) = (-turn).to_radians().sin_cos();
    let unturn = |p: Vec2| {
        let d = p - pivot;
        Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    };
    let (from, to) = (unturn(handle), unturn(pointer));
    let ratio = |from: f32, to: f32| (from.abs() >= 2.0).then(|| (to / from).max(0.0));
    let (across, down) = (ratio(from.x, to.x), ratio(from.y, to.y));
    match (across, down, keep_shape) {
        (Some(x), Some(y), true) => {
            let r = if (x - 1.0).abs() >= (y - 1.0).abs() {
                x
            } else {
                y
            };
            (r, r)
        }
        (Some(x), Some(y), false) => (x, y),
        (Some(r), None, true) | (None, Some(r), true) => (r, r),
        (Some(x), None, false) => (x, 1.0),
        (None, Some(y), false) => (1.0, y),
        (None, None, _) => (1.0, 1.0),
    }
}

/// A size, tidied: on quarters (1, 1.25, 1.5...) unless `free`, in
/// hundredths if it is; never smaller than a quarter (a twentieth if
/// free), never bigger than 64.
pub fn tidy_scale(scale: f32, free: bool) -> f32 {
    let (step, least) = if free { (0.01, 0.05) } else { (0.25, 0.25) };
    ((scale / step).round() * step).clamp(least, 64.0)
}

/// How a move of these objects is made, decided once when it begins.
/// Objects still where they start move their start (the file). Objects
/// the game has moved since the start, and objects the game made, move
/// for this run only. Each comes with where it is now. Tile maps,
/// backgrounds and what the game has removed don't move.
pub struct Moving {
    pub starts: Vec<(u64, Vec2)>,
    pub now: Vec<(u64, Vec2)>,
}

pub fn moving(doc: &SceneDoc, drawn: &DrawnScene, ids: &[u64]) -> Moving {
    let mut moving = Moving {
        starts: Vec::new(),
        now: Vec::new(),
    };
    for &id in ids {
        let Some(object) = drawn.object(id) else {
            continue;
        };
        if object.gone || matches!(object.kind.as_str(), "tilemap" | "background") {
            continue;
        }
        let moved = object.origin == Origin::Made || object.now.contains_key("at");
        match (moved, doc.object(id)) {
            (false, Some(start)) => moving.starts.push((id, start.at)),
            _ => {
                let at = at_now(doc, Some(drawn), id)
                    .or_else(|| object.now.get("at").and_then(point_of));
                if let Some(at) = at {
                    moving.now.push((id, at));
                }
            }
        }
    }
    moving
}

/// Objects' lines from text someone pasted: one line of a scene file each
/// (`{"name": "crate", "at": [96, 208]},`), a list of them, or a whole
/// scene file. `None` if it isn't that (any other text on the
/// clipboard): then Paste uses the Studio's own copy.
pub fn lines_from_text(text: &str) -> Option<Vec<Map<String, Value>>> {
    let text = text.trim();
    let whole: Option<Value> = serde_json::from_str(text).ok();
    let objects: Vec<Value> = match whole {
        Some(Value::Array(items)) => items,
        Some(Value::Object(map)) => match map.get("objects") {
            Some(Value::Array(items)) => items.clone(),
            _ => vec![Value::Object(map)],
        },
        _ => {
            let mut items = Vec::new();
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let line = line.trim_end_matches(',');
                items.push(serde_json::from_str(line).ok()?);
            }
            items
        }
    };
    let looks_like_one = |map: &Map<String, Value>| {
        ["at", "picture", "kind", "name"]
            .iter()
            .any(|key| map.contains_key(*key))
    };
    let lines: Vec<Map<String, Value>> = objects
        .into_iter()
        .map(|item| match item {
            Value::Object(map) if looks_like_one(&map) => Some(map),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (!lines.is_empty()).then_some(lines)
}

/// The clipboard's text for these lines: one a line, as the file writes
/// them, so they can be pasted into a scene file by hand too.
pub fn clipboard_text(lines: &[Map<String, Value>]) -> String {
    lines
        .iter()
        .map(duckforge::scene::line_text)
        .collect::<Vec<_>>()
        .join(",\n")
}

/// A kind that Add offers: the plain kinds, which do nothing by
/// themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlainKind {
    /// As the file writes it.
    pub kind: &'static str,
    pub title: &'static str,
    pub says: &'static str,
    /// What it needs chosen first: a picture, a level, or nothing.
    pub needs: Needs,
    /// Why it can't be added yet, if it can't.
    pub not_yet: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    Picture,
    Level,
}

/// The plain kinds, in the order Add shows them.
pub const PLAIN_KINDS: &[PlainKind] = &[
    PlainKind {
        kind: "picture",
        title: "Picture",
        says: "A picture or an animation from Duck Quill.",
        needs: Needs::Picture,
        not_yet: None,
    },
    PlainKind {
        kind: "text",
        title: "Text",
        says: "Words on the screen: a title, a score.",
        needs: Needs::Nothing,
        not_yet: Some("Text comes with several scenes, in step 7."),
    },
    PlainKind {
        kind: "button",
        title: "Button",
        says: "Click or press to go somewhere.",
        needs: Needs::Nothing,
        not_yet: Some("Buttons come with several scenes, in step 7."),
    },
    PlainKind {
        kind: "sound",
        title: "Sound",
        says: "Music or a sound that plays in this scene.",
        needs: Needs::Nothing,
        not_yet: Some("Sounds come with dragging them in from Files, in step 4."),
    },
    PlainKind {
        kind: "area",
        title: "Area",
        says: "An invisible box. The game can ask what's in it.",
        needs: Needs::Nothing,
        not_yet: None,
    },
    PlainKind {
        kind: "tilemap",
        title: "Tile map",
        says: "A level painted on a grid.",
        needs: Needs::Level,
        not_yet: None,
    },
    PlainKind {
        kind: "background",
        title: "Background",
        says: "A picture far behind, that moves slower than the camera.",
        needs: Needs::Picture,
        not_yet: None,
    },
];

/// The plain kind with this name.
pub fn plain_kind(kind: &str) -> Option<&'static PlainKind> {
    PLAIN_KINDS.iter().find(|plain| plain.kind == kind)
}

/// An area's box, unless it says otherwise: two tiles square.
pub const AREA_SIZE: f32 = 32.0;

/// A new object's line: its kind, where it goes, and what it needs (a
/// picture, a level), with a name from that.
pub fn new_line(kind: &str, at: Vec2, chosen: Option<&str>, layer: &str) -> Map<String, Value> {
    let mut line = Map::new();
    let stem = chosen.map(|file| {
        let file = file.rsplit('/').next().unwrap_or(file);
        file.split('.').next().unwrap_or(file).to_string()
    });
    let name = stem.unwrap_or_else(|| kind.to_string());
    line.insert(String::from("name"), Value::from(name));
    if kind != "picture" {
        line.insert(String::from("kind"), Value::from(kind));
    }
    match kind {
        "tilemap" => {
            if let Some(level) = chosen {
                line.insert(String::from("level"), Value::from(level));
            }
            return line;
        }
        "background" => {
            // Its top edge halfway down the view; it repeats across.
            line.insert(String::from("at"), pair(Vec2::new(0.0, at.y)));
            if let Some(picture) = chosen {
                line.insert(String::from("picture"), Value::from(picture));
            }
            line.insert(String::from("parallax"), Value::from(0.5));
        }
        "area" => {
            line.insert(String::from("at"), pair(at));
            line.insert(
                String::from("box"),
                serde_json::json!([0, 0, AREA_SIZE as i64, AREA_SIZE as i64]),
            );
        }
        _ => {
            line.insert(String::from("at"), pair(at));
            if let Some(picture) = chosen {
                line.insert(String::from("picture"), Value::from(picture));
            }
        }
    }
    if layer != duckforge::scene::USUAL_LAYER {
        line.insert(String::from("layer"), Value::from(layer));
    }
    line
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
            pivot: None,
            turn: 0.0,
            start_pivot: None,
            start_turn: 0.0,
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

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 0.01
    }

    #[test]
    fn a_turned_picture_is_clicked_where_it_is_drawn() {
        // A 20 x 10 sign standing on its feet at (100, 100), turned a
        // quarter clockwise: it now lies to the left of its feet.
        let rect = Rect::new(90.0, 90.0, 20.0, 10.0);
        let feet = Vec2::new(100.0, 100.0);
        let [top_left, ..] = corners(rect, feet, 90.0);
        assert!(close(top_left, Vec2::new(110.0, 90.0)), "{top_left:?}");
        assert!(covers(rect, feet, 0.0, Vec2::new(95.0, 95.0)));
        assert!(
            !covers(rect, feet, 90.0, Vec2::new(95.0, 95.0)),
            "it isn't there any more"
        );
        assert!(covers(rect, feet, 90.0, Vec2::new(105.0, 95.0)));
        let round = bounds(rect, feet, 90.0);
        assert!((round.w - 10.0).abs() < 0.01 && (round.h - 20.0).abs() < 0.01);
    }

    #[test]
    fn a_box_selects_what_it_touches() {
        let mut turned = drawn_object(3, "object", Some(Rect::new(200.0, 0.0, 40.0, 4.0)));
        turned.pivot = Some(Vec2::new(200.0, 4.0));
        turned.turn = 90.0; // now stands up, from (200, 4) to (204, 44)
        let mut painted = drawn_object(1 << 48, "coin", Some(Rect::new(0.0, 0.0, 8.0, 8.0)));
        painted.origin = Origin::Painted;
        let drawn = DrawnScene {
            file: String::new(),
            scale: 1.0,
            offset: Vec2::ZERO,
            as_it_starts: false,
            objects: vec![
                drawn_object(1, "object", Some(Rect::new(10.0, 10.0, 10.0, 10.0))),
                drawn_object(2, "object", Some(Rect::new(100.0, 100.0, 10.0, 10.0))),
                turned,
                painted,
                drawn_object(4, "tilemap", None),
            ],
        };
        assert_eq!(
            in_box(&drawn, Vec2::new(0.0, 0.0), Vec2::new(15.0, 15.0)),
            [1]
        );
        assert_eq!(
            in_box(&drawn, Vec2::new(120.0, 120.0), Vec2::new(5.0, 5.0)),
            [1, 2],
            "dragged either way"
        );
        assert_eq!(
            in_box(&drawn, Vec2::new(190.0, 30.0), Vec2::new(202.0, 40.0)),
            [3],
            "where the turned one is now"
        );
        assert!(in_box(&drawn, Vec2::new(210.0, 0.0), Vec2::new(239.0, 3.0)).is_empty());
    }

    #[test]
    fn turning_snaps_to_fifteen_degrees_and_stays_between_minus_and_plus_half_a_turn() {
        let pivot = Vec2::new(0.0, 0.0);
        let right = Vec2::new(10.0, 0.0);
        let down = Vec2::new(0.0, 10.0); // a quarter clockwise on the screen
        assert_eq!(turned(0.0, pivot, right, down, false), 90.0);
        assert_eq!(turned(-8.0, pivot, right, Vec2::new(10.0, 1.0), false), 0.0);
        assert_eq!(turned(-8.0, pivot, right, Vec2::new(10.0, 1.0), true), -2.0);
        assert_eq!(turned(170.0, pivot, right, down, false), -105.0);
        assert_eq!(tidy_turn(540.0), 180.0);
        assert_eq!(tidy_turn(-190.0), 170.0);
    }

    #[test]
    fn a_corner_dragged_sizes_round_the_feet() {
        // 16 x 16 standing on its feet at (100, 100).
        let shape = (
            Rect::new(92.0, 84.0, 16.0, 16.0),
            Vec2::new(100.0, 100.0),
            0.0,
        );
        let top_right = Vec2::new(108.0, 84.0);
        assert_eq!(
            resized(shape, top_right, Vec2::new(116.0, 68.0), false),
            (2.0, 2.0)
        );
        assert_eq!(
            resized(shape, top_right, Vec2::new(116.0, 84.0), false),
            (2.0, 1.0)
        );
        assert_eq!(
            resized(shape, top_right, Vec2::new(116.0, 80.0), true),
            (2.0, 2.0),
            "Shift keeps the shape, by the bigger change"
        );
        // A bottom corner is level with the feet: only the width.
        assert_eq!(
            resized(shape, Vec2::new(92.0, 100.0), Vec2::new(96.0, 120.0), false),
            (0.5, 1.0)
        );
        // Past the feet, it doesn't turn inside out.
        assert_eq!(
            resized(shape, top_right, Vec2::new(90.0, 120.0), false).0,
            0.0
        );
        // Turned a quarter, the same drag along its own width.
        let turned = (shape.0, shape.1, 90.0);
        let corner = corners(shape.0, shape.1, 90.0)[1];
        let further = shape.1 + (corner - shape.1) * 2.0;
        let (x, y) = resized(turned, corner, further, false);
        assert!((x - 2.0).abs() < 0.01 && (y - 2.0).abs() < 0.01, "{x} {y}");

        assert_eq!(tidy_scale(1.37, false), 1.25);
        assert_eq!(tidy_scale(0.0, false), 0.25);
        assert_eq!(tidy_scale(1.374, true), 1.37);
        assert_eq!(tidy_scale(500.0, true), 64.0);
    }

    #[test]
    fn a_move_changes_the_start_of_what_is_still_there_and_the_moment_of_what_moved() {
        let doc = doc();
        let mut duck = drawn_object(4, "player", Some(Rect::new(90.0, 190.0, 16.0, 18.0)));
        duck.now = set(r#"{"at": [98, 208]}"#);
        let mut coin = drawn_object(3, "coin", None);
        coin.gone = true;
        let mut made = drawn_object(1 << 32, "puff", Some(Rect::new(0.0, 0.0, 4.0, 4.0)));
        made.origin = Origin::Made;
        made.now = set(r#"{"name": "puff", "at": [5, 6]}"#);
        let drawn = DrawnScene {
            file: String::new(),
            scale: 1.0,
            offset: Vec2::ZERO,
            as_it_starts: false,
            objects: vec![
                drawn_object(1, "tilemap", None),
                drawn_object(2, "object", Some(Rect::new(112.0, 190.0, 16.0, 18.0))),
                duck,
                made,
                coin,
            ],
        };
        let all = moving(&doc, &drawn, &[1, 2, 3, 4, 1 << 32]);
        assert_eq!(all.starts, [(2, Vec2::new(120.0, 208.0))]);
        assert_eq!(
            all.now,
            [(4, Vec2::new(98.0, 208.0)), (1 << 32, Vec2::new(5.0, 6.0))]
        );
    }

    #[test]
    fn copied_lines_paste_back_from_the_clipboard_or_from_a_scene_file() {
        let doc = doc();
        let file = doc.file.as_ref().unwrap();
        let lines: Vec<_> = file.objects[1..3].iter().map(line_without_id).collect();
        let text = clipboard_text(&lines);
        assert_eq!(
            text,
            "{\"name\": \"sign\", \"at\": [120, 208], \"picture\": \"assets/sign.png\"},\n{\"name\": \"coin\", \"kind\": \"coin\", \"at\": [200, 180]}"
        );
        assert_eq!(lines_from_text(&text), Some(lines.clone()));
        assert_eq!(lines_from_text(&format!("[{text}]")), Some(lines.clone()));
        assert_eq!(
            lines_from_text(POND).map(|lines| lines.len()),
            Some(4),
            "a whole file"
        );
        assert_eq!(lines_from_text("hello"), None);
        assert_eq!(
            lines_from_text(r#"{"a": 1}"#),
            None,
            "not an object of a scene"
        );
        assert_eq!(lines_from_text(""), None);
    }

    #[test]
    fn a_new_object_has_what_its_kind_needs() {
        let at = Vec2::new(96.0, 208.0);
        let picture = new_line("picture", at, Some("assets/crate.png"), "world");
        assert_eq!(
            duckforge::scene::line_text(&picture),
            r#"{"name": "crate", "at": [96, 208], "picture": "assets/crate.png"}"#
        );
        let area = new_line("area", at, None, "world");
        assert_eq!(
            duckforge::scene::line_text(&area),
            r#"{"name": "area", "kind": "area", "at": [96, 208], "box": [0, 0, 32, 32]}"#
        );
        let map = new_line("tilemap", at, Some("level2.txt"), "world");
        assert_eq!(
            duckforge::scene::line_text(&map),
            r#"{"name": "level2", "kind": "tilemap", "level": "level2.txt"}"#
        );
        let back = new_line("background", at, Some("assets/sky.png"), "far");
        assert_eq!(
            duckforge::scene::line_text(&back),
            r#"{"name": "sky", "kind": "background", "at": [0, 208], "picture": "assets/sky.png", "layer": "far", "parallax": 0.5}"#
        );
        assert!(plain_kind("text").unwrap().not_yet.is_some());
        assert!(plain_kind("area").unwrap().not_yet.is_none());
        // What the engine reads back is what was meant.
        let mut file = doc().file.unwrap();
        let id = file.add(&area, None).unwrap();
        let area = file.object(id).unwrap();
        assert_eq!(area.hitbox, Some(Rect::new(0.0, 0.0, 32.0, 32.0)));
        assert_eq!(area.layer, "world");
    }
}

//! What a scene tells Duck Forge Studio about a frozen picture: where each
//! object was drawn, where it starts, and what has changed since the
//! start. Only games built for the Studio (the `studio` feature) have
//! this; exported games don't.

use serde_json::{Map, Value, json};

use crate::math::{Rect, Vec2, vec2};

/// Where an object comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A line of the scene file.
    File,
    /// Made by the game's code while playing.
    Made,
    /// A thing painted on a tile map.
    Painted,
}

impl Origin {
    fn word(self) -> &'static str {
        match self {
            Origin::File => "file",
            Origin::Made => "made",
            Origin::Painted => "painted",
        }
    }

    fn from_word(word: &str) -> Origin {
        match word {
            "made" => Origin::Made,
            "painted" => Origin::Painted,
            _ => Origin::File,
        }
    }
}

/// One object of a drawn scene.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawnObject {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub layer: String,
    pub origin: Origin,
    /// Its layer doesn't move with the camera.
    pub stays_on_screen: bool,
    /// Where its picture (or its box, if it has no picture) is on the
    /// canvas, in canvas pixels. `None` for what has no place to click: a
    /// tile map, a background, an object with no size, one that's gone.
    pub rect: Option<Rect>,
    /// Where it would be drawn as the file has it, if that's somewhere
    /// else (it has moved since the start), or if it's gone.
    pub start_rect: Option<Rect>,
    /// The point of the canvas it turns round (its `at`), and how far it
    /// is turned, in degrees clockwise: `rect` is its picture before it's
    /// turned. `None` with no `rect`.
    pub pivot: Option<Vec2>,
    pub turn: f32,
    /// The same for `start_rect`.
    pub start_pivot: Option<Vec2>,
    pub start_turn: f32,
    /// The file has it, and the game has removed it (a coin that was
    /// taken).
    pub gone: bool,
    /// The keys of its line whose value now isn't the file's, with what
    /// they are now. Empty if it's as the file has it.
    pub now: Map<String, Value>,
}

/// A scene as it was drawn on one picture.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawnScene {
    /// The scene's file, as the engine names files.
    pub file: String,
    /// How a place in the scene becomes a place on the canvas, for layers
    /// that move with the camera: `canvas = scene * scale + offset`.
    pub scale: f32,
    pub offset: Vec2,
    /// It was drawn as the file has it, not as it is now.
    pub as_it_starts: bool,
    /// Its objects in drawing order (the last is on top), then the ones
    /// that are gone.
    pub objects: Vec<DrawnObject>,
}

fn rect_json(rect: Option<Rect>) -> Value {
    match rect {
        Some(r) => json!([r.x, r.y, r.w, r.h]),
        None => Value::Null,
    }
}

fn point_json(point: Option<Vec2>) -> Value {
    match point {
        Some(p) => json!([p.x, p.y]),
        None => Value::Null,
    }
}

fn point_of(value: &Value) -> Option<Vec2> {
    let pair = value.as_array()?;
    Some(vec2(
        pair.first()?.as_f64()? as f32,
        pair.get(1)?.as_f64()? as f32,
    ))
}

fn rect_of(value: &Value) -> Option<Rect> {
    let numbers: Vec<f32> = value
        .as_array()?
        .iter()
        .filter_map(|n| n.as_f64().map(|n| n as f32))
        .collect();
    match numbers.as_slice() {
        &[x, y, w, h] => Some(Rect::new(x, y, w, h)),
        _ => None,
    }
}

impl DrawnScene {
    /// As JSON, for the link.
    pub fn to_json(&self) -> Value {
        json!({
            "file": self.file,
            "scale": self.scale,
            "offset": [self.offset.x, self.offset.y],
            "as_it_starts": self.as_it_starts,
            "objects": self.objects.iter().map(|object| json!({
                "id": object.id,
                "name": object.name,
                "kind": object.kind,
                "layer": object.layer,
                "origin": object.origin.word(),
                "stays": object.stays_on_screen,
                "rect": rect_json(object.rect),
                "start_rect": rect_json(object.start_rect),
                "pivot": point_json(object.pivot),
                "turn": object.turn,
                "start_pivot": point_json(object.start_pivot),
                "start_turn": object.start_turn,
                "gone": object.gone,
                "now": object.now,
            })).collect::<Vec<_>>(),
        })
    }

    /// From the link's JSON.
    pub fn from_json(value: &Value) -> DrawnScene {
        let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
        DrawnScene {
            file: text(&value["file"]),
            scale: value["scale"].as_f64().unwrap_or(1.0) as f32,
            offset: vec2(
                value["offset"][0].as_f64().unwrap_or(0.0) as f32,
                value["offset"][1].as_f64().unwrap_or(0.0) as f32,
            ),
            as_it_starts: value["as_it_starts"].as_bool().unwrap_or(false),
            objects: value["objects"]
                .as_array()
                .map(|objects| {
                    objects
                        .iter()
                        .map(|object| DrawnObject {
                            id: object["id"].as_u64().unwrap_or(0),
                            name: text(&object["name"]),
                            kind: text(&object["kind"]),
                            layer: text(&object["layer"]),
                            origin: Origin::from_word(object["origin"].as_str().unwrap_or("")),
                            stays_on_screen: object["stays"].as_bool().unwrap_or(false),
                            rect: rect_of(&object["rect"]),
                            start_rect: rect_of(&object["start_rect"]),
                            pivot: point_of(&object["pivot"]),
                            turn: object["turn"].as_f64().unwrap_or(0.0) as f32,
                            start_pivot: point_of(&object["start_pivot"]),
                            start_turn: object["start_turn"].as_f64().unwrap_or(0.0) as f32,
                            gone: object["gone"].as_bool().unwrap_or(false),
                            now: object["now"].as_object().cloned().unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The object with this id.
    pub fn object(&self, id: u64) -> Option<&DrawnObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    /// A place on the canvas as a place in the scene (for an object on a
    /// layer that moves with the camera).
    pub fn to_scene(&self, canvas: Vec2) -> Vec2 {
        (canvas - self.offset) * (1.0 / self.scale)
    }
}

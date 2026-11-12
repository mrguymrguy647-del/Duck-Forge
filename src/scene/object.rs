//! One object of a scene: what its line in the scene file says.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value};

use crate::color::Color;
use crate::math::{Rect, Vec2, vec2};

/// Which object: a number that stays the same for as long as the object is
/// in its scene, and is never used again there.
///
/// Objects from the scene file have the ids the file gives them (small
/// numbers). Objects made while the game plays, and things painted on a
/// tile map, get bigger numbers of their own, so the three never collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub(crate) u64);

/// Ids from the file are below this; ids made while playing start here.
pub(crate) const FIRST_MADE_ID: u64 = 1 << 32;
/// Ids of things painted on a tile map start here.
pub(crate) const FIRST_PAINTED_ID: u64 = 1 << 48;

impl Id {
    /// The number, as the scene file has it.
    pub fn number(self) -> u64 {
        self.0
    }

    /// Is this an object of the scene file (and not one made while
    /// playing, or painted on a tile map)?
    pub fn is_from_the_file(self) -> bool {
        self.0 != 0 && self.0 < FIRST_MADE_ID
    }

    /// Is this a thing painted on a tile map (a letter of its level)?
    pub fn is_painted(self) -> bool {
        self.0 >= FIRST_PAINTED_ID
    }

    /// The id of the thing painted on square (`column`, `row`) of the tile
    /// map `map`: the same every time, so a replay and a reload find it.
    pub(crate) fn painted(map: Id, column: usize, row: usize) -> Id {
        Id(FIRST_PAINTED_ID
            | ((map.0 & 0xffff) << 32)
            | ((row as u64 & 0xffff) << 16)
            | (column as u64 & 0xffff))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The point of an object that sits on its position (`at`), and that it
/// turns round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Middle,
    Right,
    BottomLeft,
    /// The bottom middle: where a character stands.
    Feet,
    BottomRight,
}

impl Anchor {
    const ALL: [(Anchor, &'static str); 9] = [
        (Anchor::TopLeft, "top_left"),
        (Anchor::Top, "top"),
        (Anchor::TopRight, "top_right"),
        (Anchor::Left, "left"),
        (Anchor::Middle, "middle"),
        (Anchor::Right, "right"),
        (Anchor::BottomLeft, "bottom_left"),
        (Anchor::Feet, "feet"),
        (Anchor::BottomRight, "bottom_right"),
    ];

    /// Its word in a scene file: `"feet"`, `"top_left"`...
    pub fn word(self) -> &'static str {
        Anchor::ALL
            .iter()
            .find(|(anchor, _)| *anchor == self)
            .map_or("feet", |(_, word)| word)
    }

    /// The anchor a scene file's word means.
    pub fn from_word(word: &str) -> Option<Anchor> {
        Anchor::ALL
            .iter()
            .find(|(_, w)| *w == word)
            .map(|(anchor, _)| *anchor)
    }

    /// Every word, for a message: `top_left, top, ...`.
    pub(crate) fn words() -> String {
        let words: Vec<&str> = Anchor::ALL.iter().map(|(_, word)| *word).collect();
        words.join(", ")
    }

    /// The point of a `width` x `height` picture this anchor is, from its
    /// top-left corner.
    pub fn point(self, width: f32, height: f32) -> Vec2 {
        let x = match self {
            Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => 0.0,
            Anchor::Top | Anchor::Middle | Anchor::Feet => width / 2.0,
            Anchor::TopRight | Anchor::Right | Anchor::BottomRight => width,
        };
        let y = match self {
            Anchor::TopLeft | Anchor::Top | Anchor::TopRight => 0.0,
            Anchor::Left | Anchor::Middle | Anchor::Right => height / 2.0,
            Anchor::BottomLeft | Anchor::Feet | Anchor::BottomRight => height,
        };
        vec2(x, y)
    }

    /// The anchor a kind has when its object doesn't name one. Pictures
    /// and everything that moves stand on their feet; what's laid out like
    /// a page (words, buttons, areas, tile maps, backgrounds) is placed by
    /// its top-left corner; what has no shape sits on its middle.
    pub fn of_kind(kind: &str) -> Anchor {
        match kind {
            "text" | "button" | "area" | "tilemap" | "background" => Anchor::TopLeft,
            "sound" | "camera" | "camera_follow" | "spawner" => Anchor::Middle,
            _ => Anchor::Feet,
        }
    }
}

/// The kind an object has when its line doesn't name one: it's drawn, and
/// does nothing.
pub const PLAIN_KIND: &str = "picture";
/// The layer an object is on when its line doesn't name one.
pub const USUAL_LAYER: &str = "world";

/// The keys every object can have. A kind's own settings can't use them.
pub(crate) const COMMON_KEYS: [&str; 17] = [
    "id",
    "name",
    "kind",
    "like",
    "at",
    "turn",
    "scale",
    "flip",
    "anchor",
    "picture",
    "animation",
    "layer",
    "shown",
    "tint",
    "opacity",
    "box",
    "solid",
];

/// Keys kept for later: files made now can have them, and nothing reads
/// them yet. `parent` is for objects inside objects, `kinds` for several
/// behaviours on one object.
pub(crate) const KEPT_FOR_LATER: [&str; 2] = ["parent", "kinds"];

/// One object of a scene. Every key of its line in the scene file is a
/// field here; the kind's own settings (`speed`, `goes_to`...) are read
/// with [`number`](Object::number), [`text`](Object::text) and
/// [`flag`](Object::flag).
///
/// ```
/// # use duckforge::prelude::*;
/// let coin = Object::new("coin").at(vec2(120.0, 96.0)).with_picture("assets/coin.png");
/// assert_eq!(coin.name, "coin");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub(crate) id: Id,
    /// What you and the code call it. Several objects can share a name.
    pub name: String,
    /// What it does: a ready-made kind, or one of the game's own.
    pub kind: String,
    /// Where its anchor is, in the scene's pixels (y downwards).
    pub at: Vec2,
    /// Degrees clockwise.
    pub turn: f32,
    /// How many times bigger, sideways and up and down.
    pub scale: Vec2,
    /// Mirrored left to right.
    pub flip: bool,
    /// The point of it that sits on `at`. `None`: its kind's usual one
    /// (see [`Anchor::of_kind`]), or the pivot of its Duck Quill sheet.
    pub anchor: Option<Anchor>,
    /// A `.png`, or a Duck Quill sheet (`.json`), from the game's folder.
    pub picture: Option<String>,
    /// Which animation of the sheet. `None`: `idle` if it has one, or
    /// else its first.
    pub animation: Option<String>,
    /// One of the scene's layers.
    pub layer: String,
    /// Drawn or not. A hidden object is still there, and still touches.
    pub shown: bool,
    /// As in `Blit`.
    pub tint: Color,
    pub opacity: f32,
    /// Its collision box inside its picture, in the picture's own pixels
    /// from its top-left corner. `None`: the whole picture.
    pub hitbox: Option<Rect>,
    /// Other objects can't pass through it.
    pub solid: bool,
    /// A saved thing it starts from (`things/<name>.json`).
    pub like: Option<String>,
    /// Every other key: the kind's own settings, and keys the engine
    /// doesn't know (kept, so they survive a save).
    pub(crate) settings: BTreeMap<String, Value>,
}

impl Object {
    /// A new object of a kind, named after it, with everything else as a
    /// line that says nothing more would have it. It gets its id when it's
    /// added to a scene.
    pub fn new(kind: &str) -> Object {
        Object {
            id: Id(0),
            name: kind.to_string(),
            kind: kind.to_string(),
            at: Vec2::ZERO,
            turn: 0.0,
            scale: vec2(1.0, 1.0),
            flip: false,
            anchor: None,
            picture: None,
            animation: None,
            layer: USUAL_LAYER.to_string(),
            shown: true,
            tint: Color::WHITE,
            opacity: 1.0,
            hitbox: None,
            solid: false,
            like: None,
            settings: BTreeMap::new(),
        }
    }

    /// The same object with another name.
    pub fn named(mut self, name: &str) -> Object {
        self.name = name.to_string();
        self
    }

    /// The same object somewhere else.
    pub fn at(mut self, at: Vec2) -> Object {
        self.at = at;
        self
    }

    /// The same object with a picture (a path from the game's folder).
    pub fn with_picture(mut self, picture: &str) -> Object {
        self.picture = Some(picture.to_string());
        self
    }

    /// The same object on another layer.
    pub fn on_layer(mut self, layer: &str) -> Object {
        self.layer = layer.to_string();
        self
    }

    /// Its id: [`Id`]. 0 until it's in a scene.
    pub fn id(&self) -> Id {
        self.id
    }

    /// The point of it that sits on `at`, as a word: the one it names, or
    /// its kind's usual one.
    pub fn anchor_or_usual(&self) -> Anchor {
        self.anchor.unwrap_or_else(|| Anchor::of_kind(&self.kind))
    }

    /// One of its own settings, as a number.
    pub fn number(&self, name: &str) -> Option<f32> {
        self.settings
            .get(name)
            .and_then(Value::as_f64)
            .map(|n| n as f32)
    }

    /// One of its own settings, as words.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.settings.get(name).and_then(Value::as_str)
    }

    /// One of its own settings, as yes or no.
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.settings.get(name).and_then(Value::as_bool)
    }

    /// One of its own settings, as a point: `[x, y]` in the file.
    pub fn point(&self, name: &str) -> Option<Vec2> {
        point_of(self.settings.get(name)?)
    }

    /// Does its line have this setting?
    pub fn has(&self, name: &str) -> bool {
        self.settings.contains_key(name)
    }

    /// The names of its own settings, in alphabetical order.
    pub fn setting_names(&self) -> impl Iterator<Item = &str> {
        self.settings.keys().map(String::as_str)
    }

    /// Gives one of its own settings a number.
    #[track_caller]
    pub fn set_number(&mut self, name: &str, value: f32) {
        self.set(name, number(value));
    }

    /// Gives one of its own settings words.
    #[track_caller]
    pub fn set_text(&mut self, name: &str, value: &str) {
        self.set(name, Value::from(value));
    }

    /// Gives one of its own settings yes or no.
    #[track_caller]
    pub fn set_flag(&mut self, name: &str, value: bool) {
        self.set(name, Value::from(value));
    }

    /// Takes one of its own settings away.
    pub fn unset(&mut self, name: &str) {
        self.settings.remove(name);
    }

    #[track_caller]
    fn set(&mut self, name: &str, value: Value) {
        assert!(
            !COMMON_KEYS.contains(&name),
            "{name:?} is a key every object has, so it's a field ({}), not a setting",
            if name == "box" { "hitbox" } else { name }
        );
        self.settings.insert(name.to_string(), value);
    }

    /// One of its own settings as the file has it, whatever it is.
    pub fn setting(&self, name: &str) -> Option<&Value> {
        self.settings.get(name)
    }

    /// Its line, as the scene file has it. With `every_key`, the keys that
    /// say nothing new (a turn of 0, shown) are written too.
    pub fn to_json(&self, every_key: bool) -> Map<String, Value> {
        let usual = Object::new(&self.kind);
        let mut line = Map::new();
        let mut put = |key: &str, value: Value, differs: bool| {
            if every_key || differs {
                line.insert(key.to_string(), value);
            }
        };
        put("id", Value::from(self.id.0), self.id.0 != 0);
        put("name", Value::from(self.name.clone()), true);
        put(
            "kind",
            Value::from(self.kind.clone()),
            self.kind != PLAIN_KIND,
        );
        put(
            "like",
            self.like.clone().map_or(Value::Null, Value::from),
            self.like.is_some(),
        );
        put("at", pair(self.at), self.at != usual.at);
        put("turn", number(self.turn), self.turn != usual.turn);
        put(
            "scale",
            if self.scale.x == self.scale.y {
                number(self.scale.x)
            } else {
                pair(self.scale)
            },
            self.scale != usual.scale,
        );
        put("flip", Value::from(self.flip), self.flip);
        put(
            "anchor",
            self.anchor
                .map_or(Value::Null, |anchor| Value::from(anchor.word())),
            self.anchor.is_some(),
        );
        put(
            "picture",
            self.picture.clone().map_or(Value::Null, Value::from),
            self.picture.is_some(),
        );
        put(
            "animation",
            self.animation.clone().map_or(Value::Null, Value::from),
            self.animation.is_some(),
        );
        put(
            "layer",
            Value::from(self.layer.clone()),
            self.layer != usual.layer,
        );
        put("shown", Value::from(self.shown), !self.shown);
        put(
            "tint",
            Value::from(colour_word(self.tint)),
            self.tint != usual.tint,
        );
        put(
            "opacity",
            number(self.opacity),
            self.opacity != usual.opacity,
        );
        put(
            "box",
            self.hitbox.map_or(Value::Null, |r| {
                Value::Array(vec![number(r.x), number(r.y), number(r.w), number(r.h)])
            }),
            self.hitbox.is_some(),
        );
        put("solid", Value::from(self.solid), self.solid);
        for (name, value) in &self.settings {
            line.insert(name.clone(), value.clone());
        }
        line
    }

    /// An object from its line. What's wrong with a key is said in
    /// `wrong`, in plain words, and that key is as if it weren't there.
    pub(crate) fn from_json(line: &Map<String, Value>, wrong: &mut Vec<String>) -> Object {
        let kind = match line.get("kind") {
            None | Some(Value::Null) => PLAIN_KIND.to_string(),
            Some(Value::String(kind)) if !kind.is_empty() => kind.clone(),
            Some(other) => {
                wrong.push(format!(
                    "\"kind\" should be a name in quotes, like \"coin\"; it's {other}"
                ));
                PLAIN_KIND.to_string()
            }
        };
        let mut object = Object::new(&kind);
        for (key, value) in line {
            if value.is_null() && COMMON_KEYS.contains(&key.as_str()) {
                continue; // "picture": null is no picture
            }
            let mut bad = |should: &str| {
                wrong.push(format!("{key:?} should be {should}; it's {value}"));
            };
            match key.as_str() {
                "kind" => {}
                "id" => match value.as_u64() {
                    Some(id) if id != 0 && id < FIRST_MADE_ID => object.id = Id(id),
                    _ => bad("a whole number from 1 up, like 7"),
                },
                "name" => match value.as_str() {
                    Some(name) => object.name = name.to_string(),
                    None => bad("a name in quotes, like \"duck\""),
                },
                "like" => match value.as_str() {
                    Some(like) => object.like = Some(like.to_string()),
                    None => bad("a saved thing's name in quotes"),
                },
                "at" => match point_of(value) {
                    Some(at) => object.at = at,
                    None => bad("two numbers, like [40, 208]"),
                },
                "turn" => match value.as_f64() {
                    Some(turn) => object.turn = turn as f32,
                    None => bad("a number of degrees, like 90"),
                },
                "scale" => match (value.as_f64(), point_of(value)) {
                    (Some(scale), _) if scale > 0.0 => {
                        object.scale = vec2(scale as f32, scale as f32)
                    }
                    (None, Some(scale)) if scale.x > 0.0 && scale.y > 0.0 => object.scale = scale,
                    _ => bad("a number above 0, like 2, or two, like [2, 1]"),
                },
                "flip" => match value.as_bool() {
                    Some(flip) => object.flip = flip,
                    None => bad("true or false"),
                },
                "anchor" => match value.as_str().and_then(Anchor::from_word) {
                    Some(anchor) => object.anchor = Some(anchor),
                    None => bad(&format!("one of {}", Anchor::words())),
                },
                "picture" => match value.as_str() {
                    Some(picture) => object.picture = Some(picture.to_string()),
                    None => bad("a file's name in quotes, like \"assets/duck.png\""),
                },
                "animation" => match value.as_str() {
                    Some(animation) => object.animation = Some(animation.to_string()),
                    None => bad("an animation's name in quotes"),
                },
                "layer" => match value.as_str() {
                    Some(layer) => object.layer = layer.to_string(),
                    None => bad("a layer's name in quotes"),
                },
                "shown" => match value.as_bool() {
                    Some(shown) => object.shown = shown,
                    None => bad("true or false"),
                },
                "tint" => match value.as_str().and_then(colour_of) {
                    Some(tint) => object.tint = tint,
                    None => bad("a colour like \"#ffcc2e\""),
                },
                "opacity" => match value.as_f64() {
                    Some(opacity) => object.opacity = opacity as f32,
                    None => bad("a number from 0 to 1"),
                },
                "box" => match numbers_of(value).as_deref() {
                    Some(&[x, y, w, h]) if w >= 0.0 && h >= 0.0 => {
                        object.hitbox = Some(Rect::new(x, y, w, h));
                    }
                    _ => bad("four numbers: left, top, width, height, like [2, 4, 12, 9]"),
                },
                "solid" => match value.as_bool() {
                    Some(solid) => object.solid = solid,
                    None => bad("true or false"),
                },
                _ => {
                    if key == "size" {
                        wrong.push(String::from(
                            "an object's \"size\" is called \"scale\" (how many times bigger); \
                             \"size\" is kept as it is and does nothing",
                        ));
                    } else if KEPT_FOR_LATER.contains(&key.as_str()) {
                        wrong.push(format!(
                            "{key:?} is kept for a later Duck Forge and does nothing yet"
                        ));
                    }
                    object.settings.insert(key.clone(), value.clone());
                }
            }
        }
        if !line.contains_key("name") {
            object.name = kind;
        }
        object
    }
}

/// A number as a scene file writes it: `40`, not `40.0`, and `0.2`, not
/// `0.20000000298`.
pub(crate) fn number(value: f32) -> Value {
    if !value.is_finite() {
        return Value::from(0);
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return Value::from(value as i64);
    }
    // The shortest text that reads back as the same f32.
    format!("{value}")
        .parse::<f64>()
        .map_or(Value::from(0), Value::from)
}

/// `[x, y]`.
pub(crate) fn pair(point: Vec2) -> Value {
    Value::Array(vec![number(point.x), number(point.y)])
}

/// The numbers of a list, if that's what it is.
pub(crate) fn numbers_of(value: &Value) -> Option<Vec<f32>> {
    value
        .as_array()?
        .iter()
        .map(|n| n.as_f64().map(|n| n as f32))
        .collect()
}

/// `[x, y]` as a point.
pub(crate) fn point_of(value: &Value) -> Option<Vec2> {
    match numbers_of(value)?.as_slice() {
        &[x, y] => Some(vec2(x, y)),
        _ => None,
    }
}

/// `"#rrggbb"` as a colour.
pub(crate) fn colour_of(word: &str) -> Option<Color> {
    let digits = word.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(Color::from_hex)
}

/// A colour as `"#rrggbb"`.
pub(crate) fn colour_word(colour: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn a_line_becomes_an_object_and_back() {
        let mut wrong = Vec::new();
        let text = json!({"id": 7, "name": "snail", "kind": "patrol", "at": [496, 208],
            "scale": [2, 1], "picture": "assets/snail.png", "speed": 30, "walks": 48.5});
        let snail = Object::from_json(&line(text.clone()), &mut wrong);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(snail.id(), Id(7));
        assert_eq!(
            (snail.name.as_str(), snail.kind.as_str()),
            ("snail", "patrol")
        );
        assert_eq!(snail.at, vec2(496.0, 208.0));
        assert_eq!(snail.scale, vec2(2.0, 1.0));
        assert_eq!(snail.number("speed"), Some(30.0));
        assert_eq!(snail.number("walks"), Some(48.5));
        assert_eq!(snail.layer, "world");
        // Only what differs from the usual is written, and it reads the same.
        assert_eq!(Value::Object(snail.to_json(false)), text);
        let again = Object::from_json(&snail.to_json(true), &mut wrong);
        assert_eq!(again, snail);
    }

    #[test]
    fn a_plain_object_needs_only_a_name() {
        let mut wrong = Vec::new();
        let sign = Object::from_json(&line(json!({"name": "sign"})), &mut wrong);
        assert_eq!(sign.kind, "picture");
        assert_eq!(sign.anchor_or_usual(), Anchor::Feet);
        assert_eq!(
            Value::Object(sign.to_json(false)),
            json!({"name": "sign"}),
            "nothing it didn't say"
        );
        // With no name, it's called after its kind.
        let coin = Object::from_json(&line(json!({"kind": "coin"})), &mut wrong);
        assert_eq!(coin.name, "coin");
        assert!(wrong.is_empty());
    }

    #[test]
    fn a_wrong_key_is_said_in_plain_words_and_the_rest_is_read() {
        let mut wrong = Vec::new();
        let object = Object::from_json(
            &line(
                json!({"name": "duck", "at": "here", "scale": 0, "anchor": "toes",
                "tint": "red", "size": 2, "parent": "lift", "speed": 30}),
            ),
            &mut wrong,
        );
        assert_eq!(object.at, Vec2::ZERO);
        assert_eq!(object.scale, vec2(1.0, 1.0));
        assert_eq!(object.number("speed"), Some(30.0));
        let said = wrong.join("\n");
        assert!(
            said.contains("\"at\" should be two numbers, like [40, 208]"),
            "{said}"
        );
        assert!(
            said.contains("\"scale\" should be a number above 0"),
            "{said}"
        );
        assert!(said.contains("one of top_left, top,"), "{said}");
        assert!(said.contains("a colour like \"#ffcc2e\""), "{said}");
        assert!(said.contains("is called \"scale\""), "{said}");
        assert!(
            said.contains("\"parent\" is kept for a later Duck Forge"),
            "{said}"
        );
        // Kept, so a save doesn't lose them.
        assert!(object.has("size") && object.has("parent"));
    }

    #[test]
    fn numbers_are_written_as_a_person_would() {
        assert_eq!(number(40.0).to_string(), "40");
        assert_eq!(number(0.2).to_string(), "0.2");
        assert_eq!(number(-1.5).to_string(), "-1.5");
        assert_eq!(colour_word(colour_of("#6ab4f0").unwrap()), "#6ab4f0");
        assert_eq!(colour_of("6ab4f0"), None);
    }

    #[test]
    fn anchors_are_points_of_the_picture() {
        assert_eq!(Anchor::Feet.point(16.0, 16.0), vec2(8.0, 16.0));
        assert_eq!(Anchor::TopLeft.point(16.0, 16.0), Vec2::ZERO);
        assert_eq!(Anchor::Middle.point(16.0, 10.0), vec2(8.0, 5.0));
        assert_eq!(Anchor::of_kind("text"), Anchor::TopLeft);
        assert_eq!(Anchor::of_kind("coin"), Anchor::Feet);
        assert_eq!(
            Anchor::from_word(Anchor::BottomRight.word()),
            Some(Anchor::BottomRight)
        );
    }

    #[test]
    fn painted_ids_are_their_square() {
        let id = Id::painted(Id(3), 34, 6);
        assert!(id.is_painted() && !id.is_from_the_file());
        assert_eq!(id, Id::painted(Id(3), 34, 6));
        assert_ne!(id, Id::painted(Id(3), 6, 34));
        assert_ne!(id, Id::painted(Id(4), 34, 6));
        assert!(Id(7).is_from_the_file());
    }
}

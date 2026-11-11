//! Input scripts: a list of "on frame N, press this key" events, saved as
//! JSON. They let a recorded playthrough be replayed exactly, by a person
//! in a window or by a program with no screen.
//!
//! The format is described for humans in `docs/input-format.md`:
//!
//! ```json
//! {
//!   "duckforge_inputs": 1,
//!   "seed": 7,
//!   "frames": 300,
//!   "expect": {"score": 20, "state": "Playing"},
//!   "events": [
//!     {"frame": 1, "press": "Space"},
//!     {"frame": 2, "release": "Space"},
//!     {"frame": 30, "mouse": [160, 120]},
//!     {"frame": 30, "mouse_motion": [4, -1]},
//!     {"frame": 31, "mouse_press": "Left"},
//!     {"frame": 33, "mouse_release": "Left"},
//!     {"frame": 40, "scroll": -1}
//!   ]
//! }
//! ```
//!
//! A run where files or tune values changed while it played (in Duck
//! Forge Studio, say) also has the files the game read, as they were on
//! frame 1, and each change on the frame it happened:
//!
//! ```json
//! {
//!   "duckforge_inputs": 1,
//!   "files": {"examples/mygame/level.txt": "p..\n###\n"},
//!   "events": [
//!     {"frame": 413, "file": "examples/mygame/level.txt", "text": "p.=\n###\n"},
//!     {"frame": 413, "tune": {"jump_speed": 380}},
//!     {"frame": 413, "press": "Space"}
//!   ]
//! }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::engine::{Config, Timestep};
use crate::input::{Key, MouseButton};
use crate::math::{Vec2, vec2};
use crate::report::ReportValue;

/// The format version written in `"duckforge_inputs"`.
const VERSION: u64 = 1;

/// One thing a player can do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputAction {
    /// Start holding a key.
    Press(Key),
    /// Let go of a key.
    Release(Key),
    /// Move the mouse pointer to this point, in canvas pixels.
    MouseMove(Vec2),
    /// Move the mouse by this much, in canvas pixels, without saying where
    /// the pointer is. This is what a locked pointer
    /// ([`Context::lock_pointer`](crate::engine::Context::lock_pointer))
    /// records: the game sees it in `mouse_delta()`.
    MouseMotion(Vec2),
    /// Start holding a mouse button.
    MousePress(MouseButton),
    /// Let go of a mouse button.
    MouseRelease(MouseButton),
    /// Roll the scroll wheel: positive is up (away from you), negative is down.
    Scroll(f32),
}

/// An action, and the frame that sees it.
///
/// Frames count from 1, like [`Context::frame`](crate::engine::Context::frame):
/// an event on frame 1 is seen by the very first update.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputEvent {
    pub frame: u64,
    pub action: InputAction,
}

/// A change made while a game ran, applied just before the update of its
/// frame (before that frame's inputs).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A text file the game read has new text: `{"frame": 413, "file":
    /// "examples/mygame/level.txt", "text": "..."}`. The game's
    /// [`reload`](crate::engine::Game::reload) hook is called.
    File { path: String, text: String },
    /// New `ctx.tune` values: `{"frame": 413, "tune": {"jump_speed": 380}}`.
    Tune(BTreeMap<String, f32>),
}

/// A [`Change`], and the frame that sees it first.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeEvent {
    pub frame: u64,
    pub change: Change,
}

/// A whole recorded (or written) playthrough.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputScript {
    /// The random seed the recording was made with. A replay uses it, so
    /// the game's random numbers come out the same as when it was recorded.
    pub seed: Option<u32>,
    /// How many updates per second the game ran at when this was recorded.
    pub updates_per_second: Option<u32>,
    /// How long the recording lasted, in frames. A headless replay runs this
    /// many frames unless told otherwise.
    pub frames: Option<u64>,
    /// The events, in frame order.
    pub events: Vec<InputEvent>,
    /// What the playthrough is expected to show: the game's reported
    /// values at the end, as they were when it was recorded (or accepted
    /// with `--accept`). A replay that ends differently is reported as
    /// "playthrough changed".
    pub expect: Option<BTreeMap<String, ReportValue>>,
    /// The `ctx.tune` values the recording was made with. A replay with
    /// other values says which changed (a warning if it still ends as
    /// expected, the likely cause if it doesn't).
    pub tune: Option<BTreeMap<String, f32>>,
    /// The text files the game read (its level, say), as they were on
    /// frame 1. A replay gives the game these, not what's on disk now.
    /// Empty in a script written before files were kept.
    pub files: BTreeMap<String, String>,
    /// Files and tune values changed while it played, in frame order. In
    /// the JSON they're in the `"events"` list with the inputs.
    pub changes: Vec<ChangeEvent>,
}

impl InputScript {
    /// Reads a script from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> Result<InputScript, String> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .map_err(|err| format!("could not read {}: {err}", path.display()))?;
        InputScript::parse(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The same as [`load`](InputScript::load): reads a script from a JSON
    /// file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<InputScript, String> {
        InputScript::load(path)
    }

    /// The same as [`parse`](InputScript::parse): reads a script from JSON
    /// text.
    pub fn from_json(text: &str) -> Result<InputScript, String> {
        InputScript::parse(text)
    }

    /// Reads a script from JSON text. The error message says what's wrong
    /// and where.
    pub fn parse(text: &str) -> Result<InputScript, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|err| format!("not valid JSON: {err}"))?;
        let Value::Object(top) = value else {
            return Err(String::from(
                "an input script is a JSON object like {\"events\": [...]}",
            ));
        };
        check_keys(
            &top,
            &[
                "duckforge_inputs",
                "seed",
                "updates_per_second",
                "frames",
                "expect",
                "tune",
                "files",
                "events",
            ],
            "the script",
        )?;

        if let Some(version) = top.get("duckforge_inputs") {
            if version.as_u64() != Some(VERSION) {
                return Err(format!(
                    "\"duckforge_inputs\" is {version}, but this engine reads version {VERSION}"
                ));
            }
        }
        let seed = match top.get("seed") {
            None | Some(Value::Null) => None,
            Some(value) => Some(whole_number(value, "\"seed\"", u32::MAX.into())? as u32),
        };
        let updates_per_second = match top.get("updates_per_second") {
            None | Some(Value::Null) => None,
            Some(value) => Some(whole_number(value, "\"updates_per_second\"", 100_000)? as u32),
        };
        let frames = match top.get("frames") {
            None | Some(Value::Null) => None,
            Some(value) => Some(whole_number(value, "\"frames\"", u64::MAX)?),
        };

        let expect = match top.get("expect") {
            None | Some(Value::Null) => None,
            Some(Value::Object(values)) => Some(
                values
                    .iter()
                    .map(|(name, value)| {
                        let value = match value {
                            Value::Bool(b) => ReportValue::Bool(*b),
                            Value::String(text) => ReportValue::Text(text.clone()),
                            Value::Number(n) => ReportValue::Number(n.as_f64().unwrap_or(0.0)),
                            other => {
                                return Err(format!(
                                    "\"expect\": {name:?} is {other}, but reported values are \
                                     numbers, true/false or text"
                                ));
                            }
                        };
                        Ok((name.clone(), value))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Some(_) => {
                return Err(String::from(
                    "\"expect\" must be an object like {\"score\": 20, \"state\": \"Won\"}",
                ));
            }
        };

        let tune = match top.get("tune") {
            None | Some(Value::Null) => None,
            Some(Value::Object(values)) => Some(
                values
                    .iter()
                    .map(|(name, value)| match value.as_f64() {
                        Some(n) => Ok((name.clone(), n as f32)),
                        None => Err(format!(
                            "\"tune\": {name:?} is {value}, but tune values are numbers"
                        )),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Some(_) => {
                return Err(String::from(
                    "\"tune\" must be an object like {\"jump_speed\": 330}",
                ));
            }
        };

        let files = match top.get("files") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(texts)) => texts
                .iter()
                .map(|(name, text)| match text.as_str() {
                    Some(text) => Ok((name.clone(), text.to_string())),
                    None => Err(format!("\"files\": {name:?} must be the file's text")),
                })
                .collect::<Result<_, _>>()?,
            Some(_) => {
                return Err(String::from(
                    "\"files\" must be an object like {\"examples/mygame/level.txt\": \"...\"}",
                ));
            }
        };

        let Some(list) = top.get("events") else {
            return Err(String::from("the script has no \"events\" list"));
        };
        let Value::Array(list) = list else {
            return Err(String::from("\"events\" must be a list: [...]"));
        };
        let mut events = Vec::new();
        let mut changes = Vec::new();
        for (index, event) in list.iter().enumerate() {
            let place = |err: String| format!("event {} ({event}): {err}", index + 1);
            if event.get("file").is_some() || event.get("tune").is_some() {
                changes.push(parse_change(event).map_err(place)?);
            } else {
                events.push(parse_event(event).map_err(place)?);
            }
        }
        // Events may be written in any order; same-frame events keep theirs.
        events.sort_by_key(|event| event.frame);
        changes.sort_by_key(|change| change.frame);

        Ok(InputScript {
            seed,
            updates_per_second,
            frames,
            events,
            expect,
            tune,
            files,
            changes,
        })
    }

    /// The script as JSON text, one event per line so it's easy to read and
    /// to compare.
    pub fn to_json(&self) -> String {
        let mut text = format!("{{\n  \"duckforge_inputs\": {VERSION},\n");
        if let Some(seed) = self.seed {
            text += &format!("  \"seed\": {seed},\n");
        }
        if let Some(updates) = self.updates_per_second {
            text += &format!("  \"updates_per_second\": {updates},\n");
        }
        if let Some(frames) = self.frames {
            text += &format!("  \"frames\": {frames},\n");
        }
        if let Some(expect) = &self.expect {
            let values: Vec<String> = expect
                .iter()
                .map(|(name, value)| format!("{}: {}", Value::from(name.as_str()), value.to_json()))
                .collect();
            text += &format!("  \"expect\": {{{}}},\n", values.join(", "));
        }
        if let Some(tune) = &self.tune {
            let values: Vec<String> = tune
                .iter()
                .map(|(name, &value)| format!("{}: {}", Value::from(name.as_str()), number(value)))
                .collect();
            text += &format!("  \"tune\": {{{}}},\n", values.join(", "));
        }
        if !self.files.is_empty() {
            text += "  \"files\": {";
            for (index, (name, file)) in self.files.iter().enumerate() {
                let comma = if index + 1 < self.files.len() {
                    ","
                } else {
                    ""
                };
                text += &format!(
                    "\n    {}: {}{comma}",
                    Value::from(name.as_str()),
                    Value::from(file.as_str())
                );
            }
            text += "\n  },\n";
        }
        // Inputs and changes in one list, by frame; on the same frame the
        // changes come first, as they're applied first.
        let mut lines: Vec<(u64, u8, String)> = self
            .changes
            .iter()
            .map(|change| (change.frame, 0, change_json(change)))
            .collect();
        lines.extend(
            self.events
                .iter()
                .map(|event| (event.frame, 1, event_json(event))),
        );
        lines.sort_by_key(|(frame, order, _)| (*frame, *order));
        text += "  \"events\": [";
        for (index, (_, _, line)) in lines.iter().enumerate() {
            let comma = if index + 1 < lines.len() { "," } else { "" };
            text += &format!("\n    {line}{comma}");
        }
        if !lines.is_empty() {
            text += "\n  ";
        }
        text += "]\n}\n";
        text
    }

    /// Saves the script as a JSON file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        fs::write(path, self.to_json())
            .map_err(|err| format!("could not save {}: {err}", path.display()))
    }

    /// How many frames a replay of this script runs when nobody says
    /// otherwise: its `"frames"`, or else until its last event plus two
    /// seconds, so every event is replayed and you see what it did. `None`
    /// for a script with neither.
    pub fn run_length(&self, updates_per_second: f64) -> Option<u64> {
        let after_last = |_| self.last_event_frame() + (2.0 * updates_per_second).round() as u64;
        self.frames.or_else(|| self.events.last().map(after_last))
    }

    /// How many events come after `frame`: in a run that stops there,
    /// they're never replayed.
    pub fn events_after(&self, frame: u64) -> usize {
        self.events
            .iter()
            .filter(|event| event.frame > frame)
            .count()
    }

    /// How a replay's final reported values differ from `expect`, one
    /// entry per difference, like `delivered 15 to 9` or
    /// `state "Lost" to "Won"`. Empty if they match, or if there's no
    /// `expect`.
    pub fn changes_from_expected<'a>(
        &self,
        reported: impl IntoIterator<Item = (&'a str, &'a ReportValue)>,
    ) -> Vec<String> {
        let Some(expect) = &self.expect else {
            return Vec::new();
        };
        let reported: BTreeMap<&str, &ReportValue> = reported.into_iter().collect();
        let mut changes = Vec::new();
        for (name, old) in expect {
            match reported.get(name.as_str()) {
                Some(new) if *new == old => {}
                Some(new) => changes.push(format!("{name} {old} to {new}")),
                None => changes.push(format!("{name} {old} to nothing")),
            }
        }
        for (name, new) in &reported {
            if !expect.contains_key(*name) {
                changes.push(format!("{name} new: {new}"));
            }
        }
        changes
    }

    /// Which `ctx.tune` values differ from the ones it was recorded with,
    /// like `sight 100 to 90`. Names the game didn't ask for this time are
    /// left out. Empty without a `"tune"` block.
    pub fn tune_changes(&self, now: impl IntoIterator<Item = (String, f32)>) -> Vec<String> {
        let Some(recorded) = &self.tune else {
            return Vec::new();
        };
        now.into_iter()
            .filter_map(|(name, value)| {
                let old = *recorded.get(&name)?;
                (old != value).then(|| format!("{name} {} to {}", number(old), number(value)))
            })
            .collect()
    }

    /// The last frame that has an event (an input or a change), or 0 if
    /// there are none.
    pub fn last_event_frame(&self) -> u64 {
        let input = self.events.last().map_or(0, |event| event.frame);
        let change = self.changes.last().map_or(0, |change| change.frame);
        input.max(change)
    }
}

/// A warning if `script` was recorded with a different timestep than
/// `config` uses: then the same inputs land at different moments of the
/// game, and the replay won't match the recording.
pub(crate) fn timestep_warning(script: &InputScript, config: &Config) -> Option<String> {
    let recorded = script.updates_per_second?;
    match config.timestep {
        Timestep::Fixed { updates_per_second } if updates_per_second == recorded => None,
        Timestep::Fixed { updates_per_second } => Some(format!(
            "these inputs were recorded at {recorded} updates per second, but the game now \
             runs {updates_per_second}, so the replay won't match the recording"
        )),
        Timestep::RealTime => Some(String::from(
            "this game uses real-time steps, so a replay won't exactly match the recording",
        )),
    }
}

/// One event as a line of JSON, like `{"frame": 12, "press": "Space"}`.
pub(crate) fn event_json(event: &InputEvent) -> String {
    let (name, value) = match event.action {
        InputAction::Press(key) => ("press", quoted(key.name())),
        InputAction::Release(key) => ("release", quoted(key.name())),
        InputAction::MouseMove(p) => ("mouse", format!("[{}, {}]", number(p.x), number(p.y))),
        InputAction::MouseMotion(d) => (
            "mouse_motion",
            format!("[{}, {}]", number(d.x), number(d.y)),
        ),
        InputAction::MousePress(button) => ("mouse_press", quoted(button.name())),
        InputAction::MouseRelease(button) => ("mouse_release", quoted(button.name())),
        InputAction::Scroll(amount) => ("scroll", number(amount)),
    };
    format!("{{\"frame\": {}, \"{name}\": {value}}}", event.frame)
}

/// One change as a line of JSON, like `{"frame": 413, "tune": {"gravity": 700}}`.
pub(crate) fn change_json(change: &ChangeEvent) -> String {
    match &change.change {
        Change::File { path, text } => format!(
            "{{\"frame\": {}, \"file\": {}, \"text\": {}}}",
            change.frame,
            Value::from(path.as_str()),
            Value::from(text.as_str())
        ),
        Change::Tune(values) => {
            let values: Vec<String> = values
                .iter()
                .map(|(name, &value)| format!("{}: {}", Value::from(name.as_str()), number(value)))
                .collect();
            format!(
                "{{\"frame\": {}, \"tune\": {{{}}}}}",
                change.frame,
                values.join(", ")
            )
        }
    }
}

pub(crate) fn parse_change(event: &Value) -> Result<ChangeEvent, String> {
    let Value::Object(fields) = event else {
        return Err(String::from("each event is an object"));
    };
    let frame = match fields.get("frame") {
        Some(value) => whole_number(value, "\"frame\"", u64::MAX)?,
        None => return Err(String::from("it has no \"frame\"")),
    };
    if frame == 0 {
        return Err(String::from("frames count from 1"));
    }
    let change = if let Some(path) = fields.get("file") {
        check_keys(fields, &["frame", "file", "text"], "a file change")?;
        let path = path.as_str().ok_or("\"file\" names the file, as text")?;
        let text = fields
            .get("text")
            .and_then(Value::as_str)
            .ok_or("a file change needs the file's new \"text\"")?;
        Change::File {
            path: path.to_string(),
            text: text.to_string(),
        }
    } else {
        check_keys(fields, &["frame", "tune"], "a tune change")?;
        let Some(Value::Object(values)) = fields.get("tune") else {
            return Err(String::from(
                "\"tune\" must be an object like {\"jump_speed\": 380}",
            ));
        };
        Change::Tune(
            values
                .iter()
                .map(|(name, value)| match value.as_f64() {
                    Some(n) => Ok((name.clone(), n as f32)),
                    None => Err(format!("\"tune\": {name:?} must be a number")),
                })
                .collect::<Result<_, _>>()?,
        )
    };
    Ok(ChangeEvent { frame, change })
}

/// A name in JSON quotes. (Key names never need escaping, but `Value`
/// would take care of it if they did.)
fn quoted(name: String) -> String {
    Value::from(name).to_string()
}

/// A number for JSON, written as briefly as possible: `160` rather than
/// `160.0`, and `0.33333334` rather than the `f64` version of the same
/// `f32`, `0.3333333432674408`. Rust's `{}` for an `f32` gives the shortest
/// text that reads back as exactly the same number.
fn number(x: f32) -> String {
    if x.fract() == 0.0 && x.abs() < 1e9 {
        format!("{}", x as i64)
    } else if x.is_finite() {
        format!("{x}")
    } else {
        String::from("0") // JSON has no infinity or NaN
    }
}

const ACTIONS: [&str; 7] = [
    "press",
    "release",
    "mouse",
    "mouse_motion",
    "mouse_press",
    "mouse_release",
    "scroll",
];

pub(crate) fn parse_event(event: &Value) -> Result<InputEvent, String> {
    let Value::Object(fields) = event else {
        return Err(String::from(
            "each event is an object like {\"frame\": 1, \"press\": \"Space\"}",
        ));
    };
    let mut allowed = vec!["frame"];
    allowed.extend(ACTIONS);
    check_keys(fields, &allowed, "an event")?;

    let frame = match fields.get("frame") {
        Some(value) => whole_number(value, "\"frame\"", u64::MAX)?,
        None => return Err(String::from("it has no \"frame\"")),
    };
    if frame == 0 {
        return Err(String::from(
            "frames count from 1 (ctx.frame() is 1 during the first update)",
        ));
    }

    let actions: Vec<(&str, &Value)> = ACTIONS
        .iter()
        .filter_map(|&name| fields.get(name).map(|value| (name, value)))
        .collect();
    let [(name, value)] = actions[..] else {
        return Err(format!(
            "it needs exactly one of {}; put each action in its own event",
            ACTIONS.join(", ")
        ));
    };
    let action = match name {
        "press" => InputAction::Press(key(value)?),
        "release" => InputAction::Release(key(value)?),
        "mouse" => InputAction::MouseMove(point(value, "a position [x, y] in canvas pixels")?),
        "mouse_motion" => InputAction::MouseMotion(point(
            value,
            "a movement [x, y] in canvas pixels (right and down are positive)",
        )?),
        "mouse_press" => InputAction::MousePress(button(value)?),
        "mouse_release" => InputAction::MouseRelease(button(value)?),
        "scroll" => InputAction::Scroll(
            value
                .as_f64()
                .ok_or("\"scroll\" needs a number, like 1 or -1")? as f32,
        ),
        _ => unreachable!("only names from ACTIONS"),
    };
    Ok(InputEvent { frame, action })
}

/// Refuses names that aren't in `allowed`: a misspelled `"preess"` would
/// otherwise be silently ignored.
fn check_keys(fields: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    match fields.keys().find(|name| !allowed.contains(&name.as_str())) {
        Some(name) => Err(format!(
            "{what} has an unknown field {name:?} (allowed: {})",
            allowed.join(", ")
        )),
        None => Ok(()),
    }
}

fn whole_number(value: &Value, what: &str, max: u64) -> Result<u64, String> {
    match value.as_u64() {
        Some(n) if n <= max => Ok(n),
        _ => Err(format!(
            "{what} needs a whole number from 0 to {max}, not {value}"
        )),
    }
}

fn key(value: &Value) -> Result<Key, String> {
    let name = value
        .as_str()
        .ok_or("a key is a name in quotes, like \"Space\"")?;
    Key::from_name(name).ok_or_else(|| {
        format!(
            "unknown key {name:?}. Key names look like \"Space\", \"Left\", \"A\", \
             \"Digit1\" or \"F5\"; docs/input-format.md lists them all"
        )
    })
}

fn button(value: &Value) -> Result<MouseButton, String> {
    let name = value
        .as_str()
        .ok_or("a mouse button is \"Left\", \"Right\" or \"Middle\"")?;
    MouseButton::from_name(name).ok_or_else(|| {
        format!("unknown mouse button {name:?}: use \"Left\", \"Right\" or \"Middle\"")
    })
}

fn point(value: &Value, wanted: &str) -> Result<Vec2, String> {
    let numbers: Option<Vec<f64>> = value
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_f64).collect());
    match numbers.as_deref() {
        Some(&[x, y]) => Ok(vec2(x as f32, y as f32)),
        _ => Err(format!("it needs {wanted}, not {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expect_is_read_written_and_compared() {
        let script = InputScript::parse(
            r#"{"frames": 10, "expect": {"delivered": 15, "state": "Lost", "won": false},
                "events": []}"#,
        )
        .unwrap();
        let again = InputScript::parse(&script.to_json()).unwrap();
        assert_eq!(again, script, "survives saving");
        assert!(
            script
                .to_json()
                .contains(r#""expect": {"delivered": 15, "state": "Lost", "won": false},"#)
        );
        let same = [
            ("delivered", ReportValue::from(15)),
            ("state", ReportValue::from("Lost")),
            ("won", ReportValue::from(false)),
        ];
        assert!(
            script
                .changes_from_expected(same.iter().map(|(n, v)| (*n, v)))
                .is_empty()
        );
        let different = [
            ("delivered", ReportValue::from(9)),
            ("state", ReportValue::from("Won")),
            ("bonus", ReportValue::from(1)),
        ];
        assert_eq!(
            script.changes_from_expected(different.iter().map(|(n, v)| (*n, v))),
            [
                "delivered 15 to 9",
                r#"state "Lost" to "Won""#,
                "won false to nothing",
                "bonus new: 1"
            ]
        );
        assert!(
            InputScript::parse(r#"{"expect": {"x": [1]}, "events": []}"#)
                .unwrap_err()
                .contains("numbers, true/false or text")
        );
    }

    #[test]
    fn without_frames_a_replay_lasts_until_two_seconds_after_the_last_event() {
        let script =
            InputScript::parse(r#"{"events": [{"frame": 898, "press": "Left"}]}"#).unwrap();
        assert_eq!(script.run_length(60.0), Some(898 + 120));
        assert_eq!(script.events_after(600), 1);
        assert_eq!(script.events_after(898), 0);
        let given = InputScript::parse(r#"{"frames": 300, "events": []}"#).unwrap();
        assert_eq!(given.run_length(60.0), Some(300), "an explicit length wins");
        assert_eq!(InputScript::default().run_length(60.0), None);
    }

    const EXAMPLE: &str = r#"{
      "duckforge_inputs": 1,
      "seed": 7,
      "frames": 300,
      "events": [
        {"frame": 30, "mouse": [160, 120.5]},
        {"frame": 1, "press": "Space"},
        {"frame": 2, "release": "space"},
        {"frame": 31, "mouse_press": "Left"},
        {"frame": 33, "mouse_release": "Left"},
        {"frame": 40, "scroll": -1}
      ]
    }"#;

    #[test]
    fn a_script_is_read() {
        let script = InputScript::parse(EXAMPLE).unwrap();
        assert_eq!(script.seed, Some(7));
        assert_eq!(script.frames, Some(300));
        assert_eq!(script.updates_per_second, None);
        assert_eq!(script.last_event_frame(), 40);
        // Sorted by frame.
        let actions: Vec<InputAction> = script.events.iter().map(|event| event.action).collect();
        assert_eq!(
            actions,
            [
                InputAction::Press(Key::Space),
                InputAction::Release(Key::Space),
                InputAction::MouseMove(vec2(160.0, 120.5)),
                InputAction::MousePress(MouseButton::Left),
                InputAction::MouseRelease(MouseButton::Left),
                InputAction::Scroll(-1.0),
            ]
        );
    }

    #[test]
    fn writing_and_reading_back_gives_the_same_script() {
        let script = InputScript {
            expect: None,
            tune: None,
            seed: Some(123_456),
            updates_per_second: Some(60),
            frames: Some(90),
            events: vec![
                InputEvent {
                    frame: 1,
                    action: InputAction::Press(Key::Digit7),
                },
                InputEvent {
                    frame: 5,
                    action: InputAction::MouseMove(vec2(1.0 / 3.0, 200.0)),
                },
                InputEvent {
                    frame: 6,
                    action: InputAction::MouseMotion(vec2(-4.0, 2.5)),
                },
                InputEvent {
                    frame: 9,
                    action: InputAction::Scroll(1.0),
                },
            ],
            files: BTreeMap::from([(
                String::from("examples/a/level.txt"),
                String::from("p.\"\n##\n"),
            )]),
            changes: vec![
                ChangeEvent {
                    frame: 6,
                    change: Change::File {
                        path: String::from("examples/a/level.txt"),
                        text: String::from("p=\n##\n"),
                    },
                },
                ChangeEvent {
                    frame: 6,
                    change: Change::Tune(BTreeMap::from([(String::from("jump_speed"), 380.5)])),
                },
            ],
        };
        let json = script.to_json();
        assert!(
            json.contains(
                "\n    {\"frame\": 6, \"tune\": {\"jump_speed\": 380.5}},\n    {\"frame\": 6, \"mouse_motion\""
            ),
            "changes come before the inputs of their frame: {json}"
        );
        assert!(json.contains(r#""files": {"#), "{json}");
        assert!(
            json.contains("\n    {\"frame\": 1, \"press\": \"Digit7\"},\n"),
            "{json}"
        );
        assert!(json.contains("[0.33333334, 200]"), "{json}");
        assert!(
            json.contains("{\"frame\": 6, \"mouse_motion\": [-4, 2.5]}"),
            "{json}"
        );
        assert_eq!(InputScript::parse(&json).unwrap(), script);

        let empty = InputScript::default();
        assert_eq!(InputScript::parse(&empty.to_json()).unwrap(), empty);
    }

    #[test]
    fn mistakes_get_clear_messages() {
        let error = |text: &str| InputScript::parse(text).unwrap_err();
        assert!(error("{").contains("not valid JSON"));
        assert!(error("[]").contains("JSON object"));
        assert!(error("{}").contains("no \"events\""));
        assert!(error(r#"{"events": [], "sead": 1}"#).contains("unknown field \"sead\""));
        assert!(error(r#"{"duckforge_inputs": 2, "events": []}"#).contains("version 1"));

        let event = |json: &str| error(&format!(r#"{{"events": [{{"frame": 1}}, {json}]}}"#));
        // The first event above has no action, so it's the one reported.
        assert!(event("{}").starts_with("event 1"));
        let event = |json: &str| error(&format!(r#"{{"events": [{json}]}}"#));
        assert!(event(r#"{"frame": 0, "press": "A"}"#).contains("count from 1"));
        assert!(event(r#"{"press": "A"}"#).contains("no \"frame\""));
        assert!(event(r#"{"frame": 2.5, "press": "A"}"#).contains("whole number"));
        assert!(event(r#"{"frame": 1, "press": "Spcae"}"#).contains("unknown key \"Spcae\""));
        assert!(event(r#"{"frame": 1, "press": "A", "release": "A"}"#).contains("exactly one"));
        assert!(event(r#"{"frame": 1, "preess": "A"}"#).contains("unknown field \"preess\""));
        assert!(event(r#"{"frame": 1, "mouse": [1]}"#).contains("[x, y]"));
        assert!(event(r#"{"frame": 1, "mouse_motion": 3}"#).contains("movement [x, y]"));
        assert!(event(r#"{"frame": 1, "mouse_press": "Back"}"#).contains("mouse button"));
        assert!(event(r#"{"frame": 1, "scroll": "up"}"#).contains("number"));
    }

    #[test]
    fn files_are_saved_and_loaded() {
        let path =
            std::env::temp_dir().join(format!("duckforge-script-{}.json", std::process::id()));
        let script = InputScript::parse(EXAMPLE).unwrap();
        script.save(&path).unwrap();
        assert_eq!(InputScript::load(&path).unwrap(), script);
        let _ = fs::remove_file(&path);
        let missing = InputScript::load(&path).unwrap_err();
        assert!(missing.contains("could not read"), "{missing}");
    }
}

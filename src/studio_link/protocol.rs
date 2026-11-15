//! The messages a game and Duck Forge Studio send each other.
//!
//! They travel over one local connection. Every message is one letter
//! saying what kind it is, four bytes saying how long it is, then the
//! message itself:
//!
//! - `J`: JSON text, like `{"tick": 7, "updates": 1, "events": [...]}`.
//! - `F`: a picture. A short JSON header (which frame, how big, how long it
//!   took), then the pixels exactly as [`Canvas::pixels`](crate::canvas::Canvas::pixels)
//!   has them.
//!
//! Inputs are written the same way as in an input script
//! (`docs/input-format.md`), so what the Studio sends is what a recording
//! holds.

use std::io::{self, Read, Write};

use serde_json::{Value, json};

use std::collections::BTreeMap;

pub use crate::canvas::DrawnMap;
pub use crate::scene::drawn::{DrawnObject, DrawnScene, Origin};
use crate::script::{InputEvent, event_json, parse_event};

/// The version of these messages. The game and the Studio must agree.
pub const LINK_VERSION: u32 = 3;

/// The environment variable holding the one-time key the Studio gives the
/// game it starts. The game says it back in its `hello`, so the Studio
/// knows it's talking to the game it started.
pub const KEY_VARIABLE: &str = "DUCKFORGE_STUDIO_KEY";

pub use crate::files::GIVE_FILES_VARIABLE;

/// The biggest message either side will read: a 4096 x 4096 picture.
const MAX_LENGTH: usize = 4096 * 4096 * 4 + 4096;

/// What the Studio tells the game.
#[derive(Debug, Clone, PartialEq)]
pub enum ToGame {
    /// "Here are the inputs since last time. Run this many updates, then
    /// send the picture." No updates means "just send the picture again".
    Tick {
        id: u64,
        updates: u32,
        events: Vec<InputEvent>,
    },
    /// Run this many updates without drawing, then send one picture. With
    /// a script (allowed only before the first update), the game follows
    /// it: that's how "replay my moves" gets back to where you were.
    Run {
        id: u64,
        updates: u64,
        script: Option<String>,
    },
    /// Open the game's own window (`true`), or close it and come back
    /// inside the Studio (`false`).
    Window { open: bool },
    /// Draw the debug overlay (F1) on the pictures from now on, or stop.
    /// It's drawn on top, so the game itself isn't changed by it.
    Overlay { debug: bool },
    /// A file the game read has new text (a level painted in Build mode):
    /// the game reads it again now, between two updates. The game answers
    /// with [`ToStudio::Changed`].
    File { path: String, text: String },
    /// New `ctx.tune` values (Feel), from now on.
    Tune { values: BTreeMap<String, f32> },
    /// An object of a scene changes as it is now, its file left alone
    /// ("for this run only"): `set` is the keys of its line to set. The
    /// game answers with [`ToStudio::Changed`], naming the scene's file.
    Object {
        file: String,
        id: u64,
        set: serde_json::Map<String, Value>,
    },
    /// How scenes are drawn on the pictures from now on: as their files
    /// have them (`true`: Build mode's "As it starts"), or as they are now.
    Show { as_it_starts: bool },
    /// Play a sound the game has played before, by its name, outside the
    /// game (the Studio's Sounds list). The game isn't changed by it.
    PlaySound { name: String },
    /// A question: `"recording"` asks for everything done so far as an
    /// input script; `"tunes"` for the `ctx.tune` values (a JSON list of
    /// name, value, default and source); `"sounds"` for the sounds played
    /// so far (name, and a `.wav`'s file or a synthesizer sound's recipe);
    /// `"files"` for the text files the game read; `"scripts"` for the
    /// scripts its scenes were given, each with its settings.
    Ask { id: u64, what: String },
    /// Say goodbye (the game's `quit` runs) and end.
    Quit,
}

/// The first thing a game says.
#[derive(Debug, Clone, PartialEq)]
pub struct Hello {
    pub link: u32,
    pub key: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub updates_per_second: f64,
    pub seed: u32,
}

/// One picture of the game, and what the Studio needs to know about it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frame {
    /// The update this picture shows (`ctx.frame()`).
    pub frame: u64,
    /// The tick (or run) it answers.
    pub tick: u64,
    pub width: u32,
    pub height: u32,
    /// How long the updates took, and the drawing, in millionths of a second.
    pub update_micros: u32,
    pub draw_micros: u32,
    /// The game wants the pointer hidden and held (`ctx.lock_pointer`).
    pub wants_pointer: bool,
    /// What drew the 3D: the CPU, or the graphics card's name.
    pub renderer: String,
    /// Where tile maps were drawn on it (for Build mode's grid).
    pub maps: Vec<DrawnMap>,
    /// Where scenes drew their objects, on a frozen picture (a tick with
    /// no updates): for Build mode's Scene list, selecting and moving.
    pub scenes: Vec<DrawnScene>,
    /// The pixels, row by row, each `0x00RRGGBB`.
    pub pixels: Vec<u32>,
}

/// What the game tells the Studio.
#[derive(Debug, Clone, PartialEq)]
pub enum ToStudio {
    Hello(Hello),
    Frame(Frame),
    /// The answer to an [`ToGame::Ask`].
    Answer {
        id: u64,
        what: String,
        text: String,
    },
    /// A line from the game's `ctx.studio_note`.
    Note(String),
    /// The game's own window opened or closed, on this frame.
    Window {
        open: bool,
        frame: u64,
    },
    /// The game took a changed file (`reloaded`), or can't without
    /// starting again.
    Changed {
        path: String,
        reloaded: bool,
    },
    /// The game is ending, and why: `"quit"` (the game asked to),
    /// `"asked"` (the Studio asked), or a problem in words.
    Bye {
        why: String,
    },
}

/// The pixels as the bytes that travel: four per pixel, lowest first.
#[cfg(target_endian = "little")]
fn pixel_bytes(pixels: &[u32]) -> std::borrow::Cow<'_, [u8]> {
    // SAFETY: a `u32` is four bytes with no padding, bytes can be read at
    // any address, and the new slice covers exactly the same memory for
    // the same lifetime. On a little-endian computer (every PC and Mac)
    // those bytes are already in the order the link uses, so the picture
    // is sent without being copied pixel by pixel.
    let bytes = unsafe {
        std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), std::mem::size_of_val(pixels))
    };
    std::borrow::Cow::Borrowed(bytes)
}

/// The pixels as the bytes that travel: four per pixel, lowest first.
#[cfg(not(target_endian = "little"))]
fn pixel_bytes(pixels: &[u32]) -> std::borrow::Cow<'_, [u8]> {
    let mut bytes = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        bytes.extend_from_slice(&pixel.to_le_bytes());
    }
    std::borrow::Cow::Owned(bytes)
}

fn bad(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn write_raw(to: &mut impl Write, kind: u8, parts: &[&[u8]]) -> io::Result<()> {
    let length: usize = parts.iter().map(|part| part.len()).sum();
    let mut head = [0u8; 5];
    head[0] = kind;
    head[1..].copy_from_slice(&(length as u32).to_le_bytes());
    to.write_all(&head)?;
    for part in parts {
        to.write_all(part)?;
    }
    to.flush()
}

/// Reads one message's kind and bytes. `Ok(None)` means the other side
/// closed the connection between messages.
fn read_raw(from: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match from.read(&mut head[..1])? {
        0 => return Ok(None),
        _ => from.read_exact(&mut head[1..])?,
    }
    let length = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if length > MAX_LENGTH {
        return Err(bad(format!("a message of {length} bytes is too long")));
    }
    let mut body = vec![0u8; length];
    from.read_exact(&mut body)?;
    Ok(Some((head[0], body)))
}

fn json_of(body: &[u8]) -> io::Result<Value> {
    serde_json::from_slice(body).map_err(|err| bad(format!("not JSON: {err}")))
}

fn text_of(value: &Value, name: &str) -> io::Result<String> {
    value[name]
        .as_str()
        .map(String::from)
        .ok_or_else(|| bad(format!("{name:?} should be text in {value}")))
}

fn number_of(value: &Value, name: &str) -> io::Result<u64> {
    value[name]
        .as_u64()
        .ok_or_else(|| bad(format!("{name:?} should be a whole number in {value}")))
}

fn events_json(events: &[InputEvent]) -> String {
    let list: Vec<String> = events.iter().map(event_json).collect();
    format!("[{}]", list.join(", "))
}

impl ToGame {
    /// Sends the message.
    pub fn write(&self, to: &mut impl Write) -> io::Result<()> {
        let text = match self {
            ToGame::Tick {
                id,
                updates,
                events,
            } => format!(
                "{{\"tick\": {id}, \"updates\": {updates}, \"events\": {}}}",
                events_json(events)
            ),
            ToGame::Run {
                id,
                updates,
                script,
            } => json!({"run": id, "updates": updates, "script": script}).to_string(),
            ToGame::Window { open } => json!({"window": open}).to_string(),
            ToGame::Overlay { debug } => json!({"overlay": {"debug": debug}}).to_string(),
            ToGame::File { path, text } => json!({"file": path, "text": text}).to_string(),
            ToGame::Tune { values } => json!({"tune": values}).to_string(),
            ToGame::Object { file, id, set } => {
                json!({"object": id, "in": file, "set": set}).to_string()
            }
            ToGame::Show { as_it_starts } => {
                json!({"show": {"as_it_starts": as_it_starts}}).to_string()
            }
            ToGame::PlaySound { name } => json!({"play_sound": name}).to_string(),
            ToGame::Ask { id, what } => json!({"ask": id, "what": what}).to_string(),
            ToGame::Quit => json!({"quit": true}).to_string(),
        };
        write_raw(to, b'J', &[text.as_bytes()])
    }

    /// Reads the next message, or `None` if the Studio closed the connection.
    pub fn read(from: &mut impl Read) -> io::Result<Option<ToGame>> {
        let Some((kind, body)) = read_raw(from)? else {
            return Ok(None);
        };
        if kind != b'J' {
            return Err(bad("the Studio only sends JSON messages"));
        }
        let value = json_of(&body)?;
        let message = if value.get("tick").is_some() {
            let events = value["events"]
                .as_array()
                .ok_or_else(|| bad("a tick needs an \"events\" list"))?
                .iter()
                .map(|event| parse_event(event).map_err(bad))
                .collect::<io::Result<Vec<_>>>()?;
            ToGame::Tick {
                id: number_of(&value, "tick")?,
                updates: number_of(&value, "updates")? as u32,
                events,
            }
        } else if value.get("run").is_some() {
            ToGame::Run {
                id: number_of(&value, "run")?,
                updates: number_of(&value, "updates")?,
                script: value["script"].as_str().map(String::from),
            }
        } else if let Some(open) = value.get("window").and_then(Value::as_bool) {
            ToGame::Window { open }
        } else if let Some(path) = value.get("file").and_then(Value::as_str) {
            ToGame::File {
                path: path.to_string(),
                text: text_of(&value, "text")?,
            }
        } else if let Some(Value::Object(values)) = value.get("tune") {
            ToGame::Tune {
                values: values
                    .iter()
                    .filter_map(|(name, value)| Some((name.clone(), value.as_f64()? as f32)))
                    .collect(),
            }
        } else if let Some(id) = value.get("object").and_then(Value::as_u64) {
            ToGame::Object {
                file: text_of(&value, "in")?,
                id,
                set: value["set"].as_object().cloned().unwrap_or_default(),
            }
        } else if let Some(show) = value.get("show") {
            ToGame::Show {
                as_it_starts: show["as_it_starts"].as_bool().unwrap_or(false),
            }
        } else if let Some(name) = value.get("play_sound").and_then(Value::as_str) {
            ToGame::PlaySound {
                name: name.to_string(),
            }
        } else if let Some(overlay) = value.get("overlay") {
            ToGame::Overlay {
                debug: overlay["debug"].as_bool().unwrap_or(false),
            }
        } else if value.get("ask").is_some() {
            ToGame::Ask {
                id: number_of(&value, "ask")?,
                what: text_of(&value, "what")?,
            }
        } else if value.get("quit").is_some() {
            ToGame::Quit
        } else {
            return Err(bad(format!("a message the game doesn't know: {value}")));
        };
        Ok(Some(message))
    }
}

impl ToStudio {
    /// Sends the message.
    pub fn write(&self, to: &mut impl Write) -> io::Result<()> {
        let text = match self {
            ToStudio::Frame(frame) => return frame.write(to),
            ToStudio::Hello(hello) => json!({
                "hello": hello.link,
                "key": hello.key,
                "title": hello.title,
                "width": hello.width,
                "height": hello.height,
                "updates_per_second": hello.updates_per_second,
                "seed": hello.seed,
            }),
            ToStudio::Answer { id, what, text } => {
                json!({"answer": id, "what": what, "text": text})
            }
            ToStudio::Note(text) => json!({"note": text}),
            ToStudio::Window { open, frame } => json!({"window": open, "frame": frame}),
            ToStudio::Changed { path, reloaded } => {
                json!({"changed": path, "reloaded": reloaded})
            }
            ToStudio::Bye { why } => json!({"bye": why}),
        }
        .to_string();
        write_raw(to, b'J', &[text.as_bytes()])
    }

    /// Reads the next message, or `None` if the game closed the connection.
    pub fn read(from: &mut impl Read) -> io::Result<Option<ToStudio>> {
        let Some((kind, body)) = read_raw(from)? else {
            return Ok(None);
        };
        if kind == b'F' {
            return Frame::from_bytes(&body).map(|frame| Some(ToStudio::Frame(frame)));
        }
        if kind != b'J' {
            return Err(bad(format!("a message of an unknown kind ({kind})")));
        }
        let value = json_of(&body)?;
        let message = if value.get("hello").is_some() {
            ToStudio::Hello(Hello {
                link: number_of(&value, "hello")? as u32,
                key: text_of(&value, "key")?,
                title: text_of(&value, "title")?,
                width: number_of(&value, "width")? as u32,
                height: number_of(&value, "height")? as u32,
                updates_per_second: value["updates_per_second"].as_f64().unwrap_or(60.0),
                seed: number_of(&value, "seed")? as u32,
            })
        } else if value.get("answer").is_some() {
            ToStudio::Answer {
                id: number_of(&value, "answer")?,
                what: text_of(&value, "what")?,
                text: text_of(&value, "text")?,
            }
        } else if value.get("note").is_some() {
            ToStudio::Note(text_of(&value, "note")?)
        } else if let Some(open) = value.get("window").and_then(Value::as_bool) {
            ToStudio::Window {
                open,
                frame: number_of(&value, "frame")?,
            }
        } else if let Some(path) = value.get("changed").and_then(Value::as_str) {
            ToStudio::Changed {
                path: path.to_string(),
                reloaded: value["reloaded"].as_bool().unwrap_or(false),
            }
        } else if value.get("bye").is_some() {
            ToStudio::Bye {
                why: text_of(&value, "bye")?,
            }
        } else {
            return Err(bad(format!("a message the Studio doesn't know: {value}")));
        };
        Ok(Some(message))
    }
}

impl Frame {
    /// The header in front of the pixels.
    fn header(&self) -> String {
        json!({
            "frame": self.frame,
            "tick": self.tick,
            "width": self.width,
            "height": self.height,
            "update_us": self.update_micros,
            "draw_us": self.draw_micros,
            "pointer": self.wants_pointer,
            "renderer": self.renderer,
            "maps": self.maps.iter().map(|map| json!({
                "file": map.file,
                "x": map.x,
                "y": map.y,
                "scale": map.scale,
                "tile": map.tile_size,
                "columns": map.columns,
                "rows": map.rows,
            })).collect::<Vec<_>>(),
            "scenes": self.scenes.iter().map(DrawnScene::to_json).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// Sends the picture: the header's length, the header, then the pixels.
    fn write(&self, to: &mut impl Write) -> io::Result<()> {
        self.write_with(to, &self.pixels)
    }

    /// Sends this frame's header with `pixels` after it, so the game can
    /// send its canvas as it is, without copying it into a `Frame` first.
    pub fn write_with(&self, to: &mut impl Write, pixels: &[u32]) -> io::Result<()> {
        let header = self.header();
        let length = (header.len() as u32).to_le_bytes();
        let bytes = pixel_bytes(pixels);
        write_raw(to, b'F', &[&length, header.as_bytes(), &bytes])
    }

    fn from_bytes(body: &[u8]) -> io::Result<Frame> {
        let short = || bad("a picture message that's too short");
        let length = body.get(..4).ok_or_else(short)?;
        let length = u32::from_le_bytes([length[0], length[1], length[2], length[3]]) as usize;
        let header = json_of(body.get(4..4 + length).ok_or_else(short)?)?;
        let (width, height) = (
            number_of(&header, "width")? as u32,
            number_of(&header, "height")? as u32,
        );
        let bytes = &body[4 + length..];
        if bytes.len() != width as usize * height as usize * 4 {
            return Err(bad(format!(
                "a {width} x {height} picture with {} bytes of pixels",
                bytes.len()
            )));
        }
        let pixels = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        Ok(Frame {
            frame: number_of(&header, "frame")?,
            tick: number_of(&header, "tick")?,
            width,
            height,
            update_micros: number_of(&header, "update_us")? as u32,
            draw_micros: number_of(&header, "draw_us")? as u32,
            wants_pointer: header["pointer"].as_bool().unwrap_or(false),
            renderer: text_of(&header, "renderer")?,
            maps: header["maps"]
                .as_array()
                .map(|maps| {
                    maps.iter()
                        .map(|map| DrawnMap {
                            file: map["file"].as_str().unwrap_or_default().to_string(),
                            x: map["x"].as_f64().unwrap_or(0.0) as f32,
                            y: map["y"].as_f64().unwrap_or(0.0) as f32,
                            scale: map["scale"].as_f64().unwrap_or(1.0) as f32,
                            tile_size: map["tile"].as_f64().unwrap_or(16.0) as f32,
                            columns: map["columns"].as_u64().unwrap_or(0) as usize,
                            rows: map["rows"].as_u64().unwrap_or(0) as usize,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            scenes: header["scenes"]
                .as_array()
                .map(|scenes| scenes.iter().map(DrawnScene::from_json).collect())
                .unwrap_or_default(),
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Key, MouseButton};
    use crate::math::vec2;
    use crate::script::InputAction;

    fn there_and_back_to_game(message: ToGame) {
        let mut bytes = Vec::new();
        message.write(&mut bytes).unwrap();
        let read = ToGame::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(read, Some(message));
    }

    fn there_and_back_to_studio(message: ToStudio) {
        let mut bytes = Vec::new();
        message.write(&mut bytes).unwrap();
        let read = ToStudio::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(read, Some(message));
    }

    #[test]
    fn every_message_to_the_game_survives_the_trip() {
        let event = |frame, action| InputEvent { frame, action };
        there_and_back_to_game(ToGame::Tick {
            id: 7,
            updates: 1,
            events: vec![
                event(12, InputAction::Press(Key::Space)),
                event(12, InputAction::MouseMove(vec2(160.5, 20.0))),
                event(12, InputAction::MouseMotion(vec2(-3.0, 1.25))),
                event(12, InputAction::MousePress(MouseButton::Left)),
                event(12, InputAction::Scroll(-1.0)),
                event(12, InputAction::Release(Key::Space)),
            ],
        });
        there_and_back_to_game(ToGame::Tick {
            id: 8,
            updates: 0,
            events: Vec::new(),
        });
        there_and_back_to_game(ToGame::Run {
            id: 9,
            updates: 600,
            script: Some(String::from("{\"events\": []}")),
        });
        there_and_back_to_game(ToGame::Run {
            id: 10,
            updates: 5,
            script: None,
        });
        there_and_back_to_game(ToGame::Window { open: true });
        there_and_back_to_game(ToGame::Overlay { debug: true });
        there_and_back_to_game(ToGame::PlaySound {
            name: String::from("jump"),
        });
        there_and_back_to_game(ToGame::File {
            path: String::from("examples/a/level.txt"),
            text: String::from("p.\"\n##\n"),
        });
        there_and_back_to_game(ToGame::Tune {
            values: BTreeMap::from([(String::from("jump_speed"), 380.5)]),
        });
        there_and_back_to_game(ToGame::Ask {
            id: 3,
            what: String::from("recording"),
        });
        there_and_back_to_game(ToGame::Quit);
    }

    #[test]
    fn every_message_to_the_studio_survives_the_trip() {
        there_and_back_to_studio(ToStudio::Hello(Hello {
            link: LINK_VERSION,
            key: String::from("k3y"),
            title: String::from("Duck \"Peaks\""),
            width: 320,
            height: 240,
            updates_per_second: 60.0,
            seed: 4_000_000_000,
        }));
        there_and_back_to_studio(ToStudio::Frame(Frame {
            frame: 412,
            tick: 400,
            width: 3,
            height: 2,
            update_micros: 210,
            draw_micros: 3100,
            wants_pointer: true,
            renderer: String::from("CPU"),
            maps: vec![DrawnMap {
                file: String::from("examples/a/level.txt"),
                x: -32.0,
                y: 0.0,
                scale: 1.0,
                tile_size: 16.0,
                columns: 80,
                rows: 15,
            }],
            scenes: Vec::new(),
            pixels: vec![0, 0x00FF_FFFF, 0x0012_3456, 1, 2, 3],
        }));
        there_and_back_to_studio(ToStudio::Changed {
            path: String::from("examples/a/level.txt"),
            reloaded: true,
        });
        there_and_back_to_studio(ToStudio::Answer {
            id: 3,
            what: String::from("recording"),
            text: String::from("{\n  \"events\": []\n}\n"),
        });
        there_and_back_to_studio(ToStudio::Note(String::from("a fresh island")));
        there_and_back_to_studio(ToStudio::Window {
            open: false,
            frame: 99,
        });
        there_and_back_to_studio(ToStudio::Bye {
            why: String::from("quit"),
        });
    }

    #[test]
    fn a_closed_connection_and_nonsense_are_told_apart() {
        assert_eq!(ToGame::read(&mut [].as_slice()).unwrap(), None);
        // Cut off in the middle of a message: that's a mistake, not an end.
        let mut bytes = Vec::new();
        ToGame::Quit.write(&mut bytes).unwrap();
        assert!(ToGame::read(&mut &bytes[..bytes.len() - 2]).is_err());
        // A picture whose pixels don't match its size.
        let mut bytes = Vec::new();
        Frame {
            width: 2,
            height: 2,
            pixels: vec![1, 2, 3],
            ..Frame::default()
        }
        .write(&mut bytes)
        .unwrap();
        let problem = ToStudio::read(&mut bytes.as_slice()).unwrap_err();
        assert!(problem.to_string().contains("2 x 2"), "{problem}");
    }
}

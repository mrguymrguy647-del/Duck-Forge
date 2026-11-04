//! Scenes: what was on screen, in words instead of pixels.
//!
//! In `draw`, a game can name what it draws with
//! [`Canvas::label`](crate::canvas::Canvas::label):
//!
//! ```
//! # use duckforge::prelude::*;
//! # fn draw(canvas: &mut Canvas, player: Rect) {
//! canvas.label("player", player);
//! # }
//! ```
//!
//! A label draws nothing. The engine remembers it, with where it is in the
//! world and on the screen, together with every piece of text drawn and
//! the camera. That's a [`Scene`]. Headless runs save one next to each
//! picture (`last.scene.json`, `frame-000300.scene.json`), `--describe`
//! prints the last one in words, and tests can look at
//! [`Simulation::scene`](crate::Simulation::scene).

use serde_json::Value;

use crate::math::{Rect, Vec2};

/// Everything a game labelled in one frame, and the text it drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    /// The frame this is a picture of.
    pub frame: u64,
    /// The first camera the game pushed, if any.
    pub camera: Option<SceneCamera>,
    /// Every label, in the order they were drawn.
    pub labels: Vec<Label>,
    /// Every piece of text drawn with `draw_text` (or `draw_text_centered`),
    /// one entry per line.
    pub texts: Vec<SceneText>,
}

/// The camera, as it was when the scene was drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneCamera {
    /// The world point in the middle of the screen.
    pub position: Vec2,
    pub zoom: f32,
}

/// One thing the game labelled with `canvas.label(name, rect)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub name: String,
    /// The rectangle as the game gave it: in the world if drawn through a
    /// camera, else in screen pixels.
    pub world: Rect,
    /// Where it was on the screen, in pixels.
    pub screen: Rect,
    /// Whether any of it was on the screen.
    pub on_screen: bool,
}

/// One line of text the game drew.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneText {
    pub text: String,
    /// The area it covers on the screen, in pixels.
    pub screen: Rect,
}

impl Scene {
    /// The labels called `name`.
    pub fn labels_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Label> + 'a {
        self.labels.iter().filter(move |label| label.name == name)
    }

    /// A short description in words: the camera, one line per kind of
    /// label (repeats grouped, like `rock x4 at ...`), and the text drawn.
    ///
    /// ```text
    /// frame 600, camera 480,120 zoom 1
    /// duck at 154,216 12x12
    /// rock x3 at 30,50 80,120 200,10 (+1 off screen)
    /// text: "TIME 10.0" at 4,4; "LIVES 3" at 286,4
    /// ```
    pub fn describe(&self) -> String {
        let mut lines = Vec::new();
        let mut first = format!("frame {}", self.frame);
        if let Some(camera) = &self.camera {
            first += &format!(
                ", camera {:.0},{:.0} zoom {}",
                camera.position.x,
                camera.position.y,
                short(camera.zoom)
            );
        }
        lines.push(first);

        let mut names: Vec<&str> = Vec::new();
        for label in &self.labels {
            if !names.contains(&label.name.as_str()) {
                names.push(&label.name);
            }
        }
        for name in names {
            let all: Vec<&Label> = self.labels_named(name).collect();
            let visible: Vec<&&Label> = all.iter().filter(|l| l.on_screen).collect();
            let hidden = all.len() - visible.len();
            let place = |l: &Label| format!("{:.0},{:.0}", l.world.x, l.world.y);
            let mut line = if all.len() == 1 {
                let l = all[0];
                format!(
                    "{name} at {} {}x{}",
                    place(l),
                    short(l.world.w),
                    short(l.world.h)
                )
            } else {
                let shown: Vec<String> = visible.iter().take(8).map(|l| place(l)).collect();
                let more = visible.len().saturating_sub(8);
                let mut line = format!("{name} x{}", all.len());
                if !shown.is_empty() {
                    line += &format!(" at {}", shown.join(" "));
                }
                if more > 0 {
                    line += &format!(" (+{more} more)");
                }
                line
            };
            if hidden > 0 {
                line += &if all.len() == 1 {
                    String::from(" (off screen)")
                } else {
                    format!(" (+{hidden} off screen)")
                };
            }
            lines.push(line);
        }
        if !self.texts.is_empty() {
            let texts: Vec<String> = self
                .texts
                .iter()
                .map(|t| format!("{:?} at {:.0},{:.0}", t.text, t.screen.x, t.screen.y))
                .collect();
            lines.push(format!("text: {}", texts.join("; ")));
        }
        lines.join("\n")
    }

    /// The scene as JSON, for `*.scene.json`.
    pub(crate) fn to_json(&self) -> String {
        let rect = |r: &Rect| format!("[{}, {}, {}, {}]", num(r.x), num(r.y), num(r.w), num(r.h));
        let camera = match &self.camera {
            Some(c) => format!(
                "{{\"x\": {}, \"y\": {}, \"zoom\": {}}}",
                num(c.position.x),
                num(c.position.y),
                num(c.zoom)
            ),
            None => String::from("null"),
        };
        let labels: Vec<String> = self
            .labels
            .iter()
            .map(|l| {
                format!(
                    "    {{\"name\": {}, \"world\": {}, \"screen\": {}, \"on_screen\": {}}}",
                    Value::from(l.name.as_str()),
                    rect(&l.world),
                    rect(&l.screen),
                    l.on_screen
                )
            })
            .collect();
        let texts: Vec<String> = self
            .texts
            .iter()
            .map(|t| {
                format!(
                    "    {{\"text\": {}, \"screen\": {}}}",
                    Value::from(t.text.as_str()),
                    rect(&t.screen)
                )
            })
            .collect();
        let list = |items: Vec<String>| {
            if items.is_empty() {
                String::from("[]")
            } else {
                format!("[\n{}\n  ]", items.join(",\n"))
            }
        };
        format!(
            "{{\n  \"frame\": {},\n  \"camera\": {camera},\n  \"labels\": {},\n  \"texts\": {}\n}}\n",
            self.frame,
            list(labels),
            list(texts)
        )
    }
}

/// A number rounded to 2 decimals, without trailing zeros.
fn short(value: f32) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A number for JSON, rounded to 2 decimals.
fn num(value: f32) -> String {
    if value.is_finite() {
        short(value)
    } else {
        String::from("null")
    }
}

/// Problems found by looking at a run's scenes, for the run summary.
pub(crate) fn warnings(scenes: &[Scene], width: f32, height: f32) -> Vec<String> {
    let mut warnings = Vec::new();
    let canvas = Rect::new(0.0, 0.0, width, height);

    // Labels that overlap on frame 1: often things spawned on top of each
    // other. A label inside another (an enemy inside the "level") is fine.
    if let Some(first) = scenes.iter().find(|s| s.frame == 1) {
        for (i, a) in first.labels.iter().enumerate() {
            for b in &first.labels[i + 1..] {
                let inside = |x: &Rect, y: &Rect| {
                    x.left() >= y.left()
                        && x.right() <= y.right()
                        && x.top() >= y.top()
                        && x.bottom() <= y.bottom()
                };
                if a.world.overlaps(&b.world)
                    && !inside(&a.world, &b.world)
                    && !inside(&b.world, &a.world)
                {
                    warnings.push(format!(
                        "frame 1: labels {:?} at {:.0},{:.0} and {:?} at {:.0},{:.0} overlap (spawned on top of each other?)",
                        a.name, a.world.x, a.world.y, b.name, b.world.x, b.world.y
                    ));
                }
            }
        }
    }

    // Text cut off by the edge of the screen (the first time each text is).
    let mut seen = Vec::new();
    for scene in scenes {
        for text in &scene.texts {
            let inside = text.screen.left() >= 0.0
                && text.screen.top() >= 0.0
                && text.screen.right() <= canvas.right()
                && text.screen.bottom() <= canvas.bottom();
            if !inside && !seen.contains(&text.text) {
                seen.push(text.text.clone());
                warnings.push(format!(
                    "frame {}: the text {:?} at {:.0},{:.0} goes off the edge of the screen",
                    scene.frame, text.text, text.screen.x, text.screen.y
                ));
            }
        }
    }

    // A kind of label that's in every checked frame, but never on screen.
    if scenes.len() >= 2 {
        let mut names: Vec<&str> = Vec::new();
        for label in scenes.iter().flat_map(|s| &s.labels) {
            if !names.contains(&label.name.as_str()) {
                names.push(&label.name);
            }
        }
        for name in names {
            let in_every = scenes.iter().all(|s| s.labels_named(name).next().is_some());
            let ever_seen = scenes
                .iter()
                .any(|s| s.labels_named(name).any(|l| l.on_screen));
            if in_every && !ever_seen {
                let frames: Vec<String> = scenes.iter().map(|s| s.frame.to_string()).collect();
                warnings.push(format!(
                    "label {name:?} is off screen in every checked frame ({}): is it drawn outside the camera's view?",
                    frames.join(", ")
                ));
            }
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vec2;

    fn label(name: &str, x: f32, y: f32, on_screen: bool) -> Label {
        let r = Rect::new(x, y, 10.0, 10.0);
        Label {
            name: name.to_string(),
            world: r,
            screen: r,
            on_screen,
        }
    }

    #[test]
    fn describing_groups_repeats() {
        let scene = Scene {
            frame: 600,
            camera: Some(SceneCamera {
                position: vec2(480.0, 120.0),
                zoom: 1.0,
            }),
            labels: vec![
                label("duck", 154.0, 216.0, true),
                label("rock", 30.0, 50.0, true),
                label("rock", 80.0, 120.0, true),
                label("rock", 900.0, 10.0, false),
            ],
            texts: vec![SceneText {
                text: String::from("LIVES 3"),
                screen: Rect::new(286.0, 4.0, 27.0, 5.0),
            }],
        };
        assert_eq!(
            scene.describe(),
            "frame 600, camera 480,120 zoom 1\n\
             duck at 154,216 10x10\n\
             rock x3 at 30,50 80,120 (+1 off screen)\n\
             text: \"LIVES 3\" at 286,4"
        );
        assert_eq!(scene.labels_named("rock").count(), 3);
        let json = scene.to_json();
        assert!(json.contains(r#"{"name": "rock", "world": [900, 10, 10, 10], "screen": [900, 10, 10, 10], "on_screen": false}"#), "{json}");
        assert!(
            json.contains(r#""camera": {"x": 480, "y": 120, "zoom": 1}"#),
            "{json}"
        );
    }

    #[test]
    fn warnings_name_the_frame_and_the_label() {
        let first = Scene {
            frame: 1,
            labels: vec![
                label("player", 10.0, 10.0, true),
                label("enemy", 15.0, 15.0, true),
                label("ghost", -50.0, 0.0, false),
                Label {
                    name: String::from("level"),
                    world: Rect::new(0.0, 0.0, 100.0, 100.0),
                    screen: Rect::new(0.0, 0.0, 100.0, 100.0),
                    on_screen: true,
                },
            ],
            texts: vec![SceneText {
                text: String::from("SCORE 1234567"),
                screen: Rect::new(300.0, 4.0, 51.0, 5.0),
            }],
            ..Scene::default()
        };
        let later = Scene {
            frame: 60,
            labels: vec![label("ghost", -80.0, 0.0, false)],
            ..Scene::default()
        };
        let warnings = warnings(&[first, later], 320.0, 240.0);
        assert_eq!(warnings.len(), 3, "{warnings:#?}");
        assert!(
            warnings[0]
                .starts_with(r#"frame 1: labels "player" at 10,10 and "enemy" at 15,15 overlap"#)
        );
        assert!(
            warnings[1]
                .starts_with(r#"frame 1: the text "SCORE 1234567" at 300,4 goes off the edge"#)
        );
        assert!(
            warnings[2]
                .starts_with(r#"label "ghost" is off screen in every checked frame (1, 60)"#)
        );
    }
}

//! What was on screen, in words instead of pixels.
//!
//! (Not to be confused with [`Scene`](crate::scene::Scene): a game's
//! objects in a scene file. This is what a frame *showed*; its files are
//! still called `last.scene.json`, and `sim.scene()` still gives one.)
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
//! the camera. That's an [`OnScreen`]. Headless runs save one next to each
//! picture (`last.scene.json`, `frame-000300.scene.json`), `--describe`
//! prints the last one in words, and tests can look at
//! [`Simulation::scene`](crate::Simulation::scene).

use serde_json::Value;

use crate::math::{Rect, Vec2, vec2};

/// Everything a game labelled in one frame, and the text it drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OnScreen {
    /// The frame this is a picture of.
    pub frame: u64,
    /// The first camera the game pushed, if any.
    pub camera: Option<OnScreenCamera>,
    /// Every label, in the order they were drawn.
    pub labels: Vec<Label>,
    /// Every piece of text drawn with `draw_text` (or `draw_text_centered`),
    /// one entry per line.
    pub texts: Vec<OnScreenText>,
}

/// The camera, as it was when the scene was drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnScreenCamera {
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
pub struct OnScreenText {
    pub text: String,
    /// The area it covers on the screen, in pixels.
    pub screen: Rect,
}

impl OnScreen {
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

/// The part of the world a scene's screen showed: through its camera,
/// or the screen itself without one.
fn view_of(scene: &OnScreen, width: f32, height: f32) -> Rect {
    match scene.camera {
        Some(camera) => {
            let zoom = if camera.zoom > 0.0 { camera.zoom } else { 1.0 };
            let size = vec2(width, height) * (1.0 / zoom);
            Rect::new(
                camera.position.x - size.x / 2.0,
                camera.position.y - size.y / 2.0,
                size.x,
                size.y,
            )
        }
        None => Rect::new(0.0, 0.0, width, height),
    }
}

/// The smallest rectangle around both.
fn union(a: Rect, b: Rect) -> Rect {
    let left = a.left().min(b.left());
    let top = a.top().min(b.top());
    Rect::new(
        left,
        top,
        a.right().max(b.right()) - left,
        a.bottom().max(b.bottom()) - top,
    )
}

/// Problems found by looking at a run's scenes, for the run summary.
pub(crate) fn warnings(scenes: &[OnScreen], width: f32, height: f32) -> Vec<String> {
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

    // A kind of label that's in every checked frame, but never on screen,
    // and never anywhere near the game's world: usually drawn in the wrong
    // coordinates (screen pixels through a camera, or the reverse).
    //
    // Off screen alone is normal: a long level has enemies ahead that the
    // camera hasn't reached, and wasps start out of view. So a label is
    // only suspicious when it's also outside the world's area: everything
    // the camera showed in the checked frames, and every label of a kind
    // that was on screen at least once (the level, the ground, the coins:
    // things drawn in the right coordinates, which show how big the world
    // is).
    if scenes.len() >= 2 {
        let mut names: Vec<&str> = Vec::new();
        for label in scenes.iter().flat_map(|s| &s.labels) {
            if !names.contains(&label.name.as_str()) {
                names.push(&label.name);
            }
        }
        let ever_seen = |name: &str| {
            scenes
                .iter()
                .any(|s| s.labels_named(name).any(|l| l.on_screen))
        };
        let mut world: Option<Rect> = None;
        let mut grow = |r: Rect| {
            world = Some(world.map_or(r, |w| union(w, r)));
        };
        for scene in scenes {
            grow(view_of(scene, width, height));
        }
        for name in names.iter().filter(|name| ever_seen(name)) {
            for scene in scenes {
                for label in scene.labels_named(name) {
                    grow(label.world);
                }
            }
        }
        let world = world.unwrap_or(canvas);
        for name in names {
            let in_every = scenes.iter().all(|s| s.labels_named(name).next().is_some());
            let anywhere_near = scenes
                .iter()
                .any(|s| s.labels_named(name).any(|l| l.world.overlaps(&world)));
            if in_every && !ever_seen(name) && !anywhere_near {
                let frames: Vec<String> = scenes.iter().map(|s| s.frame.to_string()).collect();
                let first = scenes[0].labels_named(name).next().map(|l| l.world);
                let at = first.map_or(String::new(), |r| format!(" at {:.0},{:.0}", r.x, r.y));
                warnings.push(format!(
                    "label {name:?}{at} is off screen in every checked frame ({}), and outside the world the camera showed ({:.0},{:.0} to {:.0},{:.0}): is it drawn in the wrong coordinates?",
                    frames.join(", "),
                    world.left(),
                    world.top(),
                    world.right(),
                    world.bottom()
                ));
            }
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let scene = OnScreen {
            frame: 600,
            camera: Some(OnScreenCamera {
                position: vec2(480.0, 120.0),
                zoom: 1.0,
            }),
            labels: vec![
                label("duck", 154.0, 216.0, true),
                label("rock", 30.0, 50.0, true),
                label("rock", 80.0, 120.0, true),
                label("rock", 900.0, 10.0, false),
            ],
            texts: vec![OnScreenText {
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
        let first = OnScreen {
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
            texts: vec![OnScreenText {
                text: String::from("SCORE 1234567"),
                screen: Rect::new(300.0, 4.0, 51.0, 5.0),
            }],
            ..OnScreen::default()
        };
        let later = OnScreen {
            frame: 60,
            labels: vec![label("ghost", -80.0, 0.0, false)],
            ..OnScreen::default()
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
                .starts_with(r#"label "ghost" at -50,0 is off screen in every checked frame (1, 60), and outside the world the camera showed (0,0 to 320,240)"#),
            "{warnings:#?}"
        );
    }

    #[test]
    fn off_screen_things_inside_the_world_are_normal() {
        // A long level, 2000 wide, labelled as "ground"; a wasp far ahead
        // that the camera hasn't reached; a "ghost" drawn in the wrong
        // coordinates, far outside everything.
        let scene = |frame: u64, camera_x: f32| OnScreen {
            frame,
            camera: Some(OnScreenCamera {
                position: vec2(camera_x, 120.0),
                zoom: 1.0,
            }),
            labels: vec![
                Label {
                    name: String::from("ground"),
                    world: Rect::new(0.0, 200.0, 2000.0, 40.0),
                    screen: Rect::new(160.0 - camera_x, 200.0, 2000.0, 40.0),
                    on_screen: true,
                },
                label("wasp", 1500.0, 10.0, false),
                label("ghost", -900.0, 10.0, false),
            ],
            ..OnScreen::default()
        };
        for frames in [
            [scene(1, 160.0), scene(60, 160.0)], // the camera stays put
            [scene(1, 160.0), scene(60, 400.0)], // it has scrolled a little
        ] {
            let warnings = warnings(&frames, 320.0, 240.0);
            assert_eq!(warnings.len(), 1, "{warnings:#?}");
            assert!(
                warnings[0].starts_with(r#"label "ghost" at -900,10 is off screen"#),
                "{warnings:#?}"
            );
        }
        // Somewhere the camera has shown counts as the world too, even
        // with nothing else labelled there.
        let wasp_only = |frame: u64, camera_x: f32| OnScreen {
            frame,
            camera: Some(OnScreenCamera {
                position: vec2(camera_x, 120.0),
                zoom: 1.0,
            }),
            labels: vec![label("wasp", 400.0, 10.0, false)],
            ..OnScreen::default()
        };
        let passed = [wasp_only(1, 160.0), wasp_only(60, 800.0)];
        assert!(warnings(&passed, 320.0, 240.0).is_empty());
        let not_yet = [wasp_only(1, 160.0), wasp_only(60, 161.0)];
        assert_eq!(warnings(&not_yet, 320.0, 240.0).len(), 1);
    }
}

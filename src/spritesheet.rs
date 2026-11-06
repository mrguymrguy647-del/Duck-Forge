//! Sprite sheets from Duck Quill: a PNG of animation frames and a JSON
//! that says where each animation is.
//!
//! `quill export hero.json` writes `hero.png` (one row per animation, every
//! frame the same size) and `hero.json`:
//!
//! ```json
//! { "image": "hero.png", "frame": { "width": 16, "height": 16 },
//!   "columns": 3, "rows": 2, "scale": 1, "pivot": [8.0, 16.0],
//!   "animations": {
//!     "idle": { "row": 0, "frames": [0, 1], "fps": 4.0, "loop": true },
//!     "jump": { "row": 1, "frames": [3, 4, 5], "fps": 10.0, "loop": false } } }
//! ```
//!
//! [`SpriteSheet::load`] reads both and gives each animation by name, with
//! its speed and whether it loops, and the *pivot*: the point in a frame
//! that stands on the ground (the bottom centre unless the JSON says
//! otherwise). Drawing by the pivot keeps a character's feet in the same
//! place whichever animation it plays.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use crate::canvas::Canvas;
use crate::error::LoadError;
use crate::math::{Vec2, vec2};
use crate::sprite::{Animation, Blit, Sprite};

/// A Duck Quill sprite sheet: named animations and a pivot.
#[derive(Debug, Clone)]
pub struct SpriteSheet {
    image: Sprite,
    frame_width: usize,
    frame_height: usize,
    pivot: Vec2,
    scale: u32,
    /// In the order the JSON lists them.
    animations: Vec<(String, Animation)>,
}

impl SpriteSheet {
    /// Reads a sprite sheet's JSON and the PNG it names (in the same
    /// folder). Errors are one sentence naming the file, including a PNG
    /// whose size doesn't match the JSON (a picture saved with
    /// `quill render --zoom` instead of `quill export`, say).
    pub fn load(path: impl AsRef<Path>) -> Result<SpriteSheet, LoadError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "sprite sheet", &error))?;
        let invalid = |message: String| LoadError::new(io::ErrorKind::InvalidData, message);
        let json: Value = serde_json::from_str(&text).map_err(|error| {
            invalid(format!(
                "{} isn't valid JSON ({error}); export it again with quill export",
                path.display()
            ))
        })?;
        let image_name = json["image"].as_str().ok_or_else(|| {
            invalid(format!(
                "{} has no \"image\": is it a Duck Quill sprite sheet (made by quill export)?",
                path.display()
            ))
        })?;
        let image_path = path.parent().unwrap_or(Path::new("")).join(image_name);
        let image = Sprite::load_png(&image_path)?;
        SpriteSheet::from_parts(&json, image, &path.display().to_string(), image_name)
            .map_err(invalid)
    }

    /// Builds a sheet from its parsed JSON and its picture. `name` and
    /// `image_name` are only used in error messages.
    fn from_parts(
        json: &Value,
        image: Sprite,
        name: &str,
        image_name: &str,
    ) -> Result<SpriteSheet, String> {
        let whole = |value: &Value, what: &str| -> Result<usize, String> {
            value
                .as_u64()
                .filter(|&n| n > 0)
                .map(|n| n as usize)
                .ok_or_else(|| {
                    format!("{name}: {what} must be a whole number above 0, not {value}")
                })
        };
        let frame_width = whole(&json["frame"]["width"], "\"frame\" \"width\"")?;
        let frame_height = whole(&json["frame"]["height"], "\"frame\" \"height\"")?;
        let columns = whole(&json["columns"], "\"columns\"")?;
        let rows = whole(&json["rows"], "\"rows\"")?;
        let scale = json["scale"].as_u64().unwrap_or(1).max(1) as u32;

        // The size check: the picture must be exactly the grid the JSON describes.
        let (want_w, want_h) = (columns * frame_width, rows * frame_height);
        if (image.width(), image.height()) != (want_w, want_h) {
            let zoom = image.width() / want_w.max(1);
            let hint = if image.width() == want_w * zoom
                && image.height() == want_h * zoom
                && zoom > 1
            {
                format!(
                    "it's exactly {zoom} times too big: was it saved with quill render --zoom {zoom}? \
                     Use quill export (with --scale to enlarge), which writes both files to match"
                )
            } else {
                String::from(
                    "export the sheet again with quill export, which writes both files to match",
                )
            };
            return Err(format!(
                "{image_name} is {} x {}, but {name} says {columns} x {rows} frames of \
                 {frame_width} x {frame_height} ({want_w} x {want_h}); {hint}",
                image.width(),
                image.height()
            ));
        }

        let pivot = match json["pivot"].as_array().map(Vec::as_slice) {
            Some([x, y]) => vec2(
                x.as_f64().unwrap_or(0.0) as f32,
                y.as_f64().unwrap_or(0.0) as f32,
            ),
            // The bottom centre: the character stands on its feet.
            _ => vec2(frame_width as f32 / 2.0, frame_height as f32),
        };

        let cells = image.frames(frame_width, frame_height);
        let mut animations = Vec::new();
        let Some(list) = json["animations"].as_object() else {
            return Err(format!("{name} has no \"animations\""));
        };
        for (animation_name, entry) in list {
            let numbers = entry["frames"].as_array().cloned().unwrap_or_default();
            let mut frames = Vec::with_capacity(numbers.len());
            for number in &numbers {
                let n = number.as_u64().unwrap_or(u64::MAX) as usize;
                let cell = cells.get(n).ok_or_else(|| {
                    format!(
                        "{name}: animation {animation_name:?} uses frame {number}, but the sheet \
                         has only {} frames (0 to {})",
                        cells.len(),
                        cells.len().saturating_sub(1)
                    )
                })?;
                frames.push(cell.clone());
            }
            if frames.is_empty() {
                return Err(format!(
                    "{name}: animation {animation_name:?} has no frames"
                ));
            }
            let fps = entry["fps"].as_f64().unwrap_or(10.0) as f32;
            let mut animation = Animation::new(frames, fps);
            animation.looping = entry["loop"].as_bool().unwrap_or(true);
            animations.push((animation_name.clone(), animation));
        }
        Ok(SpriteSheet {
            image,
            frame_width,
            frame_height,
            pivot,
            scale,
            animations,
        })
    }

    /// The animation called `name`.
    ///
    /// # Panics
    ///
    /// If there's none by that name, listing the names there are (a
    /// misspelt name is a bug to fix, like an index out of range).
    #[track_caller]
    pub fn animation(&self, name: &str) -> &Animation {
        match self.get(name) {
            Some(animation) => animation,
            None => panic!(
                "the sprite sheet has no animation {name:?}; it has: {}",
                self.names().collect::<Vec<_>>().join(", ")
            ),
        }
    }

    /// The animation called `name`, or `None`.
    pub fn get(&self, name: &str) -> Option<&Animation> {
        self.animations
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, animation)| animation)
    }

    /// The animations' names, in the order of the JSON.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.animations.iter().map(|(name, _)| name.as_str())
    }

    /// The point in a frame, in its pixels, that stands where the
    /// character stands: the bottom centre unless the JSON says otherwise.
    pub fn pivot(&self) -> Vec2 {
        self.pivot
    }

    /// One frame's width and height in pixels.
    pub fn frame_size(&self) -> (usize, usize) {
        (self.frame_width, self.frame_height)
    }

    /// How many times Duck Quill enlarged the frames when exporting (1 =
    /// as painted).
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// The whole picture.
    pub fn image(&self) -> &Sprite {
        &self.image
    }

    /// Draws animation `name` as it is `time` seconds after it started,
    /// with its pivot at `at`. (Short for `animation(name).frame_at(time)`
    /// and [`Canvas::draw_at_pivot`].)
    #[track_caller]
    pub fn draw(&self, canvas: &mut Canvas, name: &str, time: f32, at: Vec2, blit: Blit) {
        let frame = self.animation(name).frame_at(time);
        canvas.draw_at_pivot(frame, at, self.pivot, blit);
    }
}

impl Canvas {
    /// Draws a sprite so that its `pivot` (a point in the sprite, in its
    /// own pixels) lands on `at`. Mirroring and scaling keep the pivot in
    /// place, so a character flipped to face left still stands on the same
    /// spot. With a [`SpriteSheet`], pass `sheet.pivot()`.
    pub fn draw_at_pivot(&mut self, sprite: &Sprite, at: Vec2, pivot: Vec2, blit: Blit) {
        let pivot_x = if blit.flip_x {
            sprite.width() as f32 - pivot.x
        } else {
            pivot.x
        };
        let pivot_y = if blit.flip_y {
            sprite.height() as f32 - pivot.y
        } else {
            pivot.y
        };
        let top_left = at - vec2(pivot_x, pivot_y) * blit.scale;
        self.blit_sprite(sprite, top_left, blit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quill");

    #[test]
    fn a_quill_export_loads_with_its_animations() {
        let sheet = SpriteSheet::load(format!("{FIXTURES}/scale1/duck.json")).unwrap();
        assert_eq!(sheet.names().collect::<Vec<_>>(), ["idle", "jump"]);
        assert_eq!(sheet.frame_size(), (16, 16));
        assert_eq!(sheet.scale(), 1);
        assert_eq!(sheet.pivot(), vec2(8.0, 16.0), "bottom centre");
        let idle = sheet.animation("idle");
        assert_eq!((idle.frames.len(), idle.fps, idle.looping), (2, 4.0, true));
        let jump = sheet.animation("jump");
        assert_eq!(
            (jump.frames.len(), jump.fps, jump.looping),
            (3, 10.0, false)
        );
        // jump is frames 3, 4, 5: the second row.
        assert_eq!(
            jump.frames[0].get_pixel(0, 0),
            sheet.image().get_pixel(0, 16)
        );
        // Not looping: it stays on the last frame.
        assert_eq!(jump.index_at(10.0), 2);
        assert_eq!(idle.index_at(10.25), 1, "looping: 41 frames in, the second");
    }

    #[test]
    fn a_scaled_export_has_scaled_frames_and_pivot() {
        let one = SpriteSheet::load(format!("{FIXTURES}/scale1/duck.json")).unwrap();
        let two = SpriteSheet::load(format!("{FIXTURES}/scale2/duck.json")).unwrap();
        assert_eq!(two.scale(), 2);
        assert_eq!(two.frame_size(), (32, 32));
        assert_eq!(two.pivot(), vec2(16.0, 32.0));
        // The same picture, each pixel a 2 x 2 square.
        let (small, big) = (
            &one.animation("idle").frames[0],
            &two.animation("idle").frames[0],
        );
        for (x, y) in [(8, 8), (5, 3), (12, 10)] {
            assert_eq!(small.get_pixel(x, y), big.get_pixel(2 * x + 1, 2 * y + 1));
        }
        // Drawn by the pivot, the scale-2 sheet and the scale-1 sheet drawn
        // twice as big cover exactly the same pixels.
        let draw = |sheet: &SpriteSheet, scale: f32| {
            let mut canvas = Canvas::new(64, 64);
            canvas.clear(Color::BLACK);
            let blit = Blit {
                scale,
                ..Default::default()
            };
            sheet.draw(&mut canvas, "idle", 0.0, vec2(32.0, 60.0), blit);
            canvas
        };
        let (a, b) = (draw(&two, 1.0), draw(&one, 2.0));
        let painted = |c: &Canvas| -> Vec<(usize, usize)> {
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .filter(|&(x, y)| c.get_pixel(x as i32, y as i32) != Some(Color::BLACK))
                .collect()
        };
        assert!(!painted(&a).is_empty());
        assert_eq!(painted(&a), painted(&b));
        assert!(
            painted(&a).iter().all(|&(_, y)| y < 60),
            "nothing below the feet"
        );
    }

    #[test]
    fn a_picture_of_the_wrong_size_is_a_clear_error() {
        // The scale-1 JSON with the scale-2 picture: like a quill render --zoom 2.
        let folder = std::env::temp_dir().join(format!("duckforge-sheet-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            format!("{FIXTURES}/scale1/duck.json"),
            folder.join("duck.json"),
        )
        .unwrap();
        fs::copy(
            format!("{FIXTURES}/scale2/duck.png"),
            folder.join("duck.png"),
        )
        .unwrap();
        let error = SpriteSheet::load(folder.join("duck.json"))
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("duck.png is 96 x 64, but "), "{error}");
        assert!(
            error.contains("says 3 x 2 frames of 16 x 16 (48 x 32)"),
            "{error}"
        );
        assert!(
            error.contains("exactly 2 times too big: was it saved with quill render --zoom 2?"),
            "{error}"
        );
        let _ = fs::remove_dir_all(folder);
        let missing = SpriteSheet::load("no/such/sheet.json").unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    #[should_panic(expected = "no animation \"run\"; it has: idle, jump")]
    fn a_misspelt_animation_names_the_ones_there_are() {
        let sheet = SpriteSheet::load(format!("{FIXTURES}/scale1/duck.json")).unwrap();
        sheet.animation("run");
    }

    #[test]
    fn flipping_keeps_the_pivot_in_place() {
        // A 4 x 2 sprite whose left column is red; pivot at its bottom-right corner.
        let sprite = Sprite::from_ascii(&["r...", "r..."], &[('r', Color::RED)]);
        let mut canvas = Canvas::new(16, 8);
        canvas.clear(Color::BLACK);
        let blit = Blit {
            flip_x: true,
            ..Default::default()
        };
        canvas.draw_at_pivot(&sprite, vec2(8.0, 4.0), vec2(4.0, 2.0), blit);
        // Flipped, the pivot is the bottom-left corner: the sprite covers x 8..12.
        assert_eq!(
            canvas.get_pixel(11, 3),
            Some(Color::RED),
            "the red column, now on the right"
        );
        assert_eq!(canvas.get_pixel(7, 3), Some(Color::BLACK));
    }
}

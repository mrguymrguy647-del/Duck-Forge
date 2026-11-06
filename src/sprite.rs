//! Sprites: small pictures made of pixels, and animations made of sprites.
//!
//! A [`Sprite`] is a grid of colored pixels, some of which can be
//! see-through. Make one from a list of colors, from a `.png` file, or by
//! drawing it with letters right in your code, then draw it with
//! [`Canvas::blit_sprite`](crate::canvas::Canvas::blit_sprite).
//!
//! ```
//! use duckforge::prelude::*;
//!
//! let duck = Sprite::from_ascii(
//!     &[
//!         ".yy.",
//!         "yyyo",
//!         ".yy.",
//!     ],
//!     &[('y', Color::YELLOW), ('o', Color::ORANGE)],
//! );
//! let mut canvas = Canvas::new(40, 30);
//! canvas.blit_sprite(&duck, vec2(10.0, 10.0), Blit { scale: 2.0, ..Default::default() });
//! ```

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::sync::Arc;

use crate::color::Color;
use crate::error::LoadError;

/// A picture made of pixels, with see-through parts.
///
/// Cloning a sprite is cheap: clones, and frames cut out with
/// [`Sprite::sub`], share the same pixels instead of copying them.
#[derive(Clone)]
pub struct Sprite {
    /// The pixels, shared by every sprite cut from the same picture.
    /// (`Arc` is a pointer that counts its owners; the pixels are freed when
    /// the last sprite using them is dropped.)
    sheet: Arc<Sheet>,
    /// Where this sprite's top-left pixel is in the sheet.
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

/// The pixels behind one or more sprites.
struct Sheet {
    width: usize,
    /// Each pixel as `0x00RRGGBB`, the same format the canvas uses, so an
    /// opaque row can be copied onto the canvas in one go.
    rgb: Vec<u32>,
    /// How opaque each pixel is: 0 is invisible, 255 is solid.
    alpha: Vec<u8>,
    /// True if every alpha is 255, which allows the fast copy.
    opaque: bool,
}

impl Sheet {
    fn new(width: usize, rgb: Vec<u32>, alpha: Vec<u8>) -> Sheet {
        let opaque = alpha.iter().all(|&a| a == 255);
        Sheet {
            width,
            rgb,
            alpha,
            opaque,
        }
    }
}

impl Sprite {
    /// A sprite from a list of colors, row by row from the top-left, so the
    /// pixel at `(x, y)` is `pixels[y * width + x]`.
    ///
    /// # Panics
    ///
    /// If `pixels` doesn't hold exactly `width * height` colors.
    #[track_caller]
    pub fn from_pixels(width: usize, height: usize, pixels: Vec<Color>) -> Sprite {
        assert!(
            Some(pixels.len()) == width.checked_mul(height),
            "Sprite::from_pixels got {} colors, but a {width} x {height} sprite needs \
             exactly {} (width times height)",
            pixels.len(),
            width.saturating_mul(height),
        );
        let rgb = pixels.iter().map(|c| c.to_u32()).collect();
        let alpha = pixels.iter().map(|c| c.a).collect();
        Sprite::whole(Sheet::new(width, rgb, alpha), height)
    }

    /// A sprite drawn with letters: each character is one pixel, `palette`
    /// says which color each character means, and `'.'` is see-through.
    ///
    /// Handy for small pixel art, written straight into the code:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let heart = Sprite::from_ascii(
    ///     &[
    ///         ".r.r.",
    ///         "rrrrr",
    ///         ".rrr.",
    ///         "..r..",
    ///     ],
    ///     &[('r', Color::RED)],
    /// );
    /// assert_eq!((heart.width(), heart.height()), (5, 4));
    /// ```
    ///
    /// # Panics
    ///
    /// If the rows aren't all the same length, or a character isn't `'.'`
    /// and isn't in the palette. The message says which row and character.
    #[track_caller]
    pub fn from_ascii(rows: &[&str], palette: &[(char, Color)]) -> Sprite {
        let width = rows.first().map_or(0, |row| row.chars().count());
        let mut rgb = Vec::with_capacity(width * rows.len());
        let mut alpha = Vec::with_capacity(width * rows.len());
        for (y, row) in rows.iter().enumerate() {
            let length = row.chars().count();
            assert!(
                length == width,
                "Sprite::from_ascii: row {y} is {length} characters long but row 0 is {width}; \
                 every row must be the same length (use '.' for see-through pixels)"
            );
            for (x, c) in row.chars().enumerate() {
                if c == '.' {
                    rgb.push(0);
                    alpha.push(0);
                    continue;
                }
                let Some(&(_, color)) = palette.iter().find(|(key, _)| *key == c) else {
                    panic!(
                        "Sprite::from_ascii: the character {c:?} at column {x} of row {y} \
                         isn't in the palette; add ({c:?}, some_color) to it, or use '.' \
                         for a see-through pixel"
                    );
                };
                rgb.push(color.to_u32());
                alpha.push(color.a);
            }
        }
        Sprite::whole(Sheet::new(width, rgb, alpha), rows.len())
    }

    /// Loads a sprite from a `.png` file. See-through parts of the image
    /// stay see-through.
    ///
    /// The path is relative to the folder you run `cargo` from (usually
    /// the project folder), so `"assets/duck.png"` is a file in the
    /// project's `assets` folder.
    ///
    /// If the file is missing or isn't a PNG, the error says so in one
    /// sentence, with the full path it looked for.
    pub fn load_png(path: impl AsRef<Path>) -> Result<Sprite, LoadError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| missing_file_error(path, "image", &error))?;
        let bad_png = |error: &dyn fmt::Display| {
            LoadError::new(
                io::ErrorKind::InvalidData,
                format!(
                    "couldn't read {} as a PNG image ({error}); check that it really is a \
                     .png file, or open it in an image editor and export it as PNG again",
                    path.display()
                ),
            )
        };

        let mut decoder = png::Decoder::new(BufReader::new(file));
        // Turn every kind of PNG (palette, gray, 16 bits per channel...)
        // into 8 bits per channel, so only four layouts are left below.
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|e| bad_png(&e))?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| bad_png(&"the image is too big"))?;
        let mut bytes = vec![0; size];
        let info = reader.next_frame(&mut bytes).map_err(|e| bad_png(&e))?;
        let (width, height) = (info.width as usize, info.height as usize);

        let channels = match info.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => return Err(bad_png(&"unexpected palette image")),
        };
        let mut rgb = Vec::with_capacity(width * height);
        let mut alpha = Vec::with_capacity(width * height);
        for y in 0..height {
            let row = &bytes[y * info.line_size..][..width * channels];
            for p in row.chunks_exact(channels) {
                let (r, g, b, a) = match *p {
                    [v] => (v, v, v, 255),
                    [v, a] => (v, v, v, a),
                    [r, g, b] => (r, g, b, 255),
                    [r, g, b, a] => (r, g, b, a),
                    _ => unreachable!("chunks_exact gives exactly `channels` bytes"),
                };
                rgb.push(Color::rgb(r, g, b).to_u32());
                alpha.push(a);
            }
        }
        Ok(Sprite::whole(Sheet::new(width, rgb, alpha), height))
    }

    fn whole(sheet: Sheet, height: usize) -> Sprite {
        Sprite {
            left: 0,
            top: 0,
            width: sheet.width,
            height,
            sheet: Arc::new(sheet),
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// A rectangle cut out of this sprite, `w` x `h` pixels with its
    /// top-left corner at `(x, y)`. Use it to take one frame out of a
    /// sprite sheet. Nothing is copied: the new sprite shares the pixels.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let sheet = Sprite::from_pixels(64, 16, vec![Color::WHITE; 64 * 16]);
    /// let second_frame = sheet.sub(16, 0, 16, 16);
    /// ```
    ///
    /// # Panics
    ///
    /// If the rectangle doesn't fit inside the sprite.
    #[track_caller]
    pub fn sub(&self, x: usize, y: usize, w: usize, h: usize) -> Sprite {
        let fits = x.checked_add(w).is_some_and(|right| right <= self.width)
            && y.checked_add(h).is_some_and(|bottom| bottom <= self.height);
        assert!(
            fits,
            "Sprite::sub({x}, {y}, {w}, {h}) reaches outside the {} x {} sprite; \
             x + w must be at most the width and y + h at most the height",
            self.width, self.height
        );
        Sprite {
            sheet: Arc::clone(&self.sheet),
            left: self.left + x,
            top: self.top + y,
            width: w,
            height: h,
        }
    }

    /// Cuts a sprite sheet into frames of `frame_width` x `frame_height`,
    /// left to right, then top to bottom. Frames that would stick out past
    /// the edge are left out. Nothing is copied.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let sheet = Sprite::from_pixels(64, 16, vec![Color::WHITE; 64 * 16]);
    /// let frames = sheet.frames(16, 16); // 4 frames side by side
    /// let walk = Animation::new(frames, 8.0);
    /// ```
    pub fn frames(&self, frame_width: usize, frame_height: usize) -> Vec<Sprite> {
        if frame_width == 0 || frame_height == 0 {
            return Vec::new();
        }
        let mut frames = Vec::new();
        for row in 0..self.height / frame_height {
            for column in 0..self.width / frame_width {
                frames.push(self.sub(
                    column * frame_width,
                    row * frame_height,
                    frame_width,
                    frame_height,
                ));
            }
        }
        frames
    }

    /// The color of the pixel at `(x, y)`, or `None` outside the sprite.
    /// The color's alpha says how see-through it is (0 is invisible).
    pub fn get_pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = self.index(x, y);
        Some(Color::from_hex(self.sheet.rgb[i]).with_alpha(self.sheet.alpha[i]))
    }

    /// Where pixel `(x, y)` of this sprite is in the sheet's lists.
    pub(crate) fn index(&self, x: usize, y: usize) -> usize {
        (self.top + y) * self.sheet.width + self.left + x
    }

    /// One row of the sprite, as canvas colors and alphas.
    pub(crate) fn row(&self, y: usize) -> (&[u32], &[u8]) {
        let start = self.index(0, y);
        let end = start + self.width;
        (&self.sheet.rgb[start..end], &self.sheet.alpha[start..end])
    }

    /// The colour (`0x00RRGGBB`) and alpha of pixel `(x, y)`, which must be
    /// inside the sprite. For 3D textures, read many times a frame.
    #[inline]
    pub(crate) fn texel(&self, x: usize, y: usize) -> (u32, u8) {
        let i = self.index(x, y);
        (self.sheet.rgb[i], self.sheet.alpha[i])
    }

    /// The whole picture this sprite was cut from (itself, if it wasn't
    /// cut), and where this sprite is in it: left, top, width, height.
    pub(crate) fn in_sheet(&self) -> (Sprite, [usize; 4]) {
        let height = self.sheet.rgb.len() / self.sheet.width.max(1);
        let whole = Sprite {
            sheet: Arc::clone(&self.sheet),
            left: 0,
            top: 0,
            width: self.sheet.width,
            height,
        };
        (whole, [self.left, self.top, self.width, self.height])
    }

    /// Whether two sprites show exactly the same pixels of the same picture.
    pub(crate) fn same_pixels(&self, other: &Sprite) -> bool {
        Arc::ptr_eq(&self.sheet, &other.sheet)
            && (self.left, self.top, self.width, self.height)
                == (other.left, other.top, other.width, other.height)
    }

    /// A number that's the same for sprites showing the same pixels of the
    /// same picture (while the picture exists), for the GPU's texture cache.
    #[cfg_attr(not(feature = "gpu"), allow(dead_code))] // only the GPU uses it
    pub(crate) fn identity(&self) -> (usize, [usize; 4]) {
        (
            Arc::as_ptr(&self.sheet) as usize,
            [self.left, self.top, self.width, self.height],
        )
    }

    /// True if the picture this sprite was cut from has no see-through
    /// pixels at all.
    pub(crate) fn is_opaque(&self) -> bool {
        self.sheet.opaque
    }
}

impl fmt::Debug for Sprite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Printing every pixel would flood the screen, so just give the size.
        write!(f, "Sprite({} x {})", self.width, self.height)
    }
}

/// The one-sentence error for a file that can't be opened. `what` is the
/// kind of file, like `"image"` or `"sound"`.
pub(crate) fn missing_file_error(path: &Path, what: &str, error: &io::Error) -> LoadError {
    if error.kind() == io::ErrorKind::NotFound {
        let full = std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf());
        LoadError::new(
            io::ErrorKind::NotFound,
            format!(
                "couldn't find the {what} file {} (looked for {}); paths are relative to the \
                 folder you run cargo from, so check the spelling and that the file is there",
                path.display(),
                full.display()
            ),
        )
    } else {
        LoadError::new(
            error.kind(),
            format!("couldn't open the {what} file {} ({error})", path.display()),
        )
    }
}

/// How to draw a sprite with
/// [`Canvas::blit_sprite`](crate::canvas::Canvas::blit_sprite).
///
/// Set only what you need and let `..Default::default()` fill in the rest:
///
/// ```
/// # use duckforge::prelude::*;
/// let facing_left = Blit { flip_x: true, scale: 3.0, ..Default::default() };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blit {
    /// Mirror left to right (to face the other way).
    pub flip_x: bool,
    /// Mirror top to bottom.
    pub flip_y: bool,
    /// How many screen pixels each sprite pixel becomes: 2.0 draws it twice
    /// as big. Pixels stay sharp squares (no blurring). 1.0 by default.
    pub scale: f32,
    /// Every pixel's color is multiplied by this: `Color::WHITE` (the
    /// default) changes nothing, `Color::RED` keeps only the red.
    pub tint: Color,
    /// How solid the whole sprite is: 1.0 (the default) as it is, 0.5 half
    /// see-through, 0.0 invisible.
    pub opacity: f32,
}

impl Default for Blit {
    fn default() -> Self {
        Blit {
            flip_x: false,
            flip_y: false,
            scale: 1.0,
            tint: Color::WHITE,
            opacity: 1.0,
        }
    }
}

/// Frames that play one after another, like a flip book.
///
/// ```
/// # use duckforge::prelude::*;
/// # let sheet = Sprite::from_pixels(64, 16, vec![Color::WHITE; 64 * 16]);
/// let walk = Animation::new(sheet.frames(16, 16), 8.0); // 8 frames per second
/// // In draw: pick the frame for the current time.
/// let frame = walk.frame_at(1.3);
/// ```
#[derive(Debug, Clone)]
pub struct Animation {
    /// The frames, in order.
    pub frames: Vec<Sprite>,
    /// How many frames to show per second.
    pub fps: f32,
    /// Whether it starts again from the first frame after the last (the
    /// default). If not, it stays on the last frame: a jump, a death.
    pub looping: bool,
}

impl Animation {
    /// An animation that shows `frames` in order, `fps` per second, and
    /// starts again from the first when it reaches the end.
    ///
    /// # Panics
    ///
    /// If `frames` is empty.
    #[track_caller]
    pub fn new(frames: Vec<Sprite>, fps: f32) -> Animation {
        assert!(
            !frames.is_empty(),
            "Animation::new needs at least one frame, but got none (did Sprite::frames \
             get a frame size bigger than the sheet?)"
        );
        Animation {
            frames,
            fps,
            looping: true,
        }
    }

    /// The frame to show `time` seconds after the animation started. It
    /// loops forever, or stays on the last frame if `looping` is off.
    ///
    /// # Panics
    ///
    /// If `frames` has been emptied since the animation was made.
    #[track_caller]
    pub fn frame_at(&self, time: f32) -> &Sprite {
        self.frames.get(self.index_at(time)).expect(
            "Animation::frame_at: the animation has no frames left; keep at least one in `frames`",
        )
    }

    /// The number of the frame to show at `time` (0 is the first).
    pub fn index_at(&self, time: f32) -> usize {
        let count = self.frames.len();
        if count == 0 || self.fps.is_nan() || self.fps <= 0.0 || !time.is_finite() {
            return 0;
        }
        let step = (time * self.fps).floor() as i64;
        if self.looping {
            step.rem_euclid(count as i64) as usize
        } else {
            step.clamp(0, count as i64 - 1) as usize
        }
    }

    /// Seconds to play every frame once.
    pub fn duration(&self) -> f32 {
        if self.fps > 0.0 {
            self.frames.len() as f32 / self.fps
        } else {
            f32::INFINITY
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(width: usize, height: usize) -> Sprite {
        // Pixel (x, y) has the color 0x0000yx, so each one is easy to recognise.
        let pixels = (0..height)
            .flat_map(|y| (0..width).map(move |x| Color::rgb(0, y as u8, x as u8)))
            .collect();
        Sprite::from_pixels(width, height, pixels)
    }

    #[test]
    fn sub_shares_pixels_and_offsets_coordinates() {
        let sheet = numbered(8, 4);
        let frame = sheet.sub(2, 1, 3, 2);
        assert_eq!((frame.width(), frame.height()), (3, 2));
        assert_eq!(frame.get_pixel(0, 0), Some(Color::rgb(0, 1, 2)));
        assert_eq!(frame.get_pixel(2, 1), Some(Color::rgb(0, 2, 4)));
        assert_eq!(frame.get_pixel(3, 0), None);
        assert!(Arc::ptr_eq(&sheet.sheet, &frame.sheet), "sub must not copy");
        // A cut of a cut is relative to the cut.
        assert_eq!(
            frame.sub(1, 1, 1, 1).get_pixel(0, 0),
            Some(Color::rgb(0, 2, 3))
        );
    }

    #[test]
    fn frames_cut_left_to_right_then_down() {
        let frames = numbered(5, 4).frames(2, 2);
        assert_eq!(
            frames.len(),
            4,
            "the 1-pixel strip on the right is left out"
        );
        assert_eq!(frames[1].get_pixel(0, 0), Some(Color::rgb(0, 0, 2)));
        assert_eq!(frames[2].get_pixel(0, 0), Some(Color::rgb(0, 2, 0)));
        assert!(numbered(5, 4).frames(0, 2).is_empty());
    }

    #[test]
    #[should_panic(expected = "reaches outside")]
    fn sub_outside_the_sprite_says_so() {
        numbered(4, 4).sub(3, 0, 2, 1);
    }

    #[test]
    fn ascii_dots_are_see_through() {
        let s = Sprite::from_ascii(&["a.", ".b"], &[('a', Color::RED), ('b', Color::BLUE)]);
        assert_eq!(s.get_pixel(0, 0), Some(Color::RED));
        assert_eq!(s.get_pixel(1, 0).unwrap().a, 0);
        assert_eq!(s.get_pixel(1, 1), Some(Color::BLUE));
        assert!(!s.is_opaque());
        assert!(Sprite::from_ascii(&["aa"], &[('a', Color::RED)]).is_opaque());
    }

    #[test]
    #[should_panic(expected = "row 1 is 3 characters long but row 0 is 2")]
    fn ascii_rows_must_match() {
        Sprite::from_ascii(&["aa", "aaa"], &[('a', Color::RED)]);
    }

    #[test]
    #[should_panic(expected = "the character 'z' at column 1 of row 0 isn't in the palette")]
    fn ascii_unknown_characters_are_named() {
        Sprite::from_ascii(&["az"], &[('a', Color::RED)]);
    }

    #[test]
    #[should_panic(expected = "got 3 colors, but a 2 x 2 sprite needs exactly 4")]
    fn from_pixels_checks_the_count() {
        Sprite::from_pixels(2, 2, vec![Color::RED; 3]);
    }

    #[test]
    fn animation_loops_by_time() {
        let walk = Animation::new(numbered(4, 1).frames(1, 1), 10.0);
        assert_eq!(walk.index_at(0.0), 0);
        assert_eq!(walk.index_at(0.15), 1);
        assert_eq!(walk.index_at(0.39), 3);
        assert_eq!(walk.index_at(0.41), 0, "loops after the last frame");
        assert_eq!(walk.index_at(-0.05), 3, "negative time counts back");
        assert_eq!(
            walk.frame_at(0.25).get_pixel(0, 0),
            Some(Color::rgb(0, 0, 2))
        );
        assert_eq!(walk.duration(), 0.4);
        let frozen = Animation::new(numbered(4, 1).frames(1, 1), 0.0);
        assert_eq!(frozen.index_at(5.0), 0);
    }

    #[test]
    fn png_round_trip_keeps_transparency() {
        let dir = std::env::temp_dir().join(format!("duckforge-sprite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rgba.png");
        // Write a 2 x 1 RGBA PNG: one opaque red pixel, one clear pixel.
        {
            let file = std::io::BufWriter::new(File::create(&path).unwrap());
            let mut encoder = png::Encoder::new(file, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 0])
                .unwrap();
        }
        let sprite = Sprite::load_png(&path).unwrap();
        assert_eq!((sprite.width(), sprite.height()), (2, 1));
        assert_eq!(sprite.get_pixel(0, 0), Some(Color::rgb(255, 0, 0)));
        assert_eq!(sprite.get_pixel(1, 0).unwrap().a, 0);

        // A missing file and a file that isn't a PNG get one clear sentence each.
        let missing = Sprite::load_png(dir.join("nope.png"))
            .unwrap_err()
            .to_string();
        assert!(
            missing.contains("couldn't find the image file"),
            "{missing}"
        );
        let not_png = dir.join("not.png");
        std::fs::write(&not_png, b"hello").unwrap();
        let bad = Sprite::load_png(&not_png).unwrap_err().to_string();
        assert!(bad.contains("as a PNG image"), "{bad}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

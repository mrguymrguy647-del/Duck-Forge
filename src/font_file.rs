//! Text in any TrueType (`.ttf`) or OpenType (`.otf`) font, at any size,
//! with an outline and a drop shadow: captions, titles, credits.
//!
//! ```
//! # use duckforge::prelude::*;
//! # let mut canvas = Canvas::new(320, 180);
//! let font = Font::load("assets/fonts/DejaVuSerif-Bold.ttf")?;
//! let caption = TextStyle {
//!     size: 24.0,
//!     outline: 2.0,
//!     shadow: Some(vec2(2.0, 3.0)),
//!     align: Align::Center,
//!     ..TextStyle::default()
//! };
//! canvas.draw_font_text(&font, "It was watching.", vec2(160.0, 140.0), &caption);
//! # Ok::<(), duckforge::LoadError>(())
//! ```
//!
//! Letters are made from the font's curves at the size asked for, with
//! soft (antialiased) edges, by the `ab_glyph` crate. The built-in pixel
//! font ([`Canvas::draw_text`]) needs no file and stays sharp; this is for
//! text that should look like print.

use std::path::Path;

use ab_glyph::{Font as _, FontArc, GlyphId, PxScale, ScaleFont as _, point};

use crate::canvas::Canvas;
use crate::color::{self, Color};
use crate::error::LoadError;
use crate::math::{Rect, Vec2, vec2};

/// A font loaded from a `.ttf` or `.otf` file, to draw text with at any
/// size ([`Canvas::draw_font_text`]). Load it once, in your game's `new`,
/// and keep it: cloning is cheap (the file is shared, not copied).
#[derive(Clone)]
pub struct Font {
    font: FontArc,
    name: String,
    /// The last few texts' letters, outlines and shadows, ready to paint:
    /// a caption on screen for many frames is made once (only its colours
    /// are painted again).
    made: KeptTexts,
}

/// A font's texts kept ready: each with the number that says how it was
/// made (see `draw_font_text`).
type KeptTexts = std::sync::Arc<std::sync::Mutex<Vec<(u64, std::sync::Arc<TextMasks>)>>>;

/// How many texts a font keeps ready.
const KEPT_TEXTS: usize = 32;

/// A text's coverage masks, ready to paint.
struct TextMasks {
    letters: Coverage,
    outlined: Option<Coverage>,
    shadow: Option<Coverage>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Font({})", self.name)
    }
}

/// Where each line goes, sideways, from the point the text is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// The point is the left end of each line (the default).
    #[default]
    Left,
    /// The point is the middle of each line: captions.
    Center,
    /// The point is the right end of each line.
    Right,
}

/// How [`Canvas::draw_font_text`] draws: start from `TextStyle::default()`
/// (32 pixels, white, no outline or shadow, left-aligned) and change what
/// you need.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// How tall a line of text is, in pixels, from the top of the tallest
    /// letters to the bottom of the lowest (capitals are about 70% of it).
    pub size: f32,
    pub color: Color,
    /// An outline round every letter, this many pixels thick (0: none).
    /// It keeps captions readable over anything.
    pub outline: f32,
    pub outline_color: Color,
    /// A drop shadow, moved this far (right and down) from the text.
    pub shadow: Option<Vec2>,
    /// The shadow's colour; its alpha is how dark it is.
    pub shadow_color: Color,
    /// How soft the shadow is, in pixels (0: a sharp copy of the letters).
    pub shadow_blur: f32,
    pub align: Align,
    /// Lines longer than this many pixels are broken between words.
    pub wrap: Option<f32>,
    /// The gap from one line to the next, as a share of `size`: 1.2 by
    /// default.
    pub line_spacing: f32,
}

impl Default for TextStyle {
    fn default() -> TextStyle {
        TextStyle {
            size: 32.0,
            color: Color::WHITE,
            outline: 0.0,
            outline_color: Color::BLACK,
            shadow: None,
            shadow_color: Color::rgba(0, 0, 0, 170),
            shadow_blur: 0.0,
            align: Align::Left,
            wrap: None,
            line_spacing: 1.2,
        }
    }
}

impl Font {
    /// Loads a TrueType (`.ttf`) or OpenType (`.otf`) font. `assets/fonts`
    /// has two to start with (see its README).
    pub fn load(path: impl AsRef<Path>) -> Result<Font, LoadError> {
        let path = path.as_ref();
        let bytes =
            std::fs::read(path).map_err(|e| crate::sprite::missing_file_error(path, "font", &e))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Font::from_bytes(&name, bytes).map_err(|_| {
            LoadError::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "couldn't read {} as a font: Duck Forge reads TrueType (.ttf) and OpenType \
                     (.otf) fonts; a .woff or .woff2 web font needs converting to one of those first",
                    path.display()
                ),
            )
        })
    }

    /// A font from a file's bytes (`include_bytes!` a font into the game,
    /// say), called `name` in messages.
    pub fn from_bytes(name: &str, bytes: Vec<u8>) -> Result<Font, LoadError> {
        let font = FontArc::try_from_vec(bytes).map_err(|_| {
            LoadError::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} isn't a TrueType or OpenType font"),
            )
        })?;
        Ok(Font {
            font,
            name: name.to_string(),
            made: Default::default(),
        })
    }

    /// The font's name (its file's, without `.ttf`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How wide and tall `text` would be drawn with `style` (wrapped as it
    /// would be), without the outline and shadow: for placing it.
    pub fn measure(&self, text: &str, style: &TextStyle) -> Vec2 {
        let size = clean_size(style.size);
        let lines = self.lines(text, size, style.wrap);
        let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let height = if lines.is_empty() {
            0.0
        } else {
            size + (lines.len() - 1) as f32 * size * clean_spacing(style.line_spacing)
        };
        vec2(width, height)
    }

    /// The text cut into lines (at `'\n'`, and between words where a line
    /// would be wider than `wrap`), each with its width at `size`.
    fn lines(&self, text: &str, size: f32, wrap: Option<f32>) -> Vec<Line> {
        let scaled = self.font.as_scaled(PxScale::from(size));
        let width_of = |s: &str| {
            let mut width = 0.0;
            let mut last: Option<GlyphId> = None;
            for c in s.chars() {
                let id = scaled.glyph_id(c);
                if let Some(before) = last {
                    width += scaled.kern(before, id);
                }
                width += scaled.h_advance(id);
                last = Some(id);
            }
            width
        };
        let mut lines = Vec::new();
        for paragraph in text.lines() {
            let paragraph = paragraph.trim_end_matches('\r');
            let Some(limit) = wrap.filter(|w| w.is_finite() && *w > 0.0) else {
                lines.push(Line {
                    text: paragraph.to_string(),
                    width: width_of(paragraph),
                });
                continue;
            };
            let mut line = String::new();
            for word in paragraph.split(' ') {
                let tried = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && width_of(&tried) > limit {
                    let width = width_of(&line);
                    lines.push(Line {
                        text: std::mem::take(&mut line),
                        width,
                    });
                    line = word.to_string();
                } else {
                    line = tried;
                }
            }
            let width = width_of(&line);
            lines.push(Line { text: line, width });
        }
        lines
    }
}

/// One line of text and how wide it is.
struct Line {
    text: String,
    width: f32,
}

fn clean_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(1.0, 2000.0)
    } else {
        32.0
    }
}

fn clean_spacing(spacing: f32) -> f32 {
    if spacing.is_finite() {
        spacing.max(0.0)
    } else {
        1.2
    }
}

fn clean_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(0.0, 200.0)
    } else {
        0.0
    }
}

/// How much of each pixel of a picture a shape covers, 0 to 1.
struct Coverage {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Coverage {
    fn new(width: usize, height: usize) -> Coverage {
        Coverage {
            width,
            height,
            values: vec![0.0; width * height],
        }
    }

    fn get(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            0.0
        } else {
            self.values[y as usize * self.width + x as usize]
        }
    }

    /// The shape grown by `radius` pixels all round (an outline), with a
    /// soft edge.
    fn grown(&self, radius: f32) -> Coverage {
        let reach = radius.ceil() as i64 + 1;
        let mut offsets = Vec::new();
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let distance = ((dx * dx + dy * dy) as f32).sqrt();
                let weight = (radius + 0.5 - distance).clamp(0.0, 1.0);
                if weight > 0.0 {
                    offsets.push((dx, dy, weight));
                }
            }
        }
        let mut out = Coverage::new(self.width, self.height);
        for y in 0..self.height as i64 {
            for x in 0..self.width as i64 {
                let mut best = 0.0f32;
                for &(dx, dy, weight) in &offsets {
                    let v = self.get(x + dx, y + dy) * weight;
                    if v > best {
                        best = v;
                        if best >= 1.0 {
                            break;
                        }
                    }
                }
                out.values[y as usize * self.width + x as usize] = best;
            }
        }
        out
    }

    /// Blurred by about `radius` pixels (three box blurs: close to a
    /// Gaussian).
    fn blurred(&self, radius: f32) -> Coverage {
        let r = (radius / 1.7).round() as i64;
        if r < 1 {
            return Coverage {
                width: self.width,
                height: self.height,
                values: self.values.clone(),
            };
        }
        let mut now = Coverage {
            width: self.width,
            height: self.height,
            values: self.values.clone(),
        };
        for _ in 0..3 {
            for across in [true, false] {
                let mut next = Coverage::new(self.width, self.height);
                let (lines, length) = if across {
                    (self.height, self.width)
                } else {
                    (self.width, self.height)
                };
                for line in 0..lines as i64 {
                    let at = |i: i64| if across { (i, line) } else { (line, i) };
                    let mut sum = 0.0;
                    for i in -r..=r {
                        let (x, y) = at(i);
                        sum += now.get(x, y);
                    }
                    for i in 0..length as i64 {
                        let (x, y) = at(i);
                        next.values[y as usize * self.width + x as usize] =
                            sum / (2 * r + 1) as f32;
                        let (ax, ay) = at(i + r + 1);
                        let (bx, by) = at(i - r);
                        sum += now.get(ax, ay) - now.get(bx, by);
                    }
                }
                now = next;
            }
        }
        now
    }
}

impl Canvas {
    /// Draws `text` in `font`, styled by `style`: any size, with an outline
    /// and a drop shadow if asked. `pos` is the top of the first line, and
    /// its left end, middle or right end as `style.align` says. A `'\n'`
    /// starts a new line, and `style.wrap` breaks long lines between words.
    /// Returns where the text went (without its outline and shadow).
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(400, 300);
    /// let font = Font::load("assets/fonts/Inter-Bold.otf")?;
    /// let title = TextStyle { size: 48.0, outline: 3.0, align: Align::Center, ..TextStyle::default() };
    /// canvas.draw_font_text(&font, "GAME OVER", vec2(200.0, 100.0), &title);
    /// # Ok::<(), duckforge::LoadError>(())
    /// ```
    ///
    /// Its letters have soft edges, mixed into what's under them. Like the
    /// pixel font, the lines are listed in scene files and `--describe`,
    /// so tools can read the text.
    pub fn draw_font_text(
        &mut self,
        font: &Font,
        text: &str,
        pos: Vec2,
        style: &TextStyle,
    ) -> Rect {
        self.flush_3d(); // drawn over the 3D before it
        let view = self.view();
        let scale = view.scale;
        let size = clean_size(style.size * scale);
        let spacing = size * clean_spacing(style.line_spacing);
        let lines = font.lines(text, size, style.wrap.map(|w| w * scale));
        let origin = view.apply(pos);
        let widest = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let left_of = |width: f32| match style.align {
            Align::Left => origin.x,
            Align::Center => origin.x - width / 2.0,
            Align::Right => origin.x - width,
        };
        let block_left = left_of(widest);
        let height = if lines.is_empty() {
            0.0
        } else {
            size + (lines.len() - 1) as f32 * spacing
        };
        let area = Rect::new(block_left, origin.y, widest, height);
        for (i, line) in lines.iter().enumerate() {
            if line.text.trim().is_empty() {
                continue;
            }
            let top = origin.y + i as f32 * spacing;
            self.note_text(
                line.text.clone(),
                Rect::new(left_of(line.width), top, line.width, size),
            );
        }
        if lines.is_empty() || style.color.a == 0 && style.outline <= 0.0 && style.shadow.is_none()
        {
            return area;
        }

        // The letters' coverage, in a picture big enough for the outline
        // and the shadow round them.
        let outline = clean_width(style.outline * scale);
        let shadow = style.shadow.map(|s| s * scale);
        let blur = clean_width(style.shadow_blur * scale);
        let (shift_x, shift_y) = shadow.map_or((0.0, 0.0), |s| (s.x.abs(), s.y.abs()));
        let pad = (outline + blur * 2.0 + shift_x.max(shift_y)).ceil() as i64 + 2;
        let (left, top) = (area.x.floor() as i64 - pad, area.y.floor() as i64 - pad);
        let width = (area.w.ceil() as i64 + 2 * pad + 2).clamp(1, 16384) as usize;
        let height = (area.h.ceil() as i64 + 2 * pad + 2).clamp(1, 16384) as usize;
        // Made before, exactly so (the same words, size, place, outline
        // and shadow)? Then only the painting is left.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            for v in [
                size, spacing, outline, blur, origin.x, origin.y, shift_x, shift_y,
            ] {
                v.to_bits().hash(&mut hasher);
            }
            style.wrap.map(|w| (w * scale).to_bits()).hash(&mut hasher);
            (style.align as u8, shadow.is_some()).hash(&mut hasher);
            (left, top, width, height).hash(&mut hasher);
            hasher.finish()
        };
        let kept = font.made.lock().ok().and_then(|made| {
            made.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, masks)| masks.clone())
        });
        let masks = match kept {
            Some(masks) => masks,
            None => {
                let masks = std::sync::Arc::new(font.make_masks(
                    &lines,
                    size,
                    spacing,
                    origin,
                    &left_of,
                    (left, top, width, height),
                    outline,
                    shadow.map(|_| blur),
                ));
                if let Ok(mut made) = font.made.lock() {
                    if made.len() >= KEPT_TEXTS {
                        made.remove(0);
                    }
                    made.push((key, masks.clone()));
                }
                masks
            }
        };
        let (letters, outlined) = (&masks.letters, &masks.outlined);
        let shadow_mask = masks.shadow.as_ref().zip(shadow);

        // Shadow, outline, then the letters, mixed into the canvas.
        let (canvas_w, canvas_h) = (self.width() as i64, self.height() as i64);
        let pixels = self.pixels_mut();
        let mut paint = |x: i64, y: i64, color: Color, cover: f32| {
            let alpha = (cover.clamp(0.0, 1.0) * color.a as f32).round() as u8;
            if alpha == 0 || x < 0 || y < 0 || x >= canvas_w || y >= canvas_h {
                return;
            }
            let at = (y * canvas_w + x) as usize;
            pixels[at] = color::blend(pixels[at], color.to_u32(), alpha);
        };
        if let Some((mask, offset)) = shadow_mask {
            let (dx, dy) = (offset.x.round() as i64, offset.y.round() as i64);
            for y in 0..height {
                for x in 0..width {
                    let cover = mask.values[y * width + x];
                    if cover > 0.0 {
                        let (px, py) = (left + x as i64 + dx, top + y as i64 + dy);
                        paint(px, py, style.shadow_color, cover);
                    }
                }
            }
        }
        if let Some(ring) = outlined {
            for y in 0..height {
                for x in 0..width {
                    let cover = ring.values[y * width + x];
                    if cover > 0.0 {
                        paint(left + x as i64, top + y as i64, style.outline_color, cover);
                    }
                }
            }
        }
        for y in 0..height {
            for x in 0..width {
                let cover = letters.values[y * width + x];
                if cover > 0.0 {
                    paint(left + x as i64, top + y as i64, style.color, cover);
                }
            }
        }
        area
    }
}

impl Font {
    /// Draws `lines`' letters into coverage masks covering `place` (left,
    /// top, width, height), with the outline grown round them and the
    /// shadow blurred (by `shadow_blur`, if there's a shadow).
    #[allow(clippy::too_many_arguments)]
    fn make_masks(
        &self,
        lines: &[Line],
        size: f32,
        spacing: f32,
        origin: Vec2,
        left_of: &dyn Fn(f32) -> f32,
        place: (i64, i64, usize, usize),
        outline: f32,
        shadow_blur: Option<f32>,
    ) -> TextMasks {
        let font = self;
        let (left, top, width, height) = place;
        let mut letters = Coverage::new(width, height);
        let scaled = font.font.as_scaled(PxScale::from(size));
        for (i, line) in lines.iter().enumerate() {
            let baseline = origin.y + i as f32 * spacing + scaled.ascent();
            let mut x = left_of(line.width);
            let mut last: Option<GlyphId> = None;
            for c in line.text.chars() {
                let id = scaled.glyph_id(c);
                if let Some(before) = last {
                    x += scaled.kern(before, id);
                }
                let glyph = id.with_scale_and_position(size, point(x, baseline));
                x += scaled.h_advance(id);
                last = Some(id);
                let Some(outlined) = font.font.outline_glyph(glyph) else {
                    continue; // a space
                };
                let bounds = outlined.px_bounds();
                let (gx, gy) = (bounds.min.x as i64 - left, bounds.min.y as i64 - top);
                outlined.draw(|px, py, c| {
                    let (x, y) = (gx + px as i64, gy + py as i64);
                    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                        let at = y as usize * width + x as usize;
                        letters.values[at] = (letters.values[at] + c).min(1.0);
                    }
                });
            }
        }
        let outlined = (outline > 0.0).then(|| letters.grown(outline));
        let shadow = shadow_blur.map(|blur| outlined.as_ref().unwrap_or(&letters).blurred(blur));
        TextMasks {
            letters,
            outlined,
            shadow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> (Font, Font) {
        (
            Font::load("assets/fonts/DejaVuSerif-Bold.ttf").unwrap(),
            Font::load("assets/fonts/Inter-Bold.otf").unwrap(),
        )
    }

    #[test]
    fn a_caption_made_once_is_painted_the_same_every_frame() {
        let (font, _) = fonts();
        let style = |alpha: u8| TextStyle {
            size: 28.0,
            color: Color::rgba(255, 240, 200, alpha),
            outline: 2.0,
            shadow: Some(vec2(2.0, 3.0)),
            shadow_blur: 1.5,
            align: Align::Center,
            ..TextStyle::default()
        };
        let draw = |font: &Font, text: &str, alpha: u8| {
            let mut canvas = Canvas::new(320, 80);
            canvas.clear(Color::rgb(30, 40, 50));
            canvas.draw_font_text(font, text, vec2(160.0, 20.5), &style(alpha));
            canvas.pixels().to_vec()
        };
        // A caption fading in over three frames, drawn by a font that keeps
        // what it made, and by fresh fonts that make it each time.
        let fresh = || fonts().0;
        for alpha in [80, 160, 255] {
            assert_eq!(
                draw(&font, "It was watching.", alpha),
                draw(&fresh(), "It was watching.", alpha)
            );
        }
        assert_eq!(font.made.lock().unwrap().len(), 1, "made once, kept");
        // Other words are other masks.
        assert_ne!(
            draw(&font, "It was waiting.", 255),
            draw(&font, "It was watching.", 255)
        );
        assert_eq!(font.made.lock().unwrap().len(), 2);
        // Only so many are kept.
        for i in 0..40 {
            draw(&font, &format!("line {i}"), 255);
        }
        assert_eq!(font.made.lock().unwrap().len(), KEPT_TEXTS);
    }

    #[test]
    fn truetype_and_opentype_fonts_draw_at_any_size() {
        let (ttf, otf) = fonts();
        for font in [&ttf, &otf] {
            let mut sizes = Vec::new();
            for size in [12.0, 48.0, 200.0] {
                let style = TextStyle {
                    size,
                    ..TextStyle::default()
                };
                let measured = font.measure("Hello", &style);
                let mut canvas = Canvas::new(1200, 300);
                let area = canvas.draw_font_text(font, "Hello", vec2(10.0, 10.0), &style);
                assert_eq!(vec2(area.w, area.h), measured);
                let lit = canvas.pixels().iter().filter(|&&p| p != 0).count();
                assert!(lit > 0, "{} at {size}", font.name());
                // Soft edges: some pixels only partly covered.
                let soft = canvas
                    .pixels()
                    .iter()
                    .filter(|&&p| p != 0 && p != 0xffffff)
                    .count();
                assert!(soft > 0, "antialiased");
                sizes.push((measured.x, lit));
            }
            // Bigger text is wider and covers more.
            assert!(
                sizes[0].0 < sizes[1].0 && sizes[1].0 < sizes[2].0,
                "{sizes:?}"
            );
            assert!(
                sizes[0].1 < sizes[1].1 && sizes[1].1 < sizes[2].1,
                "{sizes:?}"
            );
        }
    }

    #[test]
    fn an_outline_and_a_shadow_surround_the_letters() {
        let (font, _) = fonts();
        let draw = |style: TextStyle| {
            let mut canvas = Canvas::new(300, 100);
            canvas.clear(Color::rgb(0, 0, 255));
            canvas.draw_font_text(&font, "I", vec2(150.0, 20.0), &style);
            canvas
        };
        let plain = TextStyle {
            size: 60.0,
            color: Color::rgb(255, 255, 255),
            align: Align::Center,
            ..TextStyle::default()
        };
        let count = |c: &Canvas, want: u32| c.pixels().iter().filter(|&&p| p == want).count();
        let base = draw(plain);
        assert_eq!(count(&base, 0x000000), 0, "no black yet");
        let outlined = draw(TextStyle {
            outline: 3.0,
            outline_color: Color::rgb(0, 0, 0),
            ..plain
        });
        assert!(count(&outlined, 0x000000) > 100, "a black ring");
        assert_eq!(
            count(&outlined, 0xffffff),
            count(&base, 0xffffff),
            "the letters themselves unchanged"
        );
        // The shadow: below and right of the letter, darker blue.
        let shadowed = draw(TextStyle {
            shadow: Some(vec2(6.0, 6.0)),
            shadow_color: Color::rgba(0, 0, 0, 255),
            ..plain
        });
        let darker = |c: &Canvas| c.pixels().iter().filter(|&&p| p & 0xff < 255).count();
        assert!(darker(&shadowed) > 100);
        let soft = draw(TextStyle {
            shadow: Some(vec2(6.0, 6.0)),
            shadow_color: Color::rgba(0, 0, 0, 255),
            shadow_blur: 6.0,
            ..plain
        });
        assert!(
            darker(&soft) > darker(&shadowed),
            "a blurred shadow spreads wider"
        );
    }

    #[test]
    fn long_lines_wrap_between_words_and_alignment_moves_them() {
        let (_, font) = fonts();
        let style = TextStyle {
            size: 20.0,
            wrap: Some(120.0),
            ..TextStyle::default()
        };
        let one = font.measure("Something", &style);
        let three = font.measure("Something is down here", &style);
        assert!(
            three.x <= 120.0 && three.y > one.y * 2.0,
            "{one:?} {three:?}"
        );
        // Centred on x = 100, the text's middle is at 100.
        let mut canvas = Canvas::new(200, 60);
        let centered = TextStyle {
            align: Align::Center,
            ..style
        };
        let area = canvas.draw_font_text(&font, "Down", vec2(100.0, 10.0), &centered);
        assert!((area.x + area.w / 2.0 - 100.0).abs() < 0.01, "{area:?}");
        let right = TextStyle {
            align: Align::Right,
            ..style
        };
        let area = canvas.draw_font_text(&font, "Down", vec2(190.0, 10.0), &right);
        assert!((area.x + area.w - 190.0).abs() < 0.01, "{area:?}");
        // Listed for tools, like the pixel font's text.
        let (_, texts) = canvas.take_scene();
        assert_eq!(texts.len(), 2);
        assert_eq!(texts[0].text, "Down");
    }

    #[test]
    fn a_file_that_isnt_a_font_says_so() {
        let dir = crate::test_folder::TestFolder::new("not-a-font");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("caption.ttf");
        std::fs::write(&path, b"not a font at all").unwrap();
        let error = Font::load(&path).unwrap_err().to_string();
        assert!(
            error.contains("TrueType (.ttf) and OpenType (.otf)"),
            "{error}"
        );
        let missing = Font::load(dir.join("nope.otf")).unwrap_err().to_string();
        assert!(missing.contains("couldn't find the font file"), "{missing}");
    }
}

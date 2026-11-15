//! Pixel fonts: letters drawn as pictures, from a tileset (a Duck Quill
//! sprite sheet) or a grid of letters, kept sharp at any whole-number size.
//! With a drop shadow, a box behind, lines broken to a width, centred, and
//! colours inside the text: `"^rRED^w or ^bBLUE"`.
//!
//! ```
//! # use duckforge::prelude::*;
//! # let mut canvas = Canvas::new(200, 100);
//! // let tiles = Tileset::load("assets/font.json")?;
//! // let font = BitmapFont::from_tileset(&tiles, &[('A', "a"), ('B', "b"), ...]);
//! let font = BitmapFont::built_in(); // the engine's own 3 x 5 letters
//! let style = BitmapStyle {
//!     scale: 3,
//!     shadow: Some((1, 1)),
//!     background: Some(Color::rgba(0, 0, 0, 160)),
//!     align: Align::Center,
//!     wrap: Some(180.0),
//!     ..BitmapStyle::default()
//! };
//! canvas.draw_bitmap_text(&font, "PICK ^rRED^w OR ^bBLUE", vec2(100.0, 20.0), &style);
//! ```
//!
//! Colour codes: `^r` red, `^g` green, `^b` blue, `^y` yellow, `^o`
//! orange, `^p` purple, `^c` cyan, `^w` white, `^k` black, `^e` grey,
//! `^#rrggbb` any colour, `^0` back to the style's colour, and `^^` a `^`.
//! They're left out of what [`--describe`](crate::cli) and scene files
//! say the text is. A letter's own colours are multiplied by the colour,
//! so white letters take it exactly.

use std::collections::HashMap;
use std::path::Path;

use crate::canvas::Canvas;
use crate::color::{self, Color};
use crate::error::LoadError;
use crate::font_file::Align;
use crate::math::{Rect, Vec2, vec2};
use crate::sprite::Sprite;
use crate::tileset::Tileset;

/// One letter: its picture and the columns of it that are drawn.
#[derive(Debug, Clone)]
struct BitmapGlyph {
    picture: Sprite,
    /// The first column drawn, and how many (all of them, unless the font
    /// is [`proportional`](BitmapFont::proportional)).
    left: usize,
    width: usize,
}

/// A pixel font: a picture for each letter. See the [module docs](self).
/// Cloning is cheap (the pictures are shared).
#[derive(Debug, Clone)]
pub struct BitmapFont {
    glyphs: HashMap<char, BitmapGlyph>,
    /// The tallest letter: the height of a line.
    height: usize,
    /// How wide a space is, if the font has no picture for it.
    space: usize,
}

/// How [`Canvas::draw_bitmap_text`] draws: start from
/// `BitmapStyle::default()` (scale 1, white, nothing else) and change what
/// you need.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BitmapStyle {
    /// Canvas pixels per font pixel: 1, 2, 3... (whole numbers keep it sharp).
    pub scale: u32,
    pub color: Color,
    /// A drop shadow, this many font pixels right and down (`(1, 1)`).
    pub shadow: Option<(i32, i32)>,
    /// The shadow's colour; its alpha is how dark it is.
    pub shadow_color: Color,
    /// A box behind the text, this colour (its alpha says how see-through).
    pub background: Option<Color>,
    /// Font pixels between the text and the edge of the box.
    pub padding: u32,
    pub align: Align,
    /// Lines longer than this many pixels are broken between words.
    pub wrap: Option<f32>,
    /// Font pixels between the letters, and between the lines.
    pub letter_spacing: u32,
    pub line_spacing: u32,
    /// Read `^r`-style colour codes (on unless you say).
    pub colour_codes: bool,
}

impl Default for BitmapStyle {
    fn default() -> BitmapStyle {
        BitmapStyle {
            scale: 1,
            color: Color::WHITE,
            shadow: None,
            shadow_color: Color::rgba(0, 0, 0, 170),
            background: None,
            padding: 2,
            align: Align::Left,
            wrap: None,
            letter_spacing: 1,
            line_spacing: 2,
            colour_codes: true,
        }
    }
}

impl BitmapFont {
    /// A font from `tileset`'s tiles: `glyphs` says which tile each letter
    /// is, like `&[('A', "a"), ('B', "b"), ('!', "bang")]`. A letter the
    /// font hasn't got is drawn as the other case of it if it has that
    /// (lowercase as capitals), else as `?`, else as a space.
    ///
    /// # Panics
    ///
    /// If a tile isn't in the tileset (the message lists the ones it has).
    #[track_caller]
    pub fn from_tileset(tileset: &Tileset, glyphs: &[(char, &str)]) -> BitmapFont {
        match BitmapFont::try_from_tileset(tileset, glyphs) {
            Ok(font) => font,
            Err(message) => panic!("BitmapFont::from_tileset: {message}"),
        }
    }

    fn try_from_tileset(tileset: &Tileset, glyphs: &[(char, &str)]) -> Result<BitmapFont, String> {
        let mut pictures = Vec::new();
        for &(letter, tile) in glyphs {
            let Some(animation) = tileset.get(tile) else {
                let mut names: Vec<&str> = tileset.names().collect();
                names.sort_unstable();
                return Err(format!(
                    "the letter {letter:?} is the tile {tile:?}, which the tileset hasn't got; it has: {}",
                    names.join(", ")
                ));
            };
            pictures.push((letter, animation.frame_at(0.0).clone()));
        }
        Ok(BitmapFont::from_pictures(pictures))
    }

    /// [`from_tileset`](BitmapFont::from_tileset), loading the tileset
    /// (a Duck Quill `.json` sprite sheet) first.
    pub fn load(path: impl AsRef<Path>, glyphs: &[(char, &str)]) -> Result<BitmapFont, LoadError> {
        let path = path.as_ref();
        let tileset = Tileset::load(path)?;
        BitmapFont::try_from_tileset(&tileset, glyphs).map_err(|message| {
            LoadError::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: {message}", path.display()),
            )
        })
    }

    /// A font from a picture of letters in a grid, each `width` x `height`
    /// pixels, in the order of `letters` (left to right, then the next
    /// row): `BitmapFont::from_grid(&sheet, 8, 8, "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")`.
    pub fn from_grid(sheet: &Sprite, width: usize, height: usize, letters: &str) -> BitmapFont {
        let pictures = letters
            .chars()
            .zip(sheet.frames(width.max(1), height.max(1)))
            .collect();
        BitmapFont::from_pictures(pictures)
    }

    /// The engine's own pixel font ([`Canvas::draw_text`]'s): 3 x 5
    /// capitals, every printable ASCII character.
    pub fn built_in() -> BitmapFont {
        let pictures = (' '..='~')
            .map(|c| {
                let glyph = crate::font::glyph(c);
                let (w, h) = (crate::font::GLYPH_WIDTH, crate::font::GLYPH_ROWS);
                let mut pixels = vec![Color::rgba(0, 0, 0, 0); w * h];
                for row in 0..h {
                    for col in 0..w {
                        if crate::font::is_lit(&glyph, col, row) {
                            pixels[row * w + col] = Color::WHITE;
                        }
                    }
                }
                (c, Sprite::from_pixels(w, h, pixels))
            })
            .collect();
        let mut font = BitmapFont::from_pictures(pictures);
        // A line is as tall as a capital, as draw_text's is; the tails of
        // g j p q y hang below it.
        font.height = crate::font::GLYPH_HEIGHT;
        font
    }

    fn from_pictures(pictures: Vec<(char, Sprite)>) -> BitmapFont {
        let height = pictures.iter().map(|(_, p)| p.height()).max().unwrap_or(0);
        let widest = pictures.iter().map(|(_, p)| p.width()).max().unwrap_or(0);
        let glyphs = pictures
            .into_iter()
            .map(|(c, picture)| {
                let width = picture.width();
                (
                    c,
                    BitmapGlyph {
                        picture,
                        left: 0,
                        width,
                    },
                )
            })
            .collect();
        BitmapFont {
            glyphs,
            height,
            space: widest.div_ceil(2).max(1),
        }
    }

    /// Each letter only as wide as its pixels, not its whole tile: an `i`
    /// narrower than an `m`. (A space keeps its width.)
    pub fn proportional(mut self) -> BitmapFont {
        for (&c, glyph) in &mut self.glyphs {
            if c == ' ' {
                continue;
            }
            let (w, h) = (glyph.picture.width(), glyph.picture.height());
            let used = |x: usize| (0..h).any(|y| glyph.picture.texel(x, y).1 >= 128);
            if let (Some(first), Some(last)) =
                ((0..w).find(|&x| used(x)), (0..w).rev().find(|&x| used(x)))
            {
                glyph.left = first;
                glyph.width = last + 1 - first;
            }
        }
        self
    }

    /// How tall a line is, in font pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// The picture for `c`, if the font can draw it.
    fn glyph(&self, c: char) -> Option<&BitmapGlyph> {
        self.glyphs
            .get(&c)
            .or_else(|| c.to_uppercase().next().and_then(|u| self.glyphs.get(&u)))
            .or_else(|| c.to_lowercase().next().and_then(|l| self.glyphs.get(&l)))
            .or_else(|| (c != ' ').then(|| self.glyphs.get(&'?')).flatten())
    }

    /// How far `c` moves the pen, in font pixels (before letter spacing).
    fn advance(&self, c: char) -> usize {
        match (c, self.glyph(c)) {
            (' ', None) => self.space,
            (_, Some(glyph)) => glyph.width,
            (_, None) => self.space,
        }
    }

    /// `line`'s width in font pixels.
    fn width_of(&self, line: &[(char, Color)], spacing: usize) -> usize {
        let letters: usize = line.iter().map(|&(c, _)| self.advance(c)).sum();
        letters + spacing * line.len().saturating_sub(1)
    }

    /// The text's lines, each letter with its colour, broken to `wrap`
    /// font pixels.
    fn lines(
        &self,
        text: &str,
        style: &BitmapStyle,
        wrap: Option<usize>,
    ) -> Vec<Vec<(char, Color)>> {
        let spacing = style.letter_spacing as usize;
        let mut lines = Vec::new();
        let mut colour = style.color;
        for written in text.split('\n') {
            let letters = coloured(written.trim_end_matches('\r'), style, &mut colour);
            let Some(wrap) = wrap else {
                lines.push(letters);
                continue;
            };
            // Words (and the spaces before them) onto lines no wider than wrap.
            let mut line: Vec<(char, Color)> = Vec::new();
            let mut word: Vec<(char, Color)> = Vec::new();
            let flush_word = |line: &mut Vec<(char, Color)>,
                              word: &mut Vec<(char, Color)>,
                              lines: &mut Vec<_>| {
                if word.is_empty() {
                    return;
                }
                let mut joined = line.clone();
                if !joined.is_empty() {
                    joined.push((' ', colour));
                }
                joined.extend_from_slice(word);
                if !line.is_empty() && self.width_of(&joined, spacing) > wrap {
                    lines.push(std::mem::take(line));
                    *line = std::mem::take(word);
                } else {
                    *line = joined;
                    word.clear();
                }
            };
            for &(c, k) in &letters {
                if c == ' ' {
                    flush_word(&mut line, &mut word, &mut lines);
                } else {
                    word.push((c, k));
                }
            }
            flush_word(&mut line, &mut word, &mut lines);
            lines.push(line);
        }
        lines
    }

    /// How wide and tall `text` would be drawn with `style`, in canvas
    /// pixels (without the box and the shadow): for placing it.
    pub fn measure(&self, text: &str, style: &BitmapStyle) -> Vec2 {
        let scale = style.scale.max(1) as usize;
        let wrap = style.wrap.map(|w| (w / scale as f32).max(1.0) as usize);
        let lines = self.lines(text, style, wrap);
        let spacing = style.letter_spacing as usize;
        let widest = lines
            .iter()
            .map(|l| self.width_of(l, spacing))
            .max()
            .unwrap_or(0);
        let height = self.block_height(lines.len(), style);
        vec2((widest * scale) as f32, (height * scale) as f32)
    }

    fn block_height(&self, lines: usize, style: &BitmapStyle) -> usize {
        if lines == 0 {
            0
        } else {
            self.height * lines + style.line_spacing as usize * (lines - 1)
        }
    }
}

/// `text`'s letters with their colours, colour codes read (if the style
/// reads them). `colour` is the colour at the start, and at the end.
fn coloured(text: &str, style: &BitmapStyle, colour: &mut Color) -> Vec<(char, Color)> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c != '^' || !style.colour_codes {
            out.push((c, *colour));
            continue;
        }
        let Some(&(_, code)) = chars.peek() else {
            out.push((c, *colour));
            continue;
        };
        let named = match code {
            'r' => Some(Color::RED),
            'g' => Some(Color::GREEN),
            'b' => Some(Color::BLUE),
            'y' => Some(Color::YELLOW),
            'o' => Some(Color::ORANGE),
            'p' => Some(Color::PURPLE),
            'c' => Some(Color::rgb(80, 210, 230)),
            'w' => Some(Color::WHITE),
            'k' => Some(Color::BLACK),
            'e' => Some(Color::GRAY),
            '0' => Some(style.color),
            _ => None,
        };
        if let Some(named) = named {
            *colour = named.with_alpha(style.color.a);
            chars.next();
        } else if code == '^' {
            out.push(('^', *colour));
            chars.next();
        } else if code == '#' {
            match text
                .get(at + 2..at + 8)
                .and_then(|hex| Color::from_hex_str(&format!("#{hex}")))
            {
                Some(any) => {
                    *colour = any.with_alpha(style.color.a);
                    for _ in 0..7 {
                        chars.next();
                    }
                }
                None => out.push((c, *colour)),
            }
        } else {
            out.push((c, *colour));
        }
    }
    out
}

impl Canvas {
    /// Draws `text` in the pixel font `font`, styled by `style` (see
    /// [`BitmapStyle`] and the [module docs](crate::bitmap_font)). `pos` is
    /// the top of the first line, and its left end, middle or right end as
    /// `style.align` says. A `'\n'` starts a new line. Returns where the
    /// text went, box and all.
    pub fn draw_bitmap_text(
        &mut self,
        font: &BitmapFont,
        text: &str,
        pos: Vec2,
        style: &BitmapStyle,
    ) -> Rect {
        self.flush_3d(); // drawn over the 3D before it
        let view = self.view();
        let s = ((style.scale.max(1) as f32 * view.scale).round() as usize).max(1);
        let wrap = style
            .wrap
            .map(|w| ((w * view.scale) / s as f32).max(1.0) as usize);
        let lines = font.lines(text, style, wrap);
        let spacing = style.letter_spacing as usize;
        let origin = view.apply(pos);
        let widths: Vec<usize> = lines
            .iter()
            .map(|l| font.width_of(l, spacing) * s)
            .collect();
        let widest = widths.iter().copied().max().unwrap_or(0);
        let left_of = |width: usize| match style.align {
            Align::Left => origin.x.round() as i64,
            Align::Center => (origin.x - width as f32 / 2.0).round() as i64,
            Align::Right => (origin.x - width as f32).round() as i64,
        };
        let top = origin.y.round() as i64;
        let line_step = (font.height + style.line_spacing as usize) * s;
        let height = font.block_height(lines.len(), style) * s;
        let pad = if style.background.is_some() {
            (style.padding as usize * s) as i64
        } else {
            0
        };
        let block_left = left_of(widest);
        let area = Rect::new(
            (block_left - pad) as f32,
            (top - pad) as f32,
            (widest as i64 + 2 * pad) as f32,
            (height as i64 + 2 * pad) as f32,
        );
        for (i, line) in lines.iter().enumerate() {
            let words: String = line.iter().map(|&(c, _)| c).collect();
            if !words.trim().is_empty() {
                let at = Rect::new(
                    left_of(widths[i]) as f32,
                    (top + (i * line_step) as i64) as f32,
                    widths[i] as f32,
                    (font.height * s) as f32,
                );
                self.note_text(words, at);
            }
        }
        let (cw, ch) = (self.width() as i64, self.height() as i64);
        let pixels = self.pixels_mut();
        let mut fill = |x: i64, y: i64, w: i64, h: i64, rgb: u32, alpha: u8| {
            if alpha == 0 {
                return;
            }
            let (x0, y0, x1, y1) = (x.max(0), y.max(0), (x + w).min(cw), (y + h).min(ch));
            for py in y0..y1 {
                let row = &mut pixels[(py * cw + x0) as usize..(py * cw + x1.max(x0)) as usize];
                for p in row {
                    *p = if alpha == 255 {
                        rgb
                    } else {
                        color::blend(*p, rgb, alpha)
                    };
                }
            }
        };
        if let Some(back) = style.background {
            fill(
                area.x as i64,
                area.y as i64,
                area.w as i64,
                area.h as i64,
                back.to_u32(),
                back.a,
            );
        }
        // The shadow, then the letters, a font pixel at a time.
        let passes: &[Option<(i32, i32)>] = &[style.shadow, Some((0, 0))];
        for (pass, offset) in passes.iter().enumerate() {
            let Some((dx, dy)) = *offset else { continue };
            let shadow = pass == 0;
            for (i, line) in lines.iter().enumerate() {
                let mut x = left_of(widths[i]);
                let y = top + (i * line_step) as i64;
                for &(c, colour) in line {
                    if let Some(glyph) = font.glyph(c) {
                        let picture = &glyph.picture;
                        for gy in 0..picture.height() {
                            for gx in 0..glyph.width {
                                let (rgb, alpha) = picture.texel(glyph.left + gx, gy);
                                if alpha < 128 {
                                    continue;
                                }
                                let (paint, a) = if shadow {
                                    (style.shadow_color.to_u32(), style.shadow_color.a)
                                } else {
                                    let tint = |shift: u32, by: u8| {
                                        (((rgb >> shift) & 0xff) * by as u32 + 127) / 255
                                    };
                                    let rgb = (tint(16, colour.r) << 16)
                                        | (tint(8, colour.g) << 8)
                                        | tint(0, colour.b);
                                    (rgb, colour.a)
                                };
                                let (px, py) = (
                                    x + (gx * s) as i64 + dx as i64 * s as i64,
                                    y + (gy * s) as i64 + dy as i64 * s as i64,
                                );
                                fill(px, py, s as i64, s as i64, paint, a);
                            }
                        }
                    }
                    x += ((font.advance(c) + spacing) * s) as i64;
                }
            }
        }
        area
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny font: "A" a 2 x 3 block, "I" one column (in a 3-wide tile),
    /// as tiles of a tileset.
    fn tiny() -> BitmapFont {
        let clear = Color::rgba(0, 0, 0, 0);
        let w = Color::WHITE;
        #[rustfmt::skip]
        let pixels = vec![
            w, w, clear,   clear, w, clear,
            w, w, clear,   clear, w, clear,
            w, w, clear,   clear, w, clear,
        ];
        let sheet = Sprite::from_pixels(6, 3, pixels);
        let tiles = Tileset::grid(&sheet, 3, 3, &["letter_a", "letter_i"]);
        BitmapFont::from_tileset(&tiles, &[('A', "letter_a"), ('I', "letter_i")])
    }

    fn lit(canvas: &Canvas, color: Color) -> usize {
        canvas
            .pixels()
            .iter()
            .filter(|&&p| p == color.to_u32())
            .count()
    }

    #[test]
    fn letters_come_from_the_tileset_at_any_whole_size() {
        let font = tiny();
        let mut canvas = Canvas::new(40, 20);
        canvas.clear(Color::BLACK);
        let style = BitmapStyle {
            scale: 2,
            ..BitmapStyle::default()
        };
        let area = canvas.draw_bitmap_text(&font, "AI", vec2(1.0, 1.0), &style);
        // A: 2 x 3 font pixels, I: 1 x 3, each 2 x 2 canvas pixels.
        assert_eq!(lit(&canvas, Color::WHITE), (6 + 3) * 4);
        // Tiles are 3 wide, with 1 between letters: 3 + 1 + 3.
        assert_eq!(area, Rect::new(1.0, 1.0, 14.0, 6.0));
        assert_eq!(font.measure("AI", &style), vec2(14.0, 6.0));
        // Proportional: A is 2 wide, I is 1.
        let narrow = tiny().proportional();
        assert_eq!(narrow.measure("AI", &style), vec2(8.0, 6.0));
        // Lowercase is drawn as capitals; unknown letters as spaces here.
        assert_eq!(font.measure("ai", &style), font.measure("AI", &style));
        // And it's in the scene's text, for --describe.
        assert!(canvas.take_scene().1.iter().any(|t| t.text == "AI"));
    }

    #[test]
    #[should_panic(expected = "hasn't got; it has: letter_a, letter_i")]
    fn a_missing_tile_says_which_there_are() {
        let tiles = Tileset::grid(
            &Sprite::from_pixels(6, 3, vec![Color::WHITE; 18]),
            3,
            3,
            &["letter_a", "letter_i"],
        );
        let _ = BitmapFont::from_tileset(&tiles, &[('B', "letter_b")]);
    }

    #[test]
    fn colour_codes_change_the_colour_and_are_not_drawn() {
        let font = tiny();
        let mut canvas = Canvas::new(60, 10);
        canvas.clear(Color::BLACK);
        let style = BitmapStyle::default();
        canvas.draw_bitmap_text(&font, "^rA^wI^#00ff00A^0I^^", vec2(0.0, 0.0), &style);
        assert_eq!(lit(&canvas, Color::RED), 6, "the first A");
        assert_eq!(lit(&canvas, Color::from_hex(0x00ff00)), 6, "the second A");
        assert_eq!(lit(&canvas, Color::WHITE), 6, "both I's");
        let said = canvas.take_scene().1;
        assert_eq!(said[0].text, "AIAI^", "the codes are left out; ^^ is a ^");
        // Turned off, the codes are letters (drawn as spaces here).
        let plain = BitmapStyle {
            colour_codes: false,
            ..style
        };
        assert!(font.measure("^rA", &plain).x > font.measure("^rA", &style).x);
    }

    #[test]
    fn shadow_box_wrap_and_centring() {
        let font = tiny();
        let mut canvas = Canvas::new(40, 40);
        canvas.clear(Color::BLACK);
        let style = BitmapStyle {
            shadow: Some((1, 1)),
            shadow_color: Color::rgb(0, 0, 255),
            background: Some(Color::rgb(60, 60, 60)),
            padding: 1,
            align: Align::Center,
            wrap: Some(8.0),
            ..BitmapStyle::default()
        };
        // "AI AI" is 11 wide: wrapped at 8, two lines of "AI" (7 wide).
        let area = canvas.draw_bitmap_text(&font, "AI AI", vec2(20.0, 5.0), &style);
        let said: Vec<String> = canvas.take_scene().1.into_iter().map(|t| t.text).collect();
        assert_eq!(said, ["AI", "AI"]);
        // Centred on x = 20: 7 wide from 16.5, rounded; the box one pixel
        // round two lines of 3 with 2 between.
        assert_eq!(area, Rect::new(16.0, 4.0, 9.0, 10.0));
        assert_eq!(
            canvas.get_pixel(16, 4),
            Some(Color::rgb(60, 60, 60)),
            "the box's corner"
        );
        assert_eq!(canvas.get_pixel(17, 5), Some(Color::WHITE), "the first A");
        // The shadow peeks out right of and below the A.
        assert_eq!(canvas.get_pixel(19, 8), Some(Color::rgb(0, 0, 255)));
        assert_eq!(
            canvas.get_pixel(15, 5),
            Some(Color::BLACK),
            "nothing outside the box"
        );
    }

    #[test]
    fn the_built_in_font_draws_as_draw_text_does() {
        let font = BitmapFont::built_in();
        let mut a = Canvas::new(80, 20);
        let mut b = Canvas::new(80, 20);
        a.clear(Color::BLACK);
        b.clear(Color::BLACK);
        let style = BitmapStyle {
            scale: 2,
            color: Color::YELLOW,
            ..BitmapStyle::default()
        };
        a.draw_bitmap_text(&font, "Hi, duck!", vec2(2.0, 3.0), &style);
        b.draw_text("Hi, duck!", vec2(2.0, 3.0), 2, Color::YELLOW);
        assert_eq!(a.pixels(), b.pixels());
        assert_eq!(
            font.measure("Hi, duck!", &style).x,
            Canvas::text_width("Hi, duck!", 2)
        );
    }
}

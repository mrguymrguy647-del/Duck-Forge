//! Making pictures out of a run: shrinking and enlarging frames, a contact
//! sheet of many frames in a grid, and an animated GIF.

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use crate::camera::Camera2D;
use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::{Rect, Vec2, vec2};

/// A part of the picture: `width` x `height` pixels, starting at `x`, `y`
/// (the top-left corner).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Crop {
    /// Reads `x,y,w,h`, like `--crop 100,50,40,30`.
    pub fn parse(text: &str) -> Result<Crop, String> {
        let numbers: Vec<usize> = text
            .split(',')
            .map(|part| part.trim().parse())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("--crop needs four whole numbers x,y,w,h, not {text:?}"))?;
        let [x, y, width, height] = numbers[..] else {
            return Err(format!(
                "--crop needs four whole numbers x,y,w,h, not {text:?}"
            ));
        };
        if width == 0 || height == 0 {
            return Err(String::from(
                "--crop needs a width and height of at least 1",
            ));
        }
        Ok(Crop {
            x,
            y,
            width,
            height,
        })
    }

    /// Checks the crop fits inside a canvas of this size.
    pub(crate) fn check(&self, width: usize, height: usize) -> Result<(), String> {
        let fits = self
            .x
            .checked_add(self.width)
            .is_some_and(|right| right <= width)
            && self
                .y
                .checked_add(self.height)
                .is_some_and(|bottom| bottom <= height);
        if fits {
            Ok(())
        } else {
            Err(format!(
                "the crop {},{},{},{} doesn't fit inside the {width} x {height} canvas",
                self.x, self.y, self.width, self.height
            ))
        }
    }
}

/// A copy of `canvas` with a light grid every `every` units, and the
/// coordinates written along the bottom and left edges (most games put
/// their score along the top), so positions can be read off the picture.
///
/// With `camera` (`--grid-world N`), the lines are at world coordinates,
/// placed through the camera, and numbered in world units; without it
/// (`--grid N`) they're every `every` screen pixels. Nothing is drawn over
/// `keep_clear` (the debug overlay's box), so both stay readable.
pub(crate) fn with_grid(
    canvas: &Canvas,
    every: usize,
    camera: Option<&Camera2D>,
    keep_clear: Option<Rect>,
) -> Canvas {
    let mut picture = canvas.copy_picture();
    let every = every.max(2) as f32;
    let (width, height) = (canvas.width() as f32, canvas.height() as f32);
    let to_screen = |p: Vec2| camera.map_or(p, |c| c.world_to_screen(p));
    let to_world = |p: Vec2| camera.map_or(p, |c| c.screen_to_world(p));
    let (top_left, bottom_right) = (to_world(Vec2::ZERO), to_world(vec2(width, height)));
    // How far apart neighbouring lines are on screen, to space the numbers
    // at least 24 pixels apart.
    let spacing = (to_screen(vec2(every, 0.0)).x - to_screen(Vec2::ZERO).x)
        .abs()
        .max(0.001);
    let label_step = (24.0 / spacing).ceil().max(1.0) as i64;
    // The multiples of `every` that are on screen, along one axis.
    let lines = |from: f32, to: f32| -> Vec<i64> {
        let (first, last) = ((from / every).ceil() as i64, (to / every).floor() as i64);
        (first..=last.min(first + 10_000)).collect()
    };

    let line = Color::rgba(255, 255, 255, 60);
    let xs = lines(top_left.x, bottom_right.x);
    let ys = lines(top_left.y, bottom_right.y);
    for &k in &xs {
        let x = to_screen(vec2(k as f32 * every, 0.0)).x.round();
        if x >= 1.0 {
            picture.fill_rect(Rect::new(x, 0.0, 1.0, height), line);
        }
    }
    for &k in &ys {
        let y = to_screen(vec2(0.0, k as f32 * every)).y.round();
        if y >= 1.0 {
            picture.fill_rect(Rect::new(0.0, y, width, 1.0), line);
        }
    }
    // Numbers on some of the lines, on a dark see-through patch.
    let mut number = |text: String, x: f32, y: f32| {
        let patch = Rect::new(x, y, Canvas::text_width(&text, 1) + 2.0, 7.0);
        if keep_clear.is_some_and(|clear| clear.overlaps(&patch)) || x < 0.0 || y < 0.0 {
            return;
        }
        picture.fill_rect(patch, Color::rgba(0, 0, 0, 150));
        picture.draw_text(&text, vec2(x + 1.0, y + 1.0), 1, Color::WHITE);
    };
    let bottom = height - 7.0;
    for &k in xs.iter().filter(|k| k.rem_euclid(label_step) == 0) {
        let x = to_screen(vec2(k as f32 * every, 0.0)).x.round();
        if x >= 1.0 {
            number(format!("{}", (k as f32 * every).round()), x + 1.0, bottom);
        }
    }
    for &k in ys.iter().filter(|k| k.rem_euclid(label_step) == 0) {
        let y = to_screen(vec2(0.0, k as f32 * every)).y.round();
        if y >= 1.0 && y < bottom - 7.0 {
            number(format!("{}", (k as f32 * every).round()), 0.0, y + 1.0);
        }
    }
    // Put the debug overlay's box back exactly as it was: no lines over it.
    if let Some(clear) = keep_clear {
        let (x0, y0) = (clear.x.max(0.0) as i32, clear.y.max(0.0) as i32);
        let (x1, y1) = (
            clear.right().min(width) as i32,
            clear.bottom().min(height) as i32,
        );
        for y in y0..y1 {
            for x in x0..x1 {
                if let Some(color) = canvas.get_pixel(x, y) {
                    picture.set_pixel(x, y, color);
                }
            }
        }
    }
    picture
}

/// The `crop` part of `canvas`, enlarged `zoom` times: every pixel becomes a
/// `zoom` x `zoom` square, so you can see exactly which pixels are lit.
pub(crate) fn close_up(canvas: &Canvas, crop: Crop, zoom: usize) -> Canvas {
    let zoom = zoom.max(1);
    let (width, height) = (crop.width * zoom, crop.height * zoom);
    let source = canvas.pixels();
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = (crop.y + y / zoom) * canvas.width();
        for x in 0..width {
            pixels.push(source[row + crop.x + x / zoom]);
        }
    }
    Canvas::from_pixels(width, height, pixels)
}

/// `canvas`, made `factor` times smaller. Each new pixel is the average of
/// a `factor` x `factor` block, so thin lines fade instead of vanishing.
pub(crate) fn shrink(canvas: &Canvas, factor: usize) -> Canvas {
    if factor <= 1 {
        return canvas.copy_picture();
    }
    let width = (canvas.width() / factor).max(1);
    let height = (canvas.height() / factor).max(1);
    let source = canvas.pixels();
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 3];
            let mut count = 0;
            for sy in y * factor..((y + 1) * factor).min(canvas.height()) {
                for sx in x * factor..((x + 1) * factor).min(canvas.width()) {
                    let [_, r, g, b] = source[sy * canvas.width() + sx].to_be_bytes();
                    sum[0] += r as u32;
                    sum[1] += g as u32;
                    sum[2] += b as u32;
                    count += 1;
                }
            }
            let count = count.max(1);
            let [r, g, b] = sum.map(|channel| (channel / count) as u8);
            pixels.push(u32::from_be_bytes([0, r, g, b]));
        }
    }
    Canvas::from_pixels(width, height, pixels)
}

/// How many times smaller to make a frame so it's at most `max_width` wide.
pub(crate) fn shrink_factor(width: usize, max_width: usize) -> usize {
    width.div_ceil(max_width.max(1)).max(1)
}

const SHEET_BACKGROUND: Color = Color::from_hex(0x30_30_38);
const SHEET_GAP: usize = 4;
const LABEL_SCALE: u32 = 2;
/// The label strip under each frame: 10-pixel text plus a little space.
const LABEL_HEIGHT: usize = 14;
/// Frames in a sheet are shrunk to at most this wide.
const SHEET_CELL_WIDTH: usize = 320;
/// The caption line under a frame's number (`--sheet-events`).
const CAPTION_HEIGHT: usize = 9;
const CAPTION_COLOR: Color = Color::from_hex(0xff_e0_60);
/// More frames than this in one sheet would be too small to see.
pub(crate) const SHEET_MAX_FRAMES: usize = 64;

/// A grid of frames, each with its frame number underneath, to see a
/// whole run at a glance.
#[cfg(test)]
pub(crate) fn contact_sheet(frames: &[(u64, Canvas)], updates_per_second: f32) -> Canvas {
    let captioned: Vec<(u64, Canvas, String)> = frames
        .iter()
        .map(|(frame, picture)| (*frame, picture.copy_picture(), String::new()))
        .collect();
    contact_sheet_with_captions(&captioned, updates_per_second)
}

/// A contact sheet with a line of text under each frame's number, saying
/// why it's there (`--sheet-events`), when any frame has one.
pub(crate) fn contact_sheet_with_captions(
    frames: &[(u64, Canvas, String)],
    updates_per_second: f32,
) -> Canvas {
    let Some((_, first, _)) = frames.first() else {
        return Canvas::new(1, 1);
    };
    let factor = shrink_factor(first.width(), SHEET_CELL_WIDTH);
    let (cell_width, cell_height) = (first.width() / factor, first.height() / factor);
    let columns = (frames.len() as f64).sqrt().ceil().max(1.0) as usize;
    let rows = frames.len().div_ceil(columns);
    let captions = frames.iter().any(|(_, _, caption)| !caption.is_empty());
    let label_height = LABEL_HEIGHT + if captions { CAPTION_HEIGHT } else { 0 };
    let width = SHEET_GAP + columns * (cell_width + SHEET_GAP);
    let height = SHEET_GAP + rows * (cell_height + label_height + SHEET_GAP);

    let mut sheet = Canvas::new(width, height);
    sheet.clear(SHEET_BACKGROUND);
    for (index, (frame, picture, caption)) in frames.iter().enumerate() {
        let left = SHEET_GAP + (index % columns) * (cell_width + SHEET_GAP);
        let top = SHEET_GAP + (index / columns) * (cell_height + label_height + SHEET_GAP);
        sheet.blit(&shrink(picture, factor), left, top);
        let seconds = *frame as f32 / updates_per_second;
        let label = format!("FRAME {frame}  {seconds:.1}S");
        let position = vec2(left as f32, (top + cell_height + 3) as f32);
        sheet.draw_text(&label, position, LABEL_SCALE, Color::WHITE);
        if !caption.is_empty() {
            // As much as fits in the cell's width, in capitals (easier to
            // read at this size).
            let fits = (cell_width as f32 / Canvas::text_width("M", 1).max(1.0)) as usize;
            let text: String = caption.to_uppercase().chars().take(fits.max(1)).collect();
            let position = vec2(left as f32, (top + cell_height + LABEL_HEIGHT + 1) as f32);
            sheet.draw_text(&text, position, 1, CAPTION_COLOR);
        }
    }
    sheet
}

/// Picks up to `count` frame numbers spread evenly from 1 to `last`,
/// always including both ends.
pub(crate) fn spread_frames(last: u64, count: usize) -> Vec<u64> {
    if last == 0 || count == 0 {
        return Vec::new();
    }
    if (last as usize) <= count || count == 1 {
        return if count == 1 {
            vec![last]
        } else {
            (1..=last).collect()
        };
    }
    let steps = (count - 1) as u64;
    let mut frames: Vec<u64> = (0..=steps)
        .map(|i| 1 + (i * (last - 1) + steps / 2) / steps)
        .collect();
    frames.dedup();
    frames
}

/// Writes an animated GIF, one frame at a time.
pub(crate) struct GifWriter {
    encoder: gif::Encoder<BufWriter<File>>,
    factor: usize,
    /// How long each frame shows, in hundredths of a second.
    delay: u16,
}

impl GifWriter {
    /// Starts a GIF of frames shrunk `factor` times, each shown for `delay`
    /// hundredths of a second, playing on a loop.
    pub(crate) fn create(
        path: &Path,
        canvas_width: usize,
        canvas_height: usize,
        factor: usize,
        delay: u16,
    ) -> io::Result<GifWriter> {
        let too_big = || io::Error::new(io::ErrorKind::InvalidInput, "too big for a GIF");
        let width = u16::try_from(canvas_width / factor).map_err(|_| too_big())?;
        let height = u16::try_from(canvas_height / factor).map_err(|_| too_big())?;
        let file = BufWriter::new(File::create(path)?);
        let mut encoder =
            gif::Encoder::new(file, width.max(1), height.max(1), &[]).map_err(io::Error::other)?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(io::Error::other)?;
        Ok(GifWriter {
            encoder,
            factor,
            delay,
        })
    }

    /// Adds a frame.
    pub(crate) fn add(&mut self, canvas: &Canvas) -> io::Result<()> {
        let small = shrink(canvas, self.factor);
        let mut rgb = Vec::with_capacity(small.pixels().len() * 3);
        for &pixel in small.pixels() {
            let [_, r, g, b] = pixel.to_be_bytes();
            rgb.extend_from_slice(&[r, g, b]);
        }
        // A GIF frame can only have 256 colors. If the frame has more (3D
        // shading, say), the gif crate picks the best 256; speed 10 is a
        // good balance between quality and time.
        let mut frame =
            gif::Frame::from_rgb_speed(small.width() as u16, small.height() as u16, &rgb, 10);
        frame.delay = self.delay;
        self.encoder.write_frame(&frame).map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    fn checkerboard(width: usize, height: usize) -> Canvas {
        let mut canvas = Canvas::new(width, height);
        for y in 0..height {
            for x in 0..width {
                if (x + y) % 2 == 0 {
                    canvas.set_pixel(x as i32, y as i32, Color::WHITE);
                }
            }
        }
        canvas
    }

    #[test]
    fn crops_are_read_and_checked() {
        let crop = Crop::parse("10, 20,30,40").unwrap();
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (10, 20, 30, 40));
        assert!(Crop::parse("1,2,3").is_err());
        assert!(Crop::parse("a,b,c,d").is_err());
        assert!(Crop::parse("0,0,0,5").unwrap_err().contains("at least 1"));
        assert!(crop.check(40, 60).is_ok());
        assert!(crop.check(39, 60).unwrap_err().contains("doesn't fit"));
        let huge = Crop::parse("1,0,18446744073709551615,1").unwrap();
        assert!(huge.check(10, 10).is_err()); // no overflow panic
    }

    #[test]
    fn a_close_up_makes_each_pixel_a_square() {
        let mut canvas = Canvas::new(8, 8);
        canvas.set_pixel(3, 2, Color::RED);
        let crop = Crop {
            x: 2,
            y: 2,
            width: 3,
            height: 2,
        };
        let big = close_up(&canvas, crop, 4);
        assert_eq!((big.width(), big.height()), (12, 8));
        // Pixel (3, 2) is the second pixel of the crop: x 4..8, y 0..4.
        assert_eq!(big.get_pixel(4, 0), Some(Color::RED));
        assert_eq!(big.get_pixel(7, 3), Some(Color::RED));
        assert_eq!(big.get_pixel(8, 0), Some(Color::BLACK));
        assert_eq!(big.get_pixel(3, 0), Some(Color::BLACK));
    }

    #[test]
    fn shrinking_averages_blocks() {
        let small = shrink(&checkerboard(4, 4), 2);
        assert_eq!((small.width(), small.height()), (2, 2));
        assert_eq!(small.get_pixel(0, 0), Some(Color::rgb(127, 127, 127)));
        assert_eq!(shrink_factor(320, 200), 2);
        assert_eq!(shrink_factor(160, 200), 1);
        assert_eq!(shrink_factor(1280, 320), 4);
    }

    #[test]
    fn spread_frames_includes_both_ends() {
        assert_eq!(spread_frames(600, 4), [1, 201, 400, 600]);
        assert_eq!(spread_frames(3, 16), [1, 2, 3]);
        assert_eq!(spread_frames(600, 1), [600]);
        assert_eq!(spread_frames(0, 16), Vec::<u64>::new());
        assert_eq!(spread_frames(600, 16).len(), 16);
    }

    #[test]
    fn a_sheet_has_a_cell_and_a_label_per_frame() {
        let mut frame = Canvas::new(40, 30);
        frame.fill_rect(Rect::new(0.0, 0.0, 40.0, 30.0), Color::RED);
        let frames: Vec<(u64, Canvas)> = (1..=5).map(|n| (n * 60, frame.copy_picture())).collect();
        let sheet = contact_sheet(&frames, 60.0);
        // 5 frames: 3 columns, 2 rows.
        assert_eq!(sheet.width(), 4 + 3 * (40 + 4));
        assert_eq!(sheet.height(), 4 + 2 * (30 + 14 + 4));
        assert_eq!(sheet.get_pixel(4, 4), Some(Color::RED));
        assert_eq!(sheet.get_pixel(0, 0), Some(SHEET_BACKGROUND));
        // The last row has 2 frames; the third cell is empty.
        assert_eq!(sheet.get_pixel(4 + 2 * 44, 4 + 48), Some(SHEET_BACKGROUND));
        // Some white label text under the first frame.
        let label_row = (4 + 30..4 + 30 + 14).flat_map(|y| (4..44).map(move |x| (x, y)));
        let white = label_row
            .filter(|&(x, y)| sheet.get_pixel(x, y) == Some(Color::WHITE))
            .count();
        assert!(white > 10);

        // With captions, each cell gets a line more, in yellow.
        let captioned: Vec<(u64, Canvas, String)> = frames
            .iter()
            .map(|(n, picture)| (*n, picture.copy_picture(), String::from("lives 2")))
            .collect();
        let sheet = contact_sheet_with_captions(&captioned, 60.0);
        assert_eq!(sheet.height(), 4 + 2 * (30 + 14 + 9 + 4));
        let caption_row = (4 + 30 + 14..4 + 30 + 14 + 9).flat_map(|y| (4..44).map(move |x| (x, y)));
        let yellow = caption_row
            .filter(|&(x, y)| sheet.get_pixel(x, y) == Some(CAPTION_COLOR))
            .count();
        assert!(yellow > 10, "{yellow}");
    }

    #[test]
    fn gifs_are_written() {
        let path = std::env::temp_dir().join(format!("duckforge-{}.gif", std::process::id()));
        let mut gif = GifWriter::create(&path, 40, 20, 2, 5).unwrap();
        gif.add(&checkerboard(40, 20)).unwrap();
        gif.add(&Canvas::new(40, 20)).unwrap();
        drop(gif); // finishes the file
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 20); // width
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 10); // height
    }
}

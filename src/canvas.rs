//! The canvas: a block of pixels in memory that games draw into.
//!
//! All 2D shapes are drawn by our own code writing numbers into a
//! `Vec<u32>` (a *software renderer*). 3D meshes are drawn either by our own
//! code too (`render3d.rs`) or by the graphics card (`gpu.rs`), which copies
//! its picture into the same pixels. Once per frame the engine hands the
//! finished pixels to the window to be shown.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::color::{self, Color};
use crate::font;
#[cfg(feature = "gpu")]
use crate::gpu::GpuRenderer;
use crate::math::{Rect, Vec2, vec2};
use crate::render3d::Renderer;
#[cfg(feature = "gpu")]
use crate::render3d::{Camera3D, Mesh, Transform};

/// A 2D grid of pixels.
///
/// Pixels are stored row by row in one flat `Vec`, so the pixel at
/// `(x, y)` lives at index `y * width + x`.
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
    /// For 3D drawing: how close the nearest thing drawn at each pixel is,
    /// stored as `1 / distance` (so bigger means closer, and 0.0 means nothing
    /// has been drawn yet). This is called a *depth buffer* or *z-buffer*.
    depth: Vec<f32>,
    /// Whether 3D drawing goes to the graphics card. This field only exists
    /// when the engine is built with the `gpu` feature.
    #[cfg(feature = "gpu")]
    gpu: GpuSlot,
}

/// The state of a canvas's connection to the graphics card.
#[cfg(feature = "gpu")]
enum GpuSlot {
    /// Draw 3D on the CPU.
    Off,
    /// Use the GPU, but nothing needed it yet. It starts at the first `draw_mesh`.
    NotStarted,
    /// The GPU is ready. (`Box` keeps the big renderer out of the `Canvas` itself.)
    Running(Box<GpuRenderer>),
    /// Starting the GPU didn't work, so 3D is drawn on the CPU.
    Failed,
}

impl Canvas {
    /// Creates a black canvas. It draws 3D on the CPU until you call
    /// [`Canvas::set_renderer`]. The engine's own canvas uses the GPU if it can.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
            depth: vec![0.0; width * height],
            #[cfg(feature = "gpu")]
            gpu: GpuSlot::Off,
        }
    }

    /// Chooses what draws 3D meshes: the graphics card or the CPU.
    ///
    /// The GPU only starts up when the first mesh is drawn, so 2D games
    /// never touch it. If it can't start (no suitable graphics driver, or
    /// the engine was built without the `gpu` feature), 3D is drawn on the
    /// CPU instead, and a message says why.
    pub fn set_renderer(&mut self, renderer: Renderer) {
        self.flush_3d();
        #[cfg(feature = "gpu")]
        {
            self.gpu = match renderer {
                Renderer::Cpu => GpuSlot::Off,
                Renderer::Auto | Renderer::Gpu => GpuSlot::NotStarted,
            };
        }
        #[cfg(not(feature = "gpu"))]
        if renderer == Renderer::Gpu {
            eprintln!("duckforge: built without the `gpu` feature, so 3D is drawn on the CPU");
        }
    }

    /// What is drawing 3D right now: `"CPU"`, or `"GPU: "` plus the graphics
    /// card's name.
    pub fn renderer_name(&self) -> &str {
        #[cfg(feature = "gpu")]
        match &self.gpu {
            GpuSlot::Running(gpu) => return gpu.description(),
            GpuSlot::NotStarted => return "GPU (not started yet)",
            GpuSlot::Off | GpuSlot::Failed => {}
        }
        "CPU"
    }

    /// Finishes any 3D drawing that is still waiting on the graphics card,
    /// so that [`Canvas::pixels`] and [`Canvas::get_pixel`] include it.
    ///
    /// You rarely need this: the engine calls it at the end of every frame,
    /// and drawing anything in 2D calls it first, so 2D drawn after 3D
    /// always lands on top.
    pub fn flush_3d(&mut self) {
        #[cfg(feature = "gpu")]
        if let GpuSlot::Running(gpu) = &mut self.gpu {
            gpu.flush(&mut self.pixels);
        }
    }

    /// Hands a mesh to the GPU. Returns `false` if the CPU should draw it instead.
    #[cfg(feature = "gpu")]
    pub(crate) fn draw_mesh_on_gpu(
        &mut self,
        mesh: &Mesh,
        transform: &Transform,
        camera: &Camera3D,
    ) -> bool {
        if matches!(self.gpu, GpuSlot::NotStarted) {
            self.gpu = match GpuRenderer::new(self.width, self.height) {
                Ok(gpu) => GpuSlot::Running(Box::new(gpu)),
                Err(err) => {
                    eprintln!("duckforge: can't use the GPU ({err}), so 3D is drawn on the CPU");
                    GpuSlot::Failed
                }
            };
        }
        match &mut self.gpu {
            GpuSlot::Running(gpu) => {
                gpu.draw(mesh, transform, camera, &mut self.pixels);
                true
            }
            _ => false,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The raw pixels, row by row, each in `0x00RRGGBB` format.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Fills the whole canvas with one color. This also resets the depth
    /// buffer, so call it at the start of every frame before drawing in 3D.
    ///
    /// The color's alpha is ignored: clearing always gives a solid
    /// picture. (To fade the old picture instead, cover it with a
    /// see-through `fill_rect`.)
    pub fn clear(&mut self, color: Color) {
        self.pixels.fill(color.to_u32());
        self.depth.fill(0.0);
        #[cfg(feature = "gpu")]
        if let GpuSlot::Running(gpu) = &mut self.gpu {
            gpu.clear(); // throw away 3D that was never shown, and reset the GPU's depth
        }
    }

    /// Converts `(x, y)` into an index into `pixels`, or `None` if it is off-canvas.
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(y as usize * self.width + x as usize)
    }

    /// Sets one pixel (or, with a see-through color, paints over it).
    /// Pixels outside the canvas are silently ignored.
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        self.flush_3d(); // 3D drawn earlier must end up *under* this pixel
        self.put_pixel(x, y, color.to_u32(), color.a);
    }

    /// Paints one pixel without flushing 3D first. For drawing functions
    /// that set many pixels: they flush once, then call this for each pixel.
    fn put_pixel(&mut self, x: i32, y: i32, color: u32, alpha: u8) {
        if let Some(i) = self.index(x, y) {
            paint(&mut self.pixels[i], color, alpha);
        }
    }

    /// Reads one pixel, or `None` if `(x, y)` is outside the canvas.
    pub fn get_pixel(&self, x: i32, y: i32) -> Option<Color> {
        self.index(x, y).map(|i| Color::from_hex(self.pixels[i]))
    }

    /// Fills a rectangle. Parts outside the canvas are clipped away.
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        if color.a == 0 {
            return; // invisible
        }
        self.flush_3d();
        // Round to whole pixels, then clamp to the canvas so we never index out of bounds.
        let clamp_x = |v: f32| (v.round() as i32).clamp(0, self.width as i32) as usize;
        let clamp_y = |v: f32| (v.round() as i32).clamp(0, self.height as i32) as usize;
        let (x0, x1) = (clamp_x(rect.left()), clamp_x(rect.right()));
        let (y0, y1) = (clamp_y(rect.top()), clamp_y(rect.bottom()));
        if x0 >= x1 || y0 >= y1 {
            return; // nothing visible
        }

        let c = color.to_u32();
        for y in y0..y1 {
            let row = &mut self.pixels[y * self.width + x0..y * self.width + x1];
            if color.is_opaque() {
                // Filling a whole slice at once is much faster than one set_pixel call per pixel.
                row.fill(c);
            } else {
                row.iter_mut().for_each(|pixel| paint(pixel, c, color.a));
            }
        }
    }

    /// Draws a 1-pixel outline of a rectangle.
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        let Rect { x, y, w, h } = rect;
        if !color.is_opaque() {
            // See-through: the four sides must not overlap at the corners,
            // or the corners would be painted twice and look darker.
            if w <= 2.0 || h <= 2.0 {
                self.fill_rect(rect, color);
            } else {
                self.fill_rect(Rect::new(x, y, w, 1.0), color); // top
                self.fill_rect(Rect::new(x, y + h - 1.0, w, 1.0), color); // bottom
                self.fill_rect(Rect::new(x, y + 1.0, 1.0, h - 2.0), color); // left
                self.fill_rect(Rect::new(x + w - 1.0, y + 1.0, 1.0, h - 2.0), color); // right
            }
            return;
        }
        self.fill_rect(Rect::new(x, y, w, 1.0), color); // top
        self.fill_rect(Rect::new(x, y + h - 1.0, w, 1.0), color); // bottom
        self.fill_rect(Rect::new(x, y, 1.0, h), color); // left
        self.fill_rect(Rect::new(x + w - 1.0, y, 1.0, h), color); // right
    }

    /// Fills a circle.
    pub fn fill_circle(&mut self, center: Vec2, radius: f32, color: Color) {
        if color.a == 0 {
            return;
        }
        self.flush_3d();
        let c = color.to_u32();
        // Only visit the pixels in the circle's bounding box that are on the canvas.
        let x0 = ((center.x - radius).floor() as i32).max(0);
        let x1 = ((center.x + radius).ceil() as i32).min(self.width as i32);
        let y0 = ((center.y - radius).floor() as i32).max(0);
        let y1 = ((center.y + radius).ceil() as i32).min(self.height as i32);

        let r2 = radius * radius;
        for y in y0..y1 {
            for x in x0..x1 {
                // Is the *center* of this pixel inside the circle?
                let dx = x as f32 + 0.5 - center.x;
                let dy = y as f32 + 0.5 - center.y;
                if dx * dx + dy * dy <= r2 {
                    self.put_pixel(x, y, c, color.a);
                }
            }
        }
    }

    /// Draws a 1-pixel line using Bresenham's algorithm, which steps
    /// pixel by pixel using only integer math.
    pub fn draw_line(&mut self, from: Vec2, to: Vec2, color: Color) {
        if color.a == 0 {
            return;
        }
        self.flush_3d();
        // First cut the line down to the part that's on the canvas. Otherwise
        // a line to a far-away point would step through millions of invisible
        // pixels, and the integer math below could overflow.
        let Some((from, to)) = clip_line(from, to, self.width, self.height) else {
            return; // no part of it is on the canvas
        };
        let c = color.to_u32();

        let (mut x, mut y) = (from.x.round() as i32, from.y.round() as i32);
        let (x_end, y_end) = (to.x.round() as i32, to.y.round() as i32);

        let dx = (x_end - x).abs();
        let dy = -(y_end - y).abs();
        let step_x = if x < x_end { 1 } else { -1 };
        let step_y = if y < y_end { 1 } else { -1 };
        let mut error = dx + dy;

        loop {
            self.put_pixel(x, y, c, color.a);
            if x == x_end && y == y_end {
                break;
            }
            let e2 = 2 * error;
            if e2 >= dy {
                error += dy;
                x += step_x;
            }
            if e2 <= dx {
                error += dx;
                y += step_y;
            }
        }
    }

    /// Fills a triangle given its three corners, in any order.
    pub fn fill_triangle(&mut self, a: Vec2, b: Vec2, c: Vec2, color: Color) {
        if color.a == 0 {
            return;
        }
        self.flush_3d();
        let c32 = color.to_u32();
        let pixels = &mut self.pixels;
        for_each_pixel_in_triangle(self.width, self.height, [a, b, c], |i, _| {
            paint(&mut pixels[i], c32, color.a);
        });
    }

    /// Fills a triangle for the 3D renderer. Each corner also has an
    /// `inverse_depth` (1 / distance from the camera). A pixel is only drawn
    /// if it's closer than whatever is already there, which is how objects in
    /// front hide objects behind them.
    pub(crate) fn fill_triangle_3d(
        &mut self,
        corners: [Vec2; 3],
        inverse_depths: [f32; 3],
        color: Color,
    ) {
        let c32 = color.to_u32();
        // Borrow the two buffers separately, so the closure can change both.
        let (pixels, depth) = (&mut self.pixels, &mut self.depth);
        for_each_pixel_in_triangle(self.width, self.height, corners, |i, weights| {
            // Blend the corners' depths by how close this pixel is to each corner.
            let d = weights[0] * inverse_depths[0]
                + weights[1] * inverse_depths[1]
                + weights[2] * inverse_depths[2];
            if d > depth[i] {
                depth[i] = d;
                pixels[i] = c32;
            }
        });
    }

    /// Draws text with the built-in 3x5 pixel font. `pos` is the top-left
    /// corner; `scale` makes each font pixel a `scale` x `scale` square.
    /// A `'\n'` starts a new line.
    pub fn draw_text(&mut self, text: &str, pos: Vec2, scale: u32, color: Color) {
        let s = scale as f32;
        let mut cursor = pos;

        for c in text.chars() {
            if c == '\n' {
                cursor.x = pos.x;
                cursor.y += font::LINE_HEIGHT as f32 * s;
                continue;
            }
            if c == '\r' {
                continue; // the first half of a Windows "\r\n" line ending
            }

            let glyph = font::glyph(c);
            for row in 0..font::GLYPH_HEIGHT {
                for col in 0..font::GLYPH_WIDTH {
                    if font::is_lit(&glyph, col, row) {
                        let x = cursor.x + col as f32 * s;
                        let y = cursor.y + row as f32 * s;
                        self.fill_rect(Rect::new(x, y, s, s), color);
                    }
                }
            }
            cursor.x += font::ADVANCE as f32 * s;
        }
    }

    /// Draws text centered horizontally on the canvas, with its top at `y`.
    /// Each line of multi-line text is centered on its own.
    pub fn draw_text_centered(&mut self, text: &str, y: f32, scale: u32, color: Color) {
        let line_height = font::LINE_HEIGHT as f32 * scale as f32;
        for (i, line) in text.lines().enumerate() {
            let x = (self.width as f32 - Canvas::text_width(line, scale)) / 2.0;
            self.draw_text(line, vec2(x, y + i as f32 * line_height), scale, color);
        }
    }

    /// How many pixels wide `text` is when drawn at `scale` (its widest line).
    pub fn text_width(text: &str, scale: u32) -> f32 {
        let longest = text
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        if longest == 0 {
            return 0.0;
        }
        // Every character takes ADVANCE pixels, but there is no gap after the
        // last one. (Multiplying as floats: whole numbers could overflow.)
        (longest * font::ADVANCE - 1) as f32 * scale as f32
    }

    /// A new canvas holding the same picture. (The copy draws 3D on the CPU.)
    pub(crate) fn copy_picture(&self) -> Canvas {
        Canvas::from_pixels(self.width, self.height, self.pixels.clone())
    }

    /// A canvas made from `pixels`, row by row.
    ///
    /// # Panics
    ///
    /// If there aren't exactly `width * height` pixels.
    pub(crate) fn from_pixels(width: usize, height: usize, pixels: Vec<u32>) -> Canvas {
        assert_eq!(pixels.len(), width * height, "wrong number of pixels");
        let mut canvas = Canvas::new(width, height);
        canvas.pixels = pixels;
        canvas
    }

    /// Copies `other` onto this canvas with its top-left corner at
    /// (`left`, `top`). Whatever doesn't fit is cut off.
    pub(crate) fn blit(&mut self, other: &Canvas, left: usize, top: usize) {
        self.flush_3d();
        let width = other.width.min(self.width.saturating_sub(left));
        for y in 0..other.height.min(self.height.saturating_sub(top)) {
            let from = y * other.width;
            let to = (top + y) * self.width + left;
            self.pixels[to..to + width].copy_from_slice(&other.pixels[from..from + width]);
        }
    }

    /// Saves the canvas as a `.png` image. PNG files are much smaller than
    /// BMP (they're compressed, without losing anything), and every phone,
    /// browser and image viewer can show them.
    pub fn save_png(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let file = File::create(path)?;
        let mut out = BufWriter::new(file);
        self.write_png(&mut out)?;
        out.flush()
    }

    /// Writes the canvas as a PNG: 8 bits each of red, green and blue per
    /// pixel. The `png` crate does the compressing.
    pub(crate) fn write_png(&self, out: &mut impl Write) -> io::Result<()> {
        let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
        let width = u32::try_from(self.width).map_err(|_| invalid("image too wide for PNG"))?;
        let height = u32::try_from(self.height).map_err(|_| invalid("image too tall for PNG"))?;
        if width == 0 || height == 0 {
            return Err(invalid("a PNG needs at least 1 x 1 pixels"));
        }
        let mut encoder = png::Encoder::new(out, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;

        // Each pixel 0x00RRGGBB becomes the three bytes R, G, B.
        let mut bytes = Vec::with_capacity(self.pixels.len() * 3);
        for &pixel in &self.pixels {
            let [_, r, g, b] = pixel.to_be_bytes();
            bytes.extend_from_slice(&[r, g, b]);
        }
        writer.write_image_data(&bytes)?;
        writer.finish()?;
        Ok(())
    }

    /// Reads a PNG written by [`Canvas::save_png`], to check what was saved.
    #[cfg(test)]
    pub(crate) fn load_png_for_tests(path: &Path) -> Canvas {
        let file = io::BufReader::new(File::open(path).unwrap());
        let mut reader = png::Decoder::new(file).read_info().unwrap();
        let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut bytes).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        let mut canvas = Canvas::new(info.width as usize, info.height as usize);
        for (pixel, rgb) in canvas.pixels.iter_mut().zip(bytes.chunks_exact(3)) {
            *pixel = u32::from_be_bytes([0, rgb[0], rgb[1], rgb[2]]);
        }
        canvas
    }

    /// Saves the canvas as a `.bmp` image, a format every image viewer can open.
    pub fn save_bmp(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let file = File::create(path)?;
        let mut out = BufWriter::new(file);
        self.write_bmp(&mut out)?;
        out.flush()
    }

    /// Writes a 32-bit BMP: a 54-byte header followed by the raw pixels.
    fn write_bmp(&self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(&bmp_header(self.width, self.height)?)?;

        // A pixel 0x00RRGGBB stored little-endian is the bytes B, G, R, 0,
        // which is exactly BMP's order. We set the last byte to 0xFF (opaque).
        for &pixel in &self.pixels {
            out.write_all(&(pixel | 0xFF00_0000).to_le_bytes())?;
        }
        Ok(())
    }
}

/// The 54-byte header of a 32-bit BMP file. BMP stores its sizes as 32-bit
/// numbers, so a gigantic image can't be saved: that's an error rather than
/// a corrupt file.
fn bmp_header(width: usize, height: usize) -> io::Result<Vec<u8>> {
    const HEADER_SIZE: u32 = 14 + 40;
    let too_big = || io::Error::new(io::ErrorKind::InvalidInput, "image too big for a BMP file");
    let bmp_width = i32::try_from(width).map_err(|_| too_big())?;
    let bmp_height = i32::try_from(height).map_err(|_| too_big())?;
    let image_size = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(too_big)?;
    let file_size = image_size.checked_add(HEADER_SIZE).ok_or_else(too_big)?;

    let mut header = Vec::with_capacity(HEADER_SIZE as usize);
    // File header.
    header.extend_from_slice(b"BM");
    header.extend_from_slice(&file_size.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes()); // reserved
    header.extend_from_slice(&HEADER_SIZE.to_le_bytes()); // where the pixels start

    // Info header (the "BITMAPINFOHEADER" layout).
    header.extend_from_slice(&40u32.to_le_bytes()); // size of this header
    header.extend_from_slice(&bmp_width.to_le_bytes());
    header.extend_from_slice(&(-bmp_height).to_le_bytes()); // negative = rows go top to bottom
    header.extend_from_slice(&1u16.to_le_bytes()); // color planes (always 1)
    header.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    header.extend_from_slice(&0u32.to_le_bytes()); // no compression
    header.extend_from_slice(&image_size.to_le_bytes());
    header.extend_from_slice(&2835u32.to_le_bytes()); // horizontal resolution (72 DPI)
    header.extend_from_slice(&2835u32.to_le_bytes()); // vertical resolution
    header.extend_from_slice(&0u32.to_le_bytes()); // colors in palette (none)
    header.extend_from_slice(&0u32.to_le_bytes()); // "important" colors (all)
    Ok(header)
}

/// Cuts the line from `a` to `b` down to the part that lies on a `width` x
/// `height` canvas. Returns `None` if none of it does, or if a coordinate
/// isn't a real number.
///
/// This is *Liang-Barsky clipping*. The line is every point `a + (b - a) * t`
/// for `t` from 0 to 1, and each edge of the canvas rules out part of that
/// range of `t`. It works in `f64`: with an end 10 billion pixels away, an
/// `f32` isn't precise enough to say where the line crosses a 20-pixel canvas.
fn clip_line(a: Vec2, b: Vec2, width: usize, height: usize) -> Option<(Vec2, Vec2)> {
    if width == 0 || height == 0 || ![a.x, a.y, b.x, b.y].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (ax, ay) = (a.x as f64, a.y as f64);
    let (dx, dy) = (b.x as f64 - ax, b.y as f64 - ay);
    let (max_x, max_y) = ((width - 1) as f64, (height - 1) as f64);

    let (mut t_start, mut t_end) = (0.0f64, 1.0f64);
    // For each edge: `p` is how fast the line moves out through it, and `q`
    // is how far inside the edge the line starts.
    for (p, q) in [(-dx, ax), (dx, max_x - ax), (-dy, ay), (dy, max_y - ay)] {
        if p == 0.0 {
            if q < 0.0 {
                return None; // runs alongside this edge, on the outside
            }
        } else if p < 0.0 {
            t_start = t_start.max(q / p); // coming in through this edge
        } else {
            t_end = t_end.min(q / p); // going out through this edge
        }
    }
    if t_start > t_end {
        return None; // misses the canvas
    }
    let point = |t: f64| vec2((ax + dx * t) as f32, (ay + dy * t) as f32);
    Some((point(t_start), point(t_end)))
}

/// Calls `plot(index, weights)` for every pixel whose center is inside the
/// triangle. `index` is the pixel's position in the pixel buffer, and
/// `weights` says how much each corner "owns" that pixel (they add up to 1).
///
/// This uses *edge functions*: for each edge, a number that is positive on
/// one side of the edge and negative on the other. A pixel is inside the
/// triangle when it's on the inner side of all three edges.
fn for_each_pixel_in_triangle(
    width: usize,
    height: usize,
    [a, b, c]: [Vec2; 3],
    mut plot: impl FnMut(usize, [f32; 3]),
) {
    // Twice the triangle's area. Its sign says whether the corners go clockwise or not.
    let area = edge(a, b, c);
    if area == 0.0 || !area.is_finite() {
        return; // a flat sliver: nothing to fill
    }

    // Only look at pixels inside the triangle's bounding box (and the canvas).
    let x0 = (a.x.min(b.x).min(c.x).floor().max(0.0)) as usize;
    let y0 = (a.y.min(b.y).min(c.y).floor().max(0.0)) as usize;
    let x1 = (a.x.max(b.x).max(c.x).ceil().max(0.0) as usize).min(width);
    let y1 = (a.y.max(b.y).max(c.y).ceil().max(0.0) as usize).min(height);

    for y in y0..y1 {
        for x in x0..x1 {
            let p = vec2(x as f32 + 0.5, y as f32 + 0.5); // the pixel's center
            let weights = [
                edge(b, c, p) / area,
                edge(c, a, p) / area,
                edge(a, b, p) / area,
            ];
            if weights.iter().all(|&w| w >= 0.0) {
                plot(y * width + x, weights);
            }
        }
    }
}

/// Paints `color` onto one canvas pixel: replaces it when `alpha` is 255
/// (solid), otherwise mixes the two (see [`color::blend`]).
#[inline]
fn paint(pixel: &mut u32, color: u32, alpha: u8) {
    *pixel = if alpha == 255 {
        color
    } else {
        color::blend(*pixel, color, alpha)
    };
}

/// Which side of the line from `a` to `b` is `p` on? Positive on one side,
/// negative on the other, zero on the line. (It's a 2D cross product.)
fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pixel of the canvas as a `Color`, for comparing pictures.
    fn colors(canvas: &Canvas) -> Vec<Color> {
        canvas.pixels.iter().map(|&p| Color::from_hex(p)).collect()
    }

    #[test]
    fn half_transparent_red_over_blue() {
        let mut canvas = Canvas::new(4, 4);
        canvas.clear(Color::rgb(0, 0, 255));
        canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, 2.0), Color::rgba(255, 0, 0, 128));
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::rgb(128, 0, 127)));
        assert_eq!(canvas.get_pixel(2, 2), Some(Color::rgb(0, 0, 255)), "outside the rect");

        // Every other shape blends the same way, and paints each pixel once.
        let half_red = Color::rgba(255, 0, 0, 128);
        let mixed = Some(Color::rgb(128, 0, 127));
        let fresh = || {
            let mut c = Canvas::new(8, 8);
            c.clear(Color::rgb(0, 0, 255));
            c
        };
        let mut c = fresh();
        c.fill_circle(vec2(4.0, 4.0), 3.0, half_red);
        assert_eq!(c.get_pixel(4, 4), mixed);
        let mut c = fresh();
        c.draw_line(vec2(0.0, 0.0), vec2(7.0, 7.0), half_red);
        assert_eq!(c.get_pixel(3, 3), mixed);
        let mut c = fresh();
        c.fill_triangle(vec2(0.0, 0.0), vec2(8.0, 0.0), vec2(0.0, 8.0), half_red);
        assert_eq!(c.get_pixel(1, 1), mixed);
        let mut c = fresh();
        c.draw_text("H", vec2(0.0, 0.0), 1, half_red);
        assert_eq!(c.get_pixel(0, 0), mixed);
        let mut c = fresh();
        c.set_pixel(5, 5, half_red);
        assert_eq!(c.get_pixel(5, 5), mixed);
        let mut c = fresh();
        c.draw_rect(Rect::new(1.0, 1.0, 5.0, 5.0), half_red);
        for (x, y) in [(1, 1), (5, 1), (1, 5), (5, 5), (3, 1), (1, 3)] {
            assert_eq!(c.get_pixel(x, y), mixed, "outline pixel ({x}, {y}) painted once");
        }
        assert_eq!(c.get_pixel(3, 3), Some(Color::rgb(0, 0, 255)), "inside stays empty");
    }

    #[test]
    fn invisible_colors_draw_nothing() {
        let mut canvas = Canvas::new(8, 8);
        canvas.clear(Color::GREEN);
        let ghost = Color::RED.with_alpha(0);
        canvas.fill_rect(Rect::new(0.0, 0.0, 8.0, 8.0), ghost);
        canvas.fill_circle(vec2(4.0, 4.0), 3.0, ghost);
        canvas.draw_line(vec2(0.0, 0.0), vec2(7.0, 7.0), ghost);
        canvas.draw_text("HI", vec2(0.0, 0.0), 1, ghost);
        assert!(colors(&canvas).iter().all(|&c| c == Color::GREEN));
    }

    #[test]
    fn solid_colors_draw_exactly_as_before() {
        // Draw the same scene with rgb() and with rgba(.., 255): identical.
        let scene = |color: fn(u8, u8, u8) -> Color| {
            let mut c = Canvas::new(40, 30);
            c.clear(color(10, 20, 30));
            c.fill_rect(Rect::new(2.5, 3.2, 10.7, 6.1), color(200, 0, 0));
            c.draw_rect(Rect::new(1.0, 1.0, 30.0, 1.5), color(0, 200, 0));
            c.fill_circle(vec2(20.0, 15.0), 6.3, color(0, 0, 200));
            c.draw_line(vec2(-5.0, 2.0), vec2(45.0, 27.0), color(250, 250, 0));
            c.fill_triangle(vec2(1.0, 29.0), vec2(15.0, 18.0), vec2(9.0, 29.0), color(9, 9, 9));
            c.draw_text("OK!", vec2(25.0, 2.0), 2, color(255, 255, 255));
            c
        };
        let old = scene(Color::rgb);
        let new = scene(|r, g, b| Color::rgba(r, g, b, 255));
        assert_eq!(old.pixels, new.pixels);
    }

    #[test]
    fn set_and_get_pixel() {
        let (width, x, y) = (4, 2, 1);
        let mut canvas = Canvas::new(width, 3);
        canvas.set_pixel(x as i32, y as i32, Color::RED);
        assert_eq!(canvas.get_pixel(2, 1), Some(Color::RED));
        assert_eq!(canvas.pixels()[y * width + x], Color::RED.to_u32());
        assert_eq!(canvas.get_pixel(0, 0), Some(Color::BLACK));
        assert_eq!(canvas.get_pixel(4, 0), None);
        assert_eq!(canvas.get_pixel(-1, 0), None);
    }

    #[test]
    fn drawing_off_canvas_does_not_panic() {
        let mut canvas = Canvas::new(10, 10);
        canvas.set_pixel(-5, 100, Color::WHITE);
        canvas.fill_rect(Rect::new(-50.0, -50.0, 20.0, 20.0), Color::WHITE);
        canvas.fill_rect(Rect::new(5.0, 5.0, -3.0, 2.0), Color::WHITE);
        canvas.fill_circle(vec2(-100.0, 5.0), 3.0, Color::WHITE);
        canvas.draw_line(vec2(-20.0, -20.0), vec2(30.0, 30.0), Color::WHITE);
        canvas.draw_text("OFF SCREEN", vec2(8.0, 8.0), 4, Color::WHITE);
    }

    #[test]
    fn fill_rect_is_clipped() {
        let mut canvas = Canvas::new(10, 10);
        canvas.fill_rect(Rect::new(-5.0, -5.0, 7.0, 7.0), Color::GREEN);
        assert_eq!(canvas.get_pixel(0, 0), Some(Color::GREEN));
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::GREEN));
        assert_eq!(canvas.get_pixel(2, 2), Some(Color::BLACK));
    }

    #[test]
    fn circle_and_line() {
        let mut canvas = Canvas::new(20, 20);
        canvas.fill_circle(vec2(10.0, 10.0), 4.0, Color::BLUE);
        assert_eq!(canvas.get_pixel(10, 10), Some(Color::BLUE));
        assert_eq!(canvas.get_pixel(0, 0), Some(Color::BLACK));

        canvas.draw_line(vec2(0.0, 19.0), vec2(19.0, 19.0), Color::RED);
        assert!((0..20).all(|x| canvas.get_pixel(x, 19) == Some(Color::RED)));
    }

    #[test]
    fn triangles_fill_either_winding() {
        for corners in [
            [vec2(0.0, 0.0), vec2(10.0, 0.0), vec2(0.0, 10.0)],
            [vec2(0.0, 0.0), vec2(0.0, 10.0), vec2(10.0, 0.0)],
        ] {
            let mut canvas = Canvas::new(10, 10);
            let [a, b, c] = corners;
            canvas.fill_triangle(a, b, c, Color::GREEN);
            assert_eq!(canvas.get_pixel(1, 1), Some(Color::GREEN));
            assert_eq!(canvas.get_pixel(9, 9), Some(Color::BLACK));
        }
        // Huge and degenerate triangles must not panic.
        let mut canvas = Canvas::new(10, 10);
        canvas.fill_triangle(vec2(-1e6, -1e6), vec2(1e6, 0.0), vec2(0.0, 1e6), Color::RED);
        canvas.fill_triangle(vec2(1.0, 1.0), vec2(2.0, 2.0), vec2(3.0, 3.0), Color::RED);
    }

    #[test]
    fn depth_test_keeps_the_nearest() {
        let mut canvas = Canvas::new(10, 10);
        canvas.clear(Color::BLACK);
        let corners = [vec2(0.0, 0.0), vec2(10.0, 0.0), vec2(0.0, 10.0)];
        canvas.fill_triangle_3d(corners, [0.5; 3], Color::RED); // 2 units away
        canvas.fill_triangle_3d(corners, [0.1; 3], Color::BLUE); // 10 units away: hidden
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::RED));
        canvas.fill_triangle_3d(corners, [1.0; 3], Color::GREEN); // 1 unit away: in front
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::GREEN));
        canvas.clear(Color::BLACK); // clearing resets depth too
        canvas.fill_triangle_3d(corners, [0.1; 3], Color::BLUE);
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::BLUE));
    }

    #[test]
    fn text_width_matches_font() {
        assert_eq!(Canvas::text_width("", 1), 0.0);
        assert_eq!(Canvas::text_width("A", 1), 3.0);
        assert_eq!(Canvas::text_width("AB", 1), 7.0);
        assert_eq!(Canvas::text_width("AB", 2), 14.0);
        assert_eq!(Canvas::text_width("A\nABC", 1), 11.0);
    }

    #[test]
    fn bmp_has_header_and_pixels() {
        let mut canvas = Canvas::new(3, 2);
        canvas.set_pixel(0, 0, Color::rgb(1, 2, 3));
        let mut bytes: Vec<u8> = Vec::new();
        canvas.write_bmp(&mut bytes).unwrap();

        assert_eq!(bytes.len(), 54 + 3 * 2 * 4);
        assert_eq!(&bytes[0..2], b"BM");
        assert_eq!(&bytes[54..58], &[3, 2, 1, 0xFF]); // blue, green, red, alpha
    }

    #[test]
    fn far_away_lines_are_clipped_to_the_canvas() {
        let mut canvas = Canvas::new(20, 20);
        // Ends far off the canvas used to overflow the integer math (a panic).
        canvas.draw_line(vec2(-1e10, 5.0), vec2(1e10, 5.0), Color::RED);
        assert!((0..20).all(|x| canvas.get_pixel(x, 5) == Some(Color::RED)));

        // A long line crossing the canvas lands on the same pixels as a short one.
        let mut short = Canvas::new(20, 20);
        short.draw_line(vec2(0.0, 0.0), vec2(19.0, 19.0), Color::RED);
        let mut long = Canvas::new(20, 20);
        long.draw_line(vec2(-1000.0, -1000.0), vec2(1000.0, 1000.0), Color::RED);
        assert_eq!(short.pixels(), long.pixels());

        // Lines completely off the canvas, or with a NaN end, draw nothing.
        let mut empty = Canvas::new(20, 20);
        empty.draw_line(vec2(-50.0, -50.0), vec2(-10.0, -30.0), Color::RED);
        empty.draw_line(vec2(f32::NAN, 3.0), vec2(10.0, 3.0), Color::RED);
        assert!(empty.pixels().iter().all(|&p| p == 0));
    }

    #[test]
    fn bmp_refuses_images_too_big_for_the_format() {
        assert_eq!(bmp_header(3, 2).unwrap().len(), 54);
        let error = bmp_header(100_000, 100_000).unwrap_err(); // 40 GB of pixels
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn huge_text_scales_do_not_overflow() {
        assert_eq!(Canvas::text_width("AB", u32::MAX), 7.0 * u32::MAX as f32);
        let mut canvas = Canvas::new(10, 10);
        canvas.draw_text_centered("A", 0.0, u32::MAX, Color::WHITE);
    }

    #[test]
    fn windows_line_endings_draw_like_unix_ones() {
        let mut windows = Canvas::new(20, 20);
        windows.draw_text("A\r\nB", vec2(0.0, 0.0), 1, Color::WHITE);
        let mut unix = Canvas::new(20, 20);
        unix.draw_text("A\nB", vec2(0.0, 0.0), 1, Color::WHITE);
        assert_eq!(windows.pixels(), unix.pixels());
    }

    #[test]
    fn png_round_trip_keeps_every_pixel() {
        let mut canvas = Canvas::new(5, 3);
        canvas.set_pixel(0, 0, Color::rgb(255, 0, 0));
        canvas.set_pixel(4, 2, Color::rgb(1, 2, 3));
        canvas.set_pixel(2, 1, Color::WHITE);
        let path = std::env::temp_dir().join(format!("duckforge-png-{}.png", std::process::id()));
        canvas.save_png(&path).unwrap();
        let loaded = Canvas::load_png_for_tests(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!((loaded.width(), loaded.height()), (5, 3));
        assert_eq!(loaded.pixels(), canvas.pixels());
    }

    #[test]
    fn an_empty_canvas_cant_be_a_png() {
        let mut bytes = Vec::new();
        let err = Canvas::new(0, 4).write_png(&mut bytes).unwrap_err();
        assert!(err.to_string().contains("at least 1 x 1"));
    }

    #[test]
    fn blit_copies_and_clips() {
        let mut small = Canvas::new(3, 2);
        small.clear(Color::RED);
        let mut big = Canvas::new(5, 4);
        big.blit(&small, 3, 3); // only 2 x 1 of it fits
        assert_eq!(big.get_pixel(3, 3), Some(Color::RED));
        assert_eq!(big.get_pixel(4, 3), Some(Color::RED));
        assert_eq!(big.get_pixel(2, 3), Some(Color::BLACK));
        assert_eq!(big.get_pixel(3, 2), Some(Color::BLACK));
        let copy = big.copy_picture();
        assert_eq!(copy.pixels(), big.pixels());
        big.blit(&small, 10, 10); // entirely outside: nothing happens
    }
}

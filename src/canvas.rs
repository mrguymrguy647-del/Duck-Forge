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
use std::rc::Rc;

use crate::camera::{Camera2D, View};
use crate::color::{self, Color};
use crate::font;
#[cfg(feature = "gpu")]
use crate::gpu::GpuRenderer;
use crate::math::{Rect, Vec2, vec2};
#[cfg(feature = "gpu")]
use crate::render3d::{Camera3D, Mesh, Transform};
use crate::render3d::{Lighting, Renderer};
use crate::scene::{Label, SceneText};
use crate::sprite::{Blit, Sprite};

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
    /// How positions given to the drawing functions become pixels: changed
    /// by `push_camera` and `push_offset`.
    view: View,
    /// The views to go back to, one per `push_camera` or `push_offset`.
    saved_views: Vec<View>,
    /// The first camera pushed this frame, for the debug overlay.
    frame_camera: Option<Camera2D>,
    /// What the game labelled with `label` since the last `take_scene`.
    labels: Vec<Label>,
    /// The lines of text drawn since the last `take_scene`.
    texts: Vec<SceneText>,
    /// The light 3D is drawn with.
    lighting: Lighting,
    /// Distance fog over 3D, if any.
    fog: Option<crate::render3d::Fog>,
    /// The last 3D drawn was in view space (`draw_mesh_in_view`): the
    /// depth buffer was emptied for it, and isn't again until the world is
    /// drawn once more.
    pub(crate) drawing_in_view: bool,
    /// For see-through meshes: which mesh last painted each pixel (by
    /// number), so a pixel on the edge two triangles share is mixed in
    /// once, not twice. Empty until the first see-through mesh.
    see_through_marks: Vec<u32>,
    see_through_mark: u32,
    /// Billboard quads made so far, kept so the GPU sees the same mesh
    /// every frame (and uploads it once).
    billboards: Vec<BillboardQuad>,
}

/// One billboard's quad: which picture, mirrored or not, and the mesh.
struct BillboardQuad {
    picture: (usize, [usize; 4]),
    flip_x: bool,
    mesh: Rc<crate::render3d::Mesh>,
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
            view: View::SCREEN,
            saved_views: Vec::new(),
            frame_camera: None,
            labels: Vec::new(),
            texts: Vec::new(),
            lighting: Lighting::default(),
            fog: None,
            drawing_in_view: false,
            see_through_marks: Vec::new(),
            see_through_mark: 0,
            billboards: Vec::new(),
        }
    }

    /// The quad mesh for a billboard of `sprite`, made once and kept (the
    /// most recent 256).
    pub(crate) fn billboard_mesh(
        &mut self,
        sprite: &Sprite,
        flip_x: bool,
    ) -> Rc<crate::render3d::Mesh> {
        let picture = sprite.identity();
        let known = self
            .billboards
            .iter()
            .find(|quad| quad.picture == picture && quad.flip_x == flip_x);
        if let Some(quad) = known {
            return Rc::clone(&quad.mesh);
        }
        if self.billboards.len() >= 256 {
            self.billboards.remove(0);
        }
        let mesh = Rc::new(crate::render3d::billboard_quad(sprite, flip_x));
        self.billboards.push(BillboardQuad {
            picture,
            flip_x,
            mesh: Rc::clone(&mesh),
        });
        mesh
    }

    /// Sets the light that 3D meshes are drawn with, from now on (it
    /// stays until it's changed: setting it at the start of `draw` is
    /// simplest). [`Lighting::default`] is the light every canvas starts
    /// with. See [`Lighting`].
    pub fn set_lighting(&mut self, lighting: Lighting) {
        self.lighting = lighting;
    }

    /// Fades 3D into a colour with distance (`Some`), or not (`None`, the
    /// start). It stays until it's changed, like the lighting. See
    /// [`Fog`](crate::render3d::Fog).
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(32, 24);
    /// let sky = Color::from_hex(0x87ceeb);
    /// canvas.clear(sky);
    /// canvas.set_fog(Some(Fog { color: sky, near: 30.0, far: 80.0 }));
    /// ```
    pub fn set_fog(&mut self, fog: Option<crate::render3d::Fog>) {
        self.fog = fog;
    }

    /// The fog 3D is drawn with now.
    pub fn fog(&self) -> Option<crate::render3d::Fog> {
        self.fog
    }

    /// The light 3D meshes are drawn with now.
    pub fn lighting(&self) -> Lighting {
        self.lighting
    }

    /// Draws everything from now on through `camera`, until
    /// [`Canvas::pop_camera`]: positions and sizes are in the *world*, and
    /// the camera decides where on the screen they end up.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(320, 240);
    /// # let camera = Camera2D::new(320.0, 240.0);
    /// canvas.push_camera(&camera);
    /// // ... draw the world ...
    /// canvas.pop_camera();
    /// // ... draw the score, menus and other screen things ...
    /// ```
    ///
    /// Everything follows the camera: shapes, lines, text, sprites and
    /// `set_pixel`. Lines stay 1 pixel thin at any zoom. `clear`,
    /// `get_pixel` and 3D meshes always work in screen pixels. The engine
    /// starts every frame with no camera, so a forgotten pop only affects
    /// the rest of that frame.
    pub fn push_camera(&mut self, camera: &Camera2D) {
        if self.frame_camera.is_none() {
            self.frame_camera = Some(camera.clone());
        }
        self.saved_views.push(self.view);
        self.view = self.view.then(camera.view());
    }

    /// Goes back to how drawing worked before the last
    /// [`push_camera`](Canvas::push_camera) (usually: screen pixels).
    pub fn pop_camera(&mut self) {
        self.view = self.saved_views.pop().unwrap_or(View::SCREEN);
    }

    /// Moves everything drawn from now on by `offset`, until
    /// [`Canvas::pop_offset`]. Handy for drawing a group of things (a
    /// character made of shapes, a panel) relative to one point.
    /// Inside a camera, the offset is in world units.
    pub fn push_offset(&mut self, offset: Vec2) {
        self.saved_views.push(self.view);
        self.view = self.view.then(View { scale: 1.0, offset });
    }

    /// Undoes the last [`push_offset`](Canvas::push_offset).
    pub fn pop_offset(&mut self) {
        self.pop_camera(); // the same stack: undo the last push, whatever it was
    }

    /// The part of the world that's on the canvas now, with the current
    /// camera and offsets (the whole canvas, in pixels, without a camera).
    /// Handy for drawing only what can be seen.
    pub fn visible_area(&self) -> Rect {
        let top_left = self.view.unapply(Vec2::ZERO);
        let bottom_right = self
            .view
            .unapply(vec2(self.width as f32, self.height as f32));
        Rect::new(
            top_left.x,
            top_left.y,
            bottom_right.x - top_left.x,
            bottom_right.y - top_left.y,
        )
    }

    /// How positions become pixels right now (for helpers that draw in
    /// screen pixels for a moment, then put it back with `set_view`).
    pub(crate) fn view(&self) -> View {
        self.view
    }

    pub(crate) fn set_view(&mut self, view: View) {
        self.view = view;
    }

    /// Where a position given to the drawing functions ends up on the
    /// screen, with the current camera and offsets.
    #[cfg(test)]
    pub(crate) fn world_to_screen(&self, p: Vec2) -> Vec2 {
        self.view.apply(p)
    }

    /// Forgets every camera and offset. The engine calls this around each
    /// frame's `draw`.
    pub(crate) fn reset_view(&mut self) {
        self.view = View::SCREEN;
        self.saved_views.clear();
    }

    /// The first camera pushed since the last call, which starts afresh.
    /// Names what's in `rect`, for tools that can't see pixels. It draws
    /// nothing (except, in the debug overlay, an outline with the name).
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(320, 240);
    /// # let player = Rect::new(10.0, 10.0, 12.0, 12.0);
    /// canvas.fill_rect(player, Color::YELLOW);
    /// canvas.label("player", player);
    /// ```
    ///
    /// Inside `push_camera` the rectangle is in the world, like everything
    /// else drawn there, and the engine works out where it is on screen.
    /// Headless runs save every label (with both positions) in
    /// `*.scene.json` files, `--describe` lists them in words, and tests
    /// can read them with [`Simulation::scene`](crate::Simulation::scene).
    pub fn label(&mut self, name: &str, rect: Rect) {
        let screen = self.view.apply_rect(rect);
        let canvas = Rect::new(0.0, 0.0, self.width as f32, self.height as f32);
        self.labels.push(Label {
            name: name.to_string(),
            world: rect,
            screen,
            on_screen: screen.overlaps(&canvas),
        });
    }

    /// Hands over the labels and text drawn since the last call.
    pub(crate) fn take_scene(&mut self) -> (Vec<Label>, Vec<SceneText>) {
        (
            std::mem::take(&mut self.labels),
            std::mem::take(&mut self.texts),
        )
    }

    pub(crate) fn take_frame_camera(&mut self) -> Option<Camera2D> {
        self.frame_camera.take()
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
    /// Starts drawing in front of all the 3D drawn so far: the depth
    /// buffer is emptied (on the GPU, for its next batch, after drawing what
    /// it has).
    pub(crate) fn forget_depth(&mut self) {
        self.depth.fill(0.0);
        #[cfg(feature = "gpu")]
        if let GpuSlot::Running(gpu) = &mut self.gpu {
            gpu.flush(&mut self.pixels);
            gpu.forget_depth();
        }
    }

    /// Called before drawing each see-through mesh on the CPU: from now on,
    /// pixels it paints are marked with a new number.
    pub(crate) fn start_see_through_mesh(&mut self) {
        let size = self.width * self.height;
        if self.see_through_marks.len() != size {
            self.see_through_marks = vec![0; size];
        }
        self.see_through_mark = self.see_through_mark.wrapping_add(1);
        if self.see_through_mark == 0 {
            // Every number used: start again from clean marks.
            self.see_through_marks.fill(0);
            self.see_through_mark = 1;
        }
    }

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
        tint: Color,
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
                let look = crate::gpu::Look {
                    lighting: self.lighting,
                    fog: self.fog,
                    tint,
                };
                gpu.draw(mesh, transform, camera, look, &mut self.pixels);
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
        self.drawing_in_view = false;
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
        let (x, y) = if self.view.is_screen() {
            (x, y)
        } else {
            let p = self.view.apply(vec2(x as f32, y as f32));
            (p.x.floor() as i32, p.y.floor() as i32)
        };
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
        let rect = self.view.apply_rect(rect);
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
        let (center, radius) = (self.view.apply(center), radius * self.view.scale);
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
        let (from, to) = (self.view.apply(from), self.view.apply(to));
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
        let [a, b, c] = [a, b, c].map(|p| self.view.apply(p));
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
        opacity: u8,
    ) {
        let c32 = color.to_u32();
        let fog = self.fog.map(|f| (f, f.color.to_u32()));
        // Borrow the two buffers separately, so the closure can change both.
        let (pixels, depth) = (&mut self.pixels, &mut self.depth);
        if opacity < 255 {
            // See-through: mixed over what's there, behind nearer things,
            // but it doesn't hide what's drawn after it (no depth written).
            // Each pixel once per mesh, even on an edge two triangles share.
            let (marks, mark) = (&mut self.see_through_marks, self.see_through_mark);
            for_each_pixel_in_triangle(self.width, self.height, corners, |i, weights| {
                let d = weights[0] * inverse_depths[0]
                    + weights[1] * inverse_depths[1]
                    + weights[2] * inverse_depths[2];
                if d > depth[i] && marks[i] != mark {
                    marks[i] = mark;
                    let color = fogged(c32, fog, d);
                    pixels[i] = crate::color::blend(pixels[i], color, opacity);
                }
            });
            return;
        }
        for_each_pixel_in_triangle(self.width, self.height, corners, |i, weights| {
            // Blend the corners' depths by how close this pixel is to each corner.
            let d = weights[0] * inverse_depths[0]
                + weights[1] * inverse_depths[1]
                + weights[2] * inverse_depths[2];
            if d > depth[i] {
                depth[i] = d;
                pixels[i] = fogged(c32, fog, d);
            }
        });
    }

    /// Fills a textured 3D triangle (see `render3d.rs`): `uv_over_z` is each
    /// corner's texture point divided by its distance, `shade` what the
    /// texture's red, green and blue are multiplied by. See-through texture
    /// pixels (alpha below half) are left out, depth and all.
    pub(crate) fn fill_triangle_3d_textured(
        &mut self,
        corners: [Vec2; 3],
        inverse_depths: [f32; 3],
        uv_over_z: [Vec2; 3],
        texture: &Sprite,
        shade: [f32; 3],
        opacity: u8,
    ) {
        let (tw, th) = (texture.width(), texture.height());
        if tw == 0 || th == 0 {
            return;
        }
        // The shade as whole numbers out of 65,536, so each pixel's colour
        // is integer arithmetic (much quicker than rounding floats).
        let shade = shade.map(|k| (k.clamp(0.0, 255.0) * 65536.0).round() as u32);
        let (texture_rgb, texture_alpha, row, start) = texture.texels();
        let fog = self.fog.map(|f| (f, f.color.to_u32()));
        let (pixels, depth) = (&mut self.pixels, &mut self.depth);
        let (marks, mark) = (&mut self.see_through_marks, self.see_through_mark);
        for_each_pixel_in_triangle(self.width, self.height, corners, |i, weights| {
            let d = weights[0] * inverse_depths[0]
                + weights[1] * inverse_depths[1]
                + weights[2] * inverse_depths[2];
            if d <= depth[i] || d <= 0.0 {
                return;
            }
            // Back from "texture point / distance" to the texture point
            // (one division, then two multiplications: quicker).
            let to_texture = 1.0 / d;
            let u = (weights[0] * uv_over_z[0].x
                + weights[1] * uv_over_z[1].x
                + weights[2] * uv_over_z[2].x)
                * to_texture;
            let v = (weights[0] * uv_over_z[0].y
                + weights[1] * uv_over_z[1].y
                + weights[2] * uv_over_z[2].y)
                * to_texture;
            // The nearest texture pixel, repeating past the edges.
            let x = wrap((u * tw as f32).floor(), tw);
            let y = wrap((v * th as f32).floor(), th);
            let at = start + y * row + x;
            let (rgb, alpha) = (texture_rgb[at], texture_alpha[at]);
            if alpha < 128 {
                return; // see-through: not drawn, and nothing hidden behind it
            }
            let channel = |shift: u32, k: u32| {
                let c = ((rgb >> shift) & 0xff) as u64 * k as u64;
                (((c + 32768) >> 16).min(255) as u32) << shift
            };
            let color = channel(16, shade[0]) | channel(8, shade[1]) | channel(0, shade[2]);
            let color = fogged(color, fog, d);
            if opacity < 255 {
                // See-through: no depth, and each pixel once per mesh.
                if marks[i] != mark {
                    marks[i] = mark;
                    pixels[i] = crate::color::blend(pixels[i], color, opacity);
                }
            } else {
                depth[i] = d;
                pixels[i] = color;
            }
        });
    }

    /// Draws a sprite with its top-left corner at `pos`.
    ///
    /// `blit` says how: mirrored, bigger, tinted or see-through. Use
    /// `Blit::default()` to draw it as it is, or set only what you need:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(64, 64);
    /// let duck = Sprite::from_ascii(&["yy.", "yyo"], &[('y', Color::YELLOW), ('o', Color::ORANGE)]);
    /// canvas.blit_sprite(&duck, vec2(10.0, 20.0), Blit::default());
    /// canvas.blit_sprite(&duck, vec2(30.0, 20.0), Blit { flip_x: true, scale: 4.0, ..Default::default() });
    /// ```
    ///
    /// Scaling keeps pixels as sharp squares (each screen pixel takes the
    /// color of the nearest sprite pixel), and whatever falls outside the
    /// canvas is cut off.
    pub fn blit_sprite(&mut self, sprite: &Sprite, pos: Vec2, mut blit: Blit) {
        let pos = self.view.apply(pos);
        blit.scale *= self.view.scale;
        let (sprite_w, sprite_h) = (sprite.width(), sprite.height());
        let opacity = if blit.opacity.is_nan() {
            0.0
        } else {
            blit.opacity.clamp(0.0, 1.0)
        };
        if sprite_w == 0
            || sprite_h == 0
            || blit.scale.is_nan()
            || blit.scale <= 0.0
            || opacity == 0.0
            || blit.tint.a == 0
        {
            return; // nothing to draw
        }
        // The sprite's size and position on the canvas, in whole pixels.
        // (Computed as f64 and clamped, so huge numbers can't overflow.)
        let to_pixels = |v: f64| v.round().clamp(-1e9, 1e9) as i64;
        let dest_w = to_pixels(sprite_w as f64 * blit.scale as f64);
        let dest_h = to_pixels(sprite_h as f64 * blit.scale as f64);
        let left = to_pixels(pos.x as f64);
        let top = to_pixels(pos.y as f64);
        // Clip: only the part that's on the canvas.
        let x0 = left.max(0);
        let x1 = (left + dest_w).min(self.width as i64);
        let y0 = top.max(0);
        let y1 = (top + dest_h).min(self.height as i64);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        self.flush_3d();
        let (x0, x1, y0, y1) = (x0 as usize, x1 as usize, y0 as usize, y1 as usize);
        let (left, top) = (left as isize, top as isize);

        // Fast path: drawn as it is, so each row is one plain copy.
        let as_is = !blit.flip_x
            && !blit.flip_y
            && dest_w == sprite_w as i64
            && dest_h == sprite_h as i64
            && blit.tint == Color::WHITE
            && opacity == 1.0;
        if as_is && sprite.is_opaque() {
            for y in y0..y1 {
                let (rgb, _) = sprite.row((y as isize - top) as usize);
                let from = (x0 as isize - left) as usize;
                let to = y * self.width;
                self.pixels[to + x0..to + x1].copy_from_slice(&rgb[from..from + (x1 - x0)]);
            }
            return;
        }

        // Which sprite column each canvas column shows. Nearest neighbour:
        // the sprite pixel under the center of the canvas pixel.
        let pick = |screen: usize, start: isize, dest: i64, size: usize, flip: bool| {
            let offset = (screen as isize - start) as f64 + 0.5;
            let i = ((offset * size as f64 / dest as f64) as usize).min(size - 1);
            if flip { size - 1 - i } else { i }
        };
        let columns: Vec<usize> = (x0..x1)
            .map(|x| pick(x, left, dest_w, sprite_w, blit.flip_x))
            .collect();
        let tint = blit.tint;
        // Tint and opacity, as whole numbers out of 255.
        let opacity = (opacity * blit.tint.a as f32).round() as u32;
        let tinted = |rgb: u32| {
            if tint == Color::WHITE {
                return rgb;
            }
            let channel =
                |shift: u32, t: u8| ((((rgb >> shift) & 0xff) * t as u32 + 127) / 255) << shift;
            channel(16, tint.r) | channel(8, tint.g) | channel(0, tint.b)
        };
        for y in y0..y1 {
            let (rgb, alpha) = sprite.row(pick(y, top, dest_h, sprite_h, blit.flip_y));
            let row = &mut self.pixels[y * self.width + x0..y * self.width + x1];
            for (pixel, &column) in row.iter_mut().zip(&columns) {
                let a = (alpha[column] as u32 * opacity + 127) / 255;
                if a > 0 {
                    paint(pixel, tinted(rgb[column]), a as u8);
                }
            }
        }
    }

    /// Draws text with the built-in pixel font. `pos` is the top-left
    /// corner; `scale` makes each font pixel a `scale` x `scale` square.
    ///
    /// Every printable ASCII character has a glyph: capitals and digits
    /// are 5 pixels tall, lowercase letters 4 (`g j p q y` hang one pixel
    /// lower). A `'\n'` starts a new line, [`Canvas::line_height`] below:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(100, 40);
    /// canvas.draw_text("Score: 120\nLives: 3", vec2(4.0, 4.0), 2, Color::WHITE);
    /// ```
    pub fn draw_text(&mut self, text: &str, pos: Vec2, scale: u32, color: Color) {
        // Remember each line and where it is on screen, for scene files.
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let top_left = vec2(pos.x, pos.y + i as f32 * Canvas::line_height(scale));
            let area = Rect::new(
                top_left.x,
                top_left.y,
                Canvas::text_width(line, scale),
                Canvas::text_height(scale),
            );
            self.texts.push(SceneText {
                text: line.trim_end_matches('\r').to_string(),
                screen: self.view.apply_rect(area),
            });
        }
        let s = scale as f32;
        let mut cursor = pos;

        for c in text.chars() {
            if c == '\n' {
                cursor.x = pos.x;
                cursor.y += Canvas::line_height(scale);
                continue;
            }
            if c == '\r' {
                continue; // the first half of a Windows "\r\n" line ending
            }

            let glyph = font::glyph(c);
            for row in 0..font::GLYPH_ROWS {
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
    ///
    /// It's centered on the *screen* even inside a camera (then `y` and the
    /// size are in world units, as for `draw_text`).
    pub fn draw_text_centered(&mut self, text: &str, y: f32, scale: u32, color: Color) {
        let line_height = Canvas::line_height(scale);
        for (i, line) in text.lines().enumerate() {
            let x = if self.view.is_screen() {
                (self.width as f32 - Canvas::text_width(line, scale)) / 2.0
            } else {
                // Where the text must start, in screen pixels, then back to world units.
                let on_screen = Canvas::text_width(line, scale) * self.view.scale;
                let left = (self.width as f32 - on_screen) / 2.0;
                self.view.unapply(vec2(left, 0.0)).x
            };
            self.draw_text(line, vec2(x, y + i as f32 * line_height), scale, color);
        }
    }

    /// How many pixels tall one line of text is at `scale`: the height of a
    /// capital letter (5 pixels at scale 1). The tails of `g j p q y` hang
    /// one font pixel below it.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// // Center a line of text vertically on y = 120:
    /// let top = 120.0 - Canvas::text_height(2) / 2.0;
    /// # assert_eq!(top, 115.0);
    /// ```
    pub fn text_height(scale: u32) -> f32 {
        font::GLYPH_HEIGHT as f32 * scale as f32
    }

    /// How far down each new line of text starts, at `scale` (7 pixels at
    /// scale 1: a line of text plus a 2-pixel gap). `draw_text` uses this
    /// for `'\n'`, so the second line's top is at `pos.y + line_height`.
    pub fn line_height(scale: u32) -> f32 {
        font::LINE_HEIGHT as f32 * scale as f32
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

/// A 3D pixel's colour (`0x00RRGGBB`) with the fog in front of it mixed
/// in: `d` is 1 / its distance ahead of the camera. Unchanged with no fog.
#[inline]
fn fogged(color: u32, fog: Option<(crate::render3d::Fog, u32)>, d: f32) -> u32 {
    match fog {
        None => color,
        Some((fog, fog_rgb)) => match fog.amount(1.0 / d) {
            0 => color,
            amount => crate::color::blend(color, fog_rgb, amount),
        },
    }
}

/// A texture pixel number, wrapped round into `0..size` (repeating
/// textures). Usually it's already inside, which is checked first because
/// it's much quicker than the general case.
#[inline]
fn wrap(position: f32, size: usize) -> usize {
    if position >= 0.0 && position < size as f32 {
        position as usize
    } else {
        (position as i64).rem_euclid(size as i64) as usize
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
        assert_eq!(
            canvas.get_pixel(2, 2),
            Some(Color::rgb(0, 0, 255)),
            "outside the rect"
        );

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
            assert_eq!(
                c.get_pixel(x, y),
                mixed,
                "outline pixel ({x}, {y}) painted once"
            );
        }
        assert_eq!(
            c.get_pixel(3, 3),
            Some(Color::rgb(0, 0, 255)),
            "inside stays empty"
        );
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
            c.fill_triangle(
                vec2(1.0, 29.0),
                vec2(15.0, 18.0),
                vec2(9.0, 29.0),
                color(9, 9, 9),
            );
            c.draw_text("OK!", vec2(25.0, 2.0), 2, color(255, 255, 255));
            c
        };
        let old = scene(Color::rgb);
        let new = scene(|r, g, b| Color::rgba(r, g, b, 255));
        assert_eq!(old.pixels, new.pixels);
    }

    /// A 3 x 2 sprite with six different colors:
    /// ```text
    /// r g b
    /// w . y      ('.' is see-through)
    /// ```
    fn six() -> Sprite {
        Sprite::from_ascii(
            &["rgb", "w.y"],
            &[
                ('r', Color::rgb(255, 0, 0)),
                ('g', Color::rgb(0, 255, 0)),
                ('b', Color::rgb(0, 0, 255)),
                ('w', Color::rgb(255, 255, 255)),
                ('y', Color::rgb(255, 255, 0)),
            ],
        )
    }

    /// The canvas as rows of letters, using the same letters as `six()`,
    /// and `-` for the background (black).
    fn letters(canvas: &Canvas) -> Vec<String> {
        let name = |c: Color| match (c.r, c.g, c.b) {
            (255, 0, 0) => 'r',
            (0, 255, 0) => 'g',
            (0, 0, 255) => 'b',
            (255, 255, 255) => 'w',
            (255, 255, 0) => 'y',
            (0, 0, 0) => '-',
            _ => '?',
        };
        (0..canvas.height as i32)
            .map(|y| {
                (0..canvas.width as i32)
                    .map(|x| name(canvas.get_pixel(x, y).unwrap()))
                    .collect()
            })
            .collect()
    }

    fn blitted(width: usize, height: usize, pos: Vec2, blit: Blit) -> Vec<String> {
        let mut canvas = Canvas::new(width, height);
        canvas.blit_sprite(&six(), pos, blit);
        letters(&canvas)
    }

    #[test]
    fn sprites_draw_with_see_through_pixels() {
        assert_eq!(
            blitted(5, 3, vec2(1.0, 1.0), Blit::default()),
            ["-----", "-rgb-", "-w-y-"]
        );
    }

    #[test]
    fn sprites_flip() {
        let flip_x = Blit {
            flip_x: true,
            ..Default::default()
        };
        assert_eq!(blitted(3, 2, Vec2::ZERO, flip_x), ["bgr", "y-w"]);
        let flip_y = Blit {
            flip_y: true,
            ..Default::default()
        };
        assert_eq!(blitted(3, 2, Vec2::ZERO, flip_y), ["w-y", "rgb"]);
        let both = Blit {
            flip_x: true,
            flip_y: true,
            ..Default::default()
        };
        assert_eq!(blitted(3, 2, Vec2::ZERO, both), ["y-w", "bgr"]);
    }

    #[test]
    fn sprites_clip_at_every_edge() {
        assert_eq!(
            blitted(3, 2, vec2(-1.0, 0.0), Blit::default()),
            ["gb-", "-y-"]
        );
        assert_eq!(
            blitted(3, 2, vec2(1.0, 1.0), Blit::default()),
            ["---", "-rg"]
        );
        assert_eq!(
            blitted(3, 2, vec2(-2.0, -1.0), Blit::default()),
            ["y--", "---"]
        );
        // Entirely off the canvas, or at absurd positions: nothing, no panic.
        assert_eq!(
            blitted(3, 2, vec2(-3.0, 0.0), Blit::default()),
            ["---", "---"]
        );
        assert_eq!(
            blitted(3, 2, vec2(1e30, -1e30), Blit::default()),
            ["---", "---"]
        );
        assert_eq!(
            blitted(3, 2, vec2(f32::NAN, 0.0), Blit::default()),
            ["rgb", "w-y"]
        );
        // Clipped and flipped together.
        let flip_x = Blit {
            flip_x: true,
            ..Default::default()
        };
        assert_eq!(blitted(3, 2, vec2(-1.0, 0.0), flip_x), ["gr-", "-w-"]);
    }

    #[test]
    fn sprites_scale_with_sharp_pixels() {
        let double = Blit {
            scale: 2.0,
            ..Default::default()
        };
        assert_eq!(
            blitted(6, 4, Vec2::ZERO, double),
            ["rrggbb", "rrggbb", "ww--yy", "ww--yy"]
        );
        // Scaled, flipped and clipped at once.
        let both = Blit {
            scale: 2.0,
            flip_x: true,
            ..Default::default()
        };
        assert_eq!(
            blitted(4, 3, vec2(-1.0, 1.0), both),
            ["----", "bggr", "bggr"]
        );
        // Shrunk to half: each canvas pixel shows the sprite pixel nearest its center.
        let half = Blit {
            scale: 0.5,
            ..Default::default()
        };
        let mut canvas = Canvas::new(2, 1);
        let big = Sprite::from_ascii(
            &["rrbb", "rrbb"],
            &[('r', Color::rgb(255, 0, 0)), ('b', Color::rgb(0, 0, 255))],
        );
        canvas.blit_sprite(&big, Vec2::ZERO, half);
        assert_eq!(letters(&canvas), ["rb"]);
        // Zero, negative or NaN scale draws nothing.
        for scale in [0.0, -1.0, f32::NAN] {
            assert_eq!(
                blitted(
                    3,
                    2,
                    Vec2::ZERO,
                    Blit {
                        scale,
                        ..Default::default()
                    }
                ),
                ["---", "---"]
            );
        }
    }

    #[test]
    fn sprites_tint_and_fade() {
        let mut canvas = Canvas::new(3, 2);
        canvas.clear(Color::rgb(0, 0, 255));
        let red_only = Blit {
            tint: Color::rgb(255, 0, 0),
            ..Default::default()
        };
        canvas.blit_sprite(&six(), Vec2::ZERO, red_only);
        assert_eq!(
            canvas.get_pixel(0, 0),
            Some(Color::rgb(255, 0, 0)),
            "red stays red"
        );
        assert_eq!(
            canvas.get_pixel(1, 0),
            Some(Color::rgb(0, 0, 0)),
            "green loses all"
        );
        assert_eq!(
            canvas.get_pixel(0, 1),
            Some(Color::rgb(255, 0, 0)),
            "white keeps the red"
        );
        assert_eq!(
            canvas.get_pixel(1, 1),
            Some(Color::rgb(0, 0, 255)),
            "see-through stays"
        );

        let mut canvas = Canvas::new(3, 2);
        canvas.clear(Color::rgb(0, 0, 255));
        let half = Blit {
            opacity: 128.0 / 255.0,
            ..Default::default()
        };
        canvas.blit_sprite(&six(), Vec2::ZERO, half);
        assert_eq!(
            canvas.get_pixel(0, 0),
            Some(Color::rgb(128, 0, 127)),
            "half red over blue"
        );

        // A see-through pixel in the sprite itself blends too.
        let glass = Sprite::from_pixels(1, 1, vec![Color::rgba(255, 0, 0, 128)]);
        let mut canvas = Canvas::new(1, 1);
        canvas.clear(Color::rgb(0, 0, 255));
        canvas.blit_sprite(&glass, Vec2::ZERO, Blit::default());
        assert_eq!(canvas.get_pixel(0, 0), Some(Color::rgb(128, 0, 127)));

        // Opacity 0 and NaN draw nothing.
        for opacity in [0.0, f32::NAN] {
            assert_eq!(
                blitted(
                    3,
                    2,
                    Vec2::ZERO,
                    Blit {
                        opacity,
                        ..Default::default()
                    }
                ),
                ["---", "---"]
            );
        }
    }

    #[test]
    fn the_fast_copy_matches_the_careful_path() {
        // A solid sprite drawn as it is takes the fast row copy. It must
        // give the same pixels as setting each pixel by hand, wherever it
        // is (cut from a bigger sheet, clipped, at fractional positions).
        let sheet = Sprite::from_pixels(
            7,
            5,
            (0..35)
                .map(|i| Color::rgb(i as u8 * 7, 255 - i as u8, i as u8))
                .collect(),
        );
        let frame = sheet.sub(1, 1, 5, 3);
        for pos in [
            vec2(0.0, 0.0),
            vec2(-2.0, 1.0),
            vec2(4.0, -1.0),
            vec2(2.4, 2.6),
        ] {
            let mut fast = Canvas::new(8, 6);
            fast.blit_sprite(&frame, pos, Blit::default());
            let mut slow = Canvas::new(8, 6);
            let (left, top) = (pos.x.round() as i32, pos.y.round() as i32);
            for y in 0..frame.height() {
                for x in 0..frame.width() {
                    let c = frame.get_pixel(x, y).unwrap();
                    slow.set_pixel(left + x as i32, top + y as i32, c);
                }
            }
            assert_eq!(fast.pixels, slow.pixels, "at {pos:?}");
        }
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
        canvas.fill_triangle_3d(corners, [0.5; 3], Color::RED, 255); // 2 units away
        canvas.fill_triangle_3d(corners, [0.1; 3], Color::BLUE, 255); // 10 units away: hidden
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::RED));
        canvas.fill_triangle_3d(corners, [1.0; 3], Color::GREEN, 255); // 1 unit away: in front
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::GREEN));
        canvas.clear(Color::BLACK); // clearing resets depth too
        canvas.fill_triangle_3d(corners, [0.1; 3], Color::BLUE, 255);
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

//! What the engine knows about each frame, recorded as it's drawn: the 3D
//! camera, where each named thing is, and how each line of text looks
//! (its colours, outline, box, and the pixels behind it). The review uses
//! this truth instead of guessing from pixels.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::{Rect, Vec3, vec3};
use crate::render3d::{Camera3D, Mesh, Transform};

/// What the canvas has noted since the frame began.
#[derive(Default)]
pub(crate) struct Truth {
    /// Also note how text looks and what's behind it (`--review`). The
    /// rest is cheap and always noted.
    pub(crate) recording: bool,
    /// The first camera the world was drawn with this frame.
    pub(crate) camera: Option<Camera3D>,
    /// Named models: name, and their box in the world.
    pub(crate) models: Vec<(String, Vec3, Vec3)>,
    /// How each line of text drawn looked.
    pub(crate) looks: Vec<TextLook>,
    /// Places the camera was inside something solid ("a block at 3, 4, 5").
    pub(crate) inside: Vec<String>,
}

/// How one line of text looked when it was drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLook {
    pub text: String,
    /// Where it is on the screen, in pixels.
    pub rect: Rect,
    pub color: Color,
    /// Its outline's colour and thickness in pixels, if it has one.
    pub outline: Option<(Color, f32)>,
    /// Its drop shadow's colour, if it has one.
    pub shadow: Option<Color>,
    /// The box behind it, if it has one.
    pub box_color: Option<Color>,
    /// How bright the pixels behind it were just before it was drawn, as
    /// relative luminance (0 black, 1 white): the darkest tenth, the
    /// middle, and the brightest tenth.
    pub behind: [f32; 3],
}

/// One named thing in one frame: a label (`canvas.label`) or a model
/// (`canvas.label_model`).
#[derive(Debug, Clone, PartialEq)]
pub struct Thing {
    pub name: String,
    /// Where it is on the screen, in pixels (it may reach past the edges).
    pub screen: Rect,
    /// The part of `screen` inside the picture.
    pub visible: Rect,
    /// Its rectangle as the game gave it (in the world, through a camera);
    /// for a model, the x and y of its box.
    pub world: Rect,
    /// How bright its visible pixels are on average, once the frame is
    /// drawn (0 black to 1 white); 0 when none is visible.
    pub light: f32,
    /// For a model: its box in the world.
    pub world_box: Option<(Vec3, Vec3)>,
}

impl Thing {
    pub fn on_screen(&self) -> bool {
        self.visible.w >= 1.0 && self.visible.h >= 1.0
    }
}

/// Everything known about one drawn frame.
#[derive(Debug, Clone, Default)]
pub struct FrameTruth {
    /// The camera the world was drawn with, if any 3D was drawn.
    pub camera: Option<Camera3D>,
    pub things: Vec<Thing>,
    pub texts: Vec<TextLook>,
    /// Where the camera was inside something solid.
    pub inside: Vec<String>,
    /// How long the frame took to draw, in milliseconds.
    pub draw_ms: f32,
}

/// sRGB 0-255 to linear light, worked out once.
pub(crate) fn linear(c: u8) -> f32 {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0.0; 256];
        for (i, v) in table.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        table
    })[c as usize]
}

/// Relative luminance (as WCAG measures it) of a `0xRRGGBB` pixel.
pub(crate) fn luminance(pixel: u32) -> f32 {
    0.2126 * linear((pixel >> 16) as u8)
        + 0.7152 * linear((pixel >> 8) as u8)
        + 0.0722 * linear(pixel as u8)
}

/// Relative luminance of a colour.
pub(crate) fn color_luminance(c: Color) -> f32 {
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// How bright a `0xRRGGBB` pixel looks, 0 to 1 (gamma luma).
pub(crate) fn luma(pixel: u32) -> f32 {
    let (r, g, b) = ((pixel >> 16) & 255, (pixel >> 8) & 255, pixel & 255);
    (299 * r + 587 * g + 114 * b) as f32 / 255_000.0
}

/// The contrast ratio between two luminances (1 to 21).
pub(crate) fn contrast(a: f32, b: f32) -> f32 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// `rect` cut down to a `width` x `height` picture.
pub(crate) fn clip(rect: Rect, width: f32, height: f32) -> Rect {
    if !(rect.x.is_finite() && rect.y.is_finite() && rect.w.is_finite() && rect.h.is_finite()) {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    let x0 = rect.x.max(0.0);
    let y0 = rect.y.max(0.0);
    let x1 = (rect.x + rect.w).min(width);
    let y1 = (rect.y + rect.h).min(height);
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// Up to about `most` pixels of `rect`, spread evenly over it.
fn sample(pixels: &[u32], width: usize, height: usize, rect: Rect, most: usize) -> Vec<u32> {
    let r = clip(rect, width as f32, height as f32);
    let (x0, y0) = (r.x.floor() as usize, r.y.floor() as usize);
    let (x1, y1) = (
        ((r.x + r.w).ceil() as usize).min(width),
        ((r.y + r.h).ceil() as usize).min(height),
    );
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let area = (x1 - x0) * (y1 - y0);
    let step = ((area as f32 / most as f32).sqrt().ceil() as usize).max(1);
    let mut out = Vec::new();
    for y in (y0..y1).step_by(step) {
        for x in (x0..x1).step_by(step) {
            out.push(pixels[y * width + x]);
        }
    }
    out
}

/// The average luma of `rect`'s pixels (0 when none is in the picture).
pub(crate) fn mean_luma(canvas: &Canvas, rect: Rect) -> f32 {
    let found = sample(canvas.pixels(), canvas.width(), canvas.height(), rect, 1024);
    if found.is_empty() {
        return 0.0;
    }
    found.iter().map(|&p| luma(p)).sum::<f32>() / found.len() as f32
}

/// A game may draw an outline or a drop shadow as dark copies of the
/// words, a little off, before the words themselves (or draw the words
/// twice, for bold): those copies are part of one line, not more lines.
/// The line keeps what was behind the first copy, before any was painted.
fn fold_shadow_copies(looks: Vec<TextLook>) -> Vec<TextLook> {
    let mut keep: Vec<Option<TextLook>> = looks.into_iter().map(Some).collect();
    // For each line: how many copies of another colour are folded into
    // it, and how far off the furthest is.
    let mut around: Vec<(usize, f32)> = vec![(0, 0.0); keep.len()];
    for j in 0..keep.len() {
        let Some(line) = keep[j].clone() else {
            continue;
        };
        let near = (line.rect.h * 0.5).max(2.0);
        let mut earliest_behind = None;
        for i in 0..j {
            let Some(copy) = &keep[i] else { continue };
            let (dx, dy) = (copy.rect.x - line.rect.x, copy.rect.y - line.rect.y);
            if copy.text != line.text
                || dx.abs() > near
                || dy.abs() > near
                || (copy.rect.h - line.rect.h).abs() > 1.0
            {
                continue;
            }
            let off = dx.hypot(dy);
            earliest_behind.get_or_insert(copy.behind);
            let copy = keep[i].take().expect("checked above");
            let (count, far) = around[i];
            if copy.color == line.color {
                // Bold: the same words again. Whatever was round the copy
                // is round the line.
                around[j].0 += count;
                around[j].1 = around[j].1.max(far);
                if let Some(l) = &mut keep[j] {
                    l.outline = l.outline.or(copy.outline);
                    l.shadow = l.shadow.or(copy.shadow);
                }
            } else {
                around[j].0 += count.max(1);
                around[j].1 = around[j].1.max(far).max(off);
                if let Some(l) = &mut keep[j] {
                    l.shadow.get_or_insert(copy.color);
                }
            }
        }
        if let (Some(behind), Some(l)) = (earliest_behind, &mut keep[j]) {
            l.behind = behind;
        }
        // Copies all round it: an outline, as thick as they're far.
        let (count, far) = around[j];
        if count >= 3 {
            if let Some(l) = &mut keep[j] {
                if l.outline.is_none() {
                    let color = l.shadow.unwrap_or(Color::BLACK);
                    l.outline = Some((color, far));
                }
            }
        }
    }
    keep.into_iter().flatten().collect()
}

impl Canvas {
    /// Names a 3D model, as [`label`](Canvas::label) names a rectangle:
    /// for `--review` (is it on screen, big enough, lit?), the scene files
    /// and `--describe`. Its label is the box round it on the screen; its
    /// box in the world is kept too, so the review notices the camera
    /// going inside it. It draws nothing.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(320, 240);
    /// let camera = Camera3D::looking_at(vec3(0.0, 1.0, -4.0), vec3(0.0, 0.5, 0.0));
    /// let host = Mesh::cube(Color::YELLOW);
    /// let at = Transform::at(vec3(0.0, 0.5, 0.0));
    /// canvas.draw_mesh(&host, &at, &camera);
    /// canvas.label_model("host", &host, &at, &camera);
    /// ```
    pub fn label_model(
        &mut self,
        name: &str,
        mesh: &Mesh,
        transform: &Transform,
        camera: &Camera3D,
    ) {
        let (low, high) = mesh.bounds();
        let corners: Vec<Vec3> = (0..8)
            .map(|i| {
                transform.apply(vec3(
                    if i & 1 == 0 { low.x } else { high.x },
                    if i & 2 == 0 { low.y } else { high.y },
                    if i & 4 == 0 { low.z } else { high.z },
                ))
            })
            .collect();
        let world_low = corners.iter().fold(corners[0], |a, &p| {
            vec3(a.x.min(p.x), a.y.min(p.y), a.z.min(p.z))
        });
        let world_high = corners.iter().fold(corners[0], |a, &p| {
            vec3(a.x.max(p.x), a.y.max(p.y), a.z.max(p.z))
        });
        let seen: Vec<_> = corners
            .iter()
            .filter_map(|&p| camera.world_to_screen(p, self))
            .collect();
        let screen = if seen.is_empty() {
            Rect::new(-1.0e6, -1.0e6, 0.0, 0.0) // all behind the camera
        } else {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in &seen {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        };
        let canvas = Rect::new(0.0, 0.0, self.width() as f32, self.height() as f32);
        self.add_label(crate::on_screen::Label {
            name: name.to_string(),
            world: Rect::new(
                world_low.x,
                world_low.y,
                world_high.x - world_low.x,
                world_high.y - world_low.y,
            ),
            screen,
            on_screen: !seen.is_empty() && screen.overlaps(&canvas),
        });
        self.truth
            .models
            .push((name.to_string(), world_low, world_high));
    }

    /// Notes the world camera (the first of the frame).
    pub(crate) fn note_camera(&mut self, camera: &Camera3D) {
        if self.truth.camera.is_none() {
            self.truth.camera = Some(*camera);
        }
    }

    /// Notes that the camera is inside something solid.
    pub(crate) fn note_inside(&mut self, what: String) {
        if self.truth.inside.len() < 8 {
            self.truth.inside.push(what);
        }
    }

    /// Whether text looks are being noted (`--review`).
    pub(crate) fn noting_looks(&self) -> bool {
        self.truth.recording
    }

    /// Notes how a line of text looks, and measures the pixels behind it
    /// (call it before the text is painted).
    pub(crate) fn note_look(&mut self, mut look: TextLook) {
        if !self.truth.recording {
            return;
        }
        self.flush_3d(); // the 3D is behind the text too
        let mut found: Vec<f32> =
            sample(self.pixels(), self.width(), self.height(), look.rect, 600)
                .into_iter()
                .map(luminance)
                .collect();
        found.sort_by(f32::total_cmp);
        if !found.is_empty() {
            let at = |share: f32| found[((found.len() - 1) as f32 * share).round() as usize];
            look.behind = [at(0.1), at(0.5), at(0.9)];
        }
        self.truth.looks.push(look);
    }

    /// Hands over what was noted this frame, as the frame's truth, with
    /// each label's brightness measured in the finished picture.
    pub(crate) fn take_truth(
        &mut self,
        labels: &[crate::on_screen::Label],
        draw_ms: f32,
    ) -> FrameTruth {
        let (w, h) = (self.width() as f32, self.height() as f32);
        let mut models = std::mem::take(&mut self.truth.models);
        let things = labels
            .iter()
            .map(|label| {
                let visible = if label.on_screen {
                    clip(label.screen, w, h)
                } else {
                    Rect::new(0.0, 0.0, 0.0, 0.0)
                };
                let world_box = models
                    .iter()
                    .position(|(name, ..)| *name == label.name)
                    .map(|i| {
                        let (_, low, high) = models.remove(i);
                        (low, high)
                    });
                Thing {
                    name: label.name.clone(),
                    screen: label.screen,
                    visible,
                    world: label.world,
                    light: if visible.w * visible.h > 0.0 {
                        mean_luma(self, visible)
                    } else {
                        0.0
                    },
                    world_box,
                }
            })
            .collect();
        FrameTruth {
            camera: self.truth.camera.take(),
            things,
            texts: fold_shadow_copies(std::mem::take(&mut self.truth.looks)),
            inside: std::mem::take(&mut self.truth.inside),
            draw_ms,
        }
    }

    /// Starts or stops noting text looks.
    pub(crate) fn record_looks(&mut self, on: bool) {
        self.truth.recording = on;
    }

    /// Forgets what was noted (a frame drawn but not reviewed).
    pub(crate) fn forget_truth(&mut self) {
        let recording = self.truth.recording;
        *self.truth = Truth {
            recording,
            ..Truth::default()
        };
    }
}

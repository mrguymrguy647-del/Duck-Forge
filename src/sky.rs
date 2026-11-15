//! A sky all round: a 360° panorama behind the 3D, turning with the
//! camera.
//!
//! The picture is an equirectangular panorama, the kind a 360° camera or
//! a sky generator saves: across is all the way round (the middle is
//! straight ahead at yaw 0, +z; the right quarter is to the right, +x),
//! and down is from straight up (the top row) to straight down (the
//! bottom row). Twice as wide as it is tall keeps it unstretched. Each
//! screen pixel shows the panorama's pixel in its direction, so looking
//! up shows the top of the picture and turning round shows all of it.
//!
//! ```
//! # use duckforge::prelude::*;
//! # let mut canvas = Canvas::new(90, 160);
//! # let camera = Camera3D::new(vec3(0.0, 2.0, 0.0));
//! # let world = Mesh::cube(Color::GRAY);
//! // let sky = Sprite::load("assets/night_sky.png")?;
//! let sky = Sprite::from_pixels(8, 4, vec![Color::rgb(20, 24, 48); 32]);
//! canvas.set_sky(&sky); // once (in `new`, or when the sky changes)
//! // Every frame, in `draw`:
//! canvas.clear(Color::BLACK);
//! canvas.draw_mesh(&world, &Transform::default(), &camera); // the sky goes in first
//! ```

use crate::canvas::Canvas;
use crate::render3d::Camera3D;
use crate::sprite::Sprite;

/// The sky waiting to be drawn, and what draws it (so games without a
/// sky don't carry the code).
#[derive(Debug, Clone)]
pub(crate) struct Sky {
    pub(crate) picture: Sprite,
    pub(crate) paint: fn(&mut Canvas, &Sprite, &Camera3D),
    /// Not drawn yet since the last `clear`.
    pub(crate) pending: bool,
}

impl Canvas {
    /// From now on, the 3D world is drawn in front of `panorama`, a 360°
    /// sky that turns with the camera (see the [module docs](crate::sky)).
    /// After each [`clear`](Canvas::clear), the first mesh drawn in the
    /// world paints the sky first, seen by that mesh's camera, over the
    /// whole canvas: so draw 2D backgrounds after the 3D, not before.
    pub fn set_sky(&mut self, panorama: &Sprite) {
        self.effects.sky = Some(Box::new(Sky {
            picture: panorama.clone(),
            paint: paint_sky,
            pending: true,
        }));
    }

    /// No sky any more: the 3D is drawn over whatever is there.
    pub fn remove_sky(&mut self) {
        self.effects.sky = None;
    }

    /// Paints `panorama` over the whole canvas now, as seen by `camera`
    /// (see the [module docs](crate::sky)): for drawing the sky yourself,
    /// once a frame before the 3D, instead of [`set_sky`](Canvas::set_sky).
    pub fn draw_sky(&mut self, panorama: &Sprite, camera: &Camera3D) {
        paint_sky(self, panorama, camera);
    }

    /// Paints the sky set with `set_sky` if it hasn't been since `clear`.
    pub(crate) fn sky_before_3d(&mut self, camera: &Camera3D) {
        let Some(sky) = self.effects.sky.as_mut() else {
            return;
        };
        if !sky.pending {
            return;
        }
        sky.pending = false;
        let (picture, paint) = (sky.picture.clone(), sky.paint);
        paint(self, &picture, camera);
    }
}

/// How many pixels apart the directions are worked out exactly; between
/// them they're blended (a sky has no sharp corners to lose).
const STEP: usize = 8;

/// Where on the panorama direction (x, y, z) is: across (0 to 1, the
/// middle straight ahead) and down (0 straight up to 1 straight down).
fn panorama_point(x: f32, y: f32, z: f32) -> (f32, f32) {
    let length = (x * x + y * y + z * z).sqrt().max(1e-12);
    let across = 0.5 + x.atan2(z) / std::f32::consts::TAU;
    let down = 0.5 - (y / length).clamp(-1.0, 1.0).asin() / std::f32::consts::PI;
    (across, down)
}

fn paint_sky(canvas: &mut Canvas, panorama: &Sprite, camera: &Camera3D) {
    let (width, height) = (canvas.width(), canvas.height());
    let (pw, ph) = (panorama.width(), panorama.height());
    if width == 0 || height == 0 || pw == 0 || ph == 0 {
        return;
    }
    // The direction through each grid point, as a point on the panorama.
    let focal = (height as f32 / 2.0) / (camera.usable_fov() / 2.0).tan();
    let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
    let (gw, gh) = (width.div_ceil(STEP) + 1, height.div_ceil(STEP) + 1);
    let (sin_p, cos_p) = camera.pitch.sin_cos();
    let (sin_y, cos_y) = camera.yaw.sin_cos();
    let mut grid = Vec::with_capacity(gw * gh);
    for gy in 0..gh {
        for gx in 0..gw {
            let (sx, sy) = ((gx * STEP) as f32 + 0.5, (gy * STEP) as f32 + 0.5);
            // Camera space, then turned by the pitch, then the yaw (as
            // `to_camera_space`, undone).
            let (x, y, z) = ((sx - cx) / focal, -(sy - cy) / focal, 1.0);
            let (y, z) = (y * cos_p + z * sin_p, -y * sin_p + z * cos_p);
            let (x, z) = (x * cos_y + z * sin_y, -x * sin_y + z * cos_y);
            grid.push(panorama_point(x, y, z));
        }
    }
    let (rgb, _, row, start) = panorama.texels();
    let pixels = canvas.pixels_mut();
    let rows_per_job = STEP * 8;
    let paint_rows = |first_row: usize, out: &mut [u32]| {
        for (dy, line) in out.chunks_mut(width).enumerate() {
            let y = first_row + dy;
            let (gy, fy) = (y / STEP, (y % STEP) as f32 / STEP as f32);
            for (gx, span) in line.chunks_mut(STEP).enumerate() {
                let at = |gx: usize, gy: usize| grid[gy * gw + gx];
                let (a, b, c, d) = (
                    at(gx, gy),
                    at(gx + 1, gy),
                    at(gx, gy + 1),
                    at(gx + 1, gy + 1),
                );
                // Across wraps round: keep the four corners on the same lap.
                let lap = |u: f32| u - (u - a.0).round();
                let (bu, cu, du) = (lap(b.0), lap(c.0), lap(d.0));
                let left = (a.0 + (cu - a.0) * fy, a.1 + (c.1 - a.1) * fy);
                let right = (bu + (du - bu) * fy, b.1 + (d.1 - b.1) * fy);
                for (dx, pixel) in span.iter_mut().enumerate() {
                    let fx = dx as f32 / STEP as f32;
                    let u = left.0 + (right.0 - left.0) * fx;
                    let v = left.1 + (right.1 - left.1) * fx;
                    let px = ((u.rem_euclid(1.0) * pw as f32) as usize).min(pw - 1);
                    let py = ((v * ph as f32).max(0.0) as usize).min(ph - 1);
                    *pixel = rgb[start + py * row + px] & 0x00ff_ffff;
                }
            }
        }
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8);
    if threads <= 1 || width * height < 250_000 {
        paint_rows(0, pixels);
        return;
    }
    std::thread::scope(|scope| {
        let paint_rows = &paint_rows;
        for (job, rows) in pixels.chunks_mut(rows_per_job * width).enumerate() {
            scope.spawn(move || paint_rows(job * rows_per_job, rows));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::math::{Vec3, vec3};
    use crate::render3d::{Mesh, Transform};

    /// A panorama 8 x 4: each eighth of the way round its own colour in
    /// the upper half (red straight ahead), and grey below the horizon.
    fn compass() -> Sprite {
        let colours = [
            0x000080, 0x008000, 0x008080, 0x800000, 0xff0000, 0x800080, 0x808000, 0x0000ff,
        ];
        let mut pixels = Vec::new();
        for y in 0..4 {
            for &c in &colours {
                pixels.push(if y < 2 {
                    Color::from_hex(c)
                } else {
                    Color::GRAY
                });
            }
        }
        Sprite::from_pixels(8, 4, pixels)
    }

    fn middle(canvas: &Canvas) -> Color {
        canvas
            .get_pixel(canvas.width() as i32 / 2, canvas.height() as i32 / 2)
            .unwrap()
    }

    #[test]
    fn the_sky_turns_with_the_camera() {
        let sky = compass();
        let mut canvas = Canvas::new(90, 160);
        let mut camera = Camera3D::new(Vec3::ZERO);
        camera.pitch = 0.1; // just above the horizon
        canvas.draw_sky(&sky, &camera);
        assert_eq!(
            middle(&canvas),
            Color::from_hex(0xff0000),
            "straight ahead: the middle"
        );
        // A quarter turn right: two eighths along.
        camera.yaw = std::f32::consts::FRAC_PI_2 + 0.1;
        canvas.draw_sky(&sky, &camera);
        assert_eq!(middle(&canvas), Color::from_hex(0x808000));
        // Right round: the picture's two ends meet behind you.
        camera.yaw = std::f32::consts::PI - 0.1;
        canvas.draw_sky(&sky, &camera);
        assert_eq!(middle(&canvas), Color::from_hex(0x0000ff));
        camera.yaw = std::f32::consts::PI + 0.1;
        canvas.draw_sky(&sky, &camera);
        assert_eq!(middle(&canvas), Color::from_hex(0x000080));
        // Looking down: below the horizon.
        camera.pitch = -0.6;
        canvas.draw_sky(&sky, &camera);
        assert_eq!(middle(&canvas), Color::GRAY);
        // The horizon crosses the screen level, half way down, at pitch 0.
        camera.pitch = 0.0;
        camera.yaw = 0.0;
        canvas.draw_sky(&sky, &camera);
        assert_eq!(canvas.get_pixel(45, 70), Some(Color::from_hex(0xff0000)));
        assert_eq!(canvas.get_pixel(45, 90), Some(Color::GRAY));
    }

    #[test]
    fn the_sky_is_behind_everything_and_drawn_once_a_frame() {
        let sky = Sprite::from_pixels(2, 2, vec![Color::BLUE; 4]);
        let camera = Camera3D::new(vec3(0.0, 0.0, -3.0));
        let cube = Mesh::cube(Color::RED).glowing();
        for lit in [false, true] {
            let mut canvas = Canvas::new(64, 48);
            canvas.set_smooth_shading(lit);
            canvas.set_sky(&sky);
            canvas.clear(Color::BLACK);
            canvas.draw_mesh(&cube, &Transform::default(), &camera);
            // A second mesh doesn't paint the sky over the first.
            canvas.draw_mesh(
                &cube,
                &Transform::at(vec3(1.5, 0.0, 0.0)).sized(0.3),
                &camera,
            );
            canvas.flush_3d();
            assert_eq!(middle(&canvas), Color::RED, "lit: {lit}");
            assert_eq!(canvas.get_pixel(1, 1), Some(Color::BLUE), "lit: {lit}");
            // Without 3D, nothing paints it: a 2D screen stays as it is.
            canvas.clear(Color::BLACK);
            assert_eq!(canvas.get_pixel(1, 1), Some(Color::BLACK));
            canvas.remove_sky();
            canvas.draw_mesh(&cube, &Transform::default(), &camera);
            canvas.flush_3d();
            assert_eq!(canvas.get_pixel(1, 1), Some(Color::BLACK));
        }
    }

    #[test]
    fn blending_between_grid_points_matches_working_out_every_pixel() {
        let sky = compass();
        let mut camera = Camera3D::new(Vec3::ZERO);
        camera.yaw = 2.9; // the panorama's two ends meet on screen
        camera.pitch = 0.3;
        // Big enough to be painted on several threads.
        let mut canvas = Canvas::new(600, 600);
        canvas.draw_sky(&sky, &camera);
        let focal = 300.0 / (camera.usable_fov() / 2.0).tan();
        let mut wrong = 0;
        for y in 0..600 {
            for x in 0..600 {
                let p = vec3(
                    (x as f32 + 0.5 - 300.0) / focal,
                    -(y as f32 + 0.5 - 300.0) / focal,
                    1.0,
                );
                let d = p.rotate_x(camera.pitch).rotate_y(camera.yaw);
                let (u, v) = panorama_point(d.x, d.y, d.z);
                let px = ((u.rem_euclid(1.0) * 8.0) as usize).min(7);
                let py = ((v * 4.0) as usize).min(3);
                let exact = sky.get_pixel(px, py).unwrap();
                if canvas.get_pixel(x, y) != Some(exact) {
                    wrong += 1;
                }
            }
        }
        // Only a few pixels right on a colour's edge can differ.
        assert!(wrong < 600 * 600 / 100, "{wrong} pixels differ");
        let distinct: std::collections::HashSet<u32> = canvas.pixels().iter().copied().collect();
        assert!(distinct.len() >= 4, "several of the compass's colours show");
    }
}

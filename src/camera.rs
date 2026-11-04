//! A 2D camera: look at any part of a big world, zoom in and out, and
//! shake the screen.
//!
//! The game *world* can be any size. The camera says which part of it is
//! on the screen: its [`position`](Camera2D::position) is the world point
//! at the center of the screen. Draw the world between
//! [`Canvas::push_camera`](crate::canvas::Canvas::push_camera) and
//! [`Canvas::pop_camera`](crate::canvas::Canvas::pop_camera); draw the score and
//! other screen things after the pop.
//!
//! ```
//! use duckforge::prelude::*;
//!
//! # let mut canvas = Canvas::new(320, 240);
//! let mut camera = Camera2D::new(320.0, 240.0);
//! camera.position = vec2(1000.0, 500.0); // look at this world point
//!
//! canvas.push_camera(&camera);
//! canvas.fill_circle(vec2(1000.0, 500.0), 10.0, Color::YELLOW); // in the world: drawn mid-screen
//! canvas.pop_camera();
//! canvas.draw_text("SCORE 0", vec2(4.0, 4.0), 1, Color::WHITE); // on the screen
//! ```

use crate::engine::Context;
use crate::math::{Rect, Vec2, vec2};

/// Which part of the world is on the screen. See the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Camera2D {
    /// The world point shown at the center of the screen.
    pub position: Vec2,
    /// How big things look: 2.0 shows everything twice as big (and half
    /// as much of the world), 0.5 half as big. 1.0 by default.
    pub zoom: f32,
    /// The size of the screen (the canvas) in pixels.
    pub screen_size: Vec2,
    /// How far the screen is pushed this frame by shaking, in screen pixels.
    shake_offset: Vec2,
    /// How hard the current shake is, in screen pixels.
    shake_amount: f32,
    /// How long the current shake lasts in total, and how much is left.
    shake_duration: f32,
    shake_left: f32,
}

impl Camera2D {
    /// A camera for a screen `screen_width` x `screen_height` pixels (use
    /// the same size as in your [`Config`](crate::engine::Config)).
    ///
    /// It starts looking at the middle of the screen with zoom 1, so world
    /// positions and screen positions are the same until you move it.
    pub fn new(screen_width: f32, screen_height: f32) -> Camera2D {
        let screen_size = vec2(screen_width, screen_height);
        Camera2D {
            position: screen_size * 0.5,
            zoom: 1.0,
            screen_size,
            shake_offset: Vec2::ZERO,
            shake_amount: 0.0,
            shake_duration: 0.0,
            shake_left: 0.0,
        }
    }

    /// Moves the camera towards `target`, smoothly.
    ///
    /// `smoothing` is roughly how many seconds the camera lags behind:
    /// `0.0` sticks to the target exactly, `0.1` to `0.3` feels smooth,
    /// `1.0` is lazy. Call it once per update with `ctx.dt()`:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut camera = Camera2D::new(320.0, 240.0);
    /// # let player = vec2(500.0, 300.0);
    /// # let dt = 1.0 / 60.0;
    /// camera.follow(player, 0.2, dt);
    /// ```
    pub fn follow(&mut self, target: Vec2, smoothing: f32, dt: f32) {
        if smoothing.is_nan() || smoothing <= 0.0 {
            self.position = target;
            return;
        }
        // Close the same fraction of the gap every second, whatever dt is:
        // after `smoothing` seconds about 63% of the gap is gone.
        let t = 1.0 - (-dt.max(0.0) / smoothing).exp();
        self.position += (target - self.position) * t;
    }

    /// Starts shaking the screen: up to `amount` pixels in every direction,
    /// fading out over `seconds`. A new shake replaces a weaker one.
    ///
    /// The shaking itself happens in [`Camera2D::update`], which must be
    /// called every update.
    pub fn shake(&mut self, amount: f32, seconds: f32) {
        if amount.is_nan() || seconds.is_nan() || amount <= 0.0 || seconds <= 0.0 {
            return;
        }
        // Keep whichever shake is stronger right now.
        if amount >= self.current_shake() {
            self.shake_amount = amount;
            self.shake_duration = seconds;
            self.shake_left = seconds;
        }
    }

    /// Moves the shake on by one update. Call it once per update, after
    /// any `shake`.
    ///
    /// The random wobble comes from `ctx.rng`, so with the same seed and
    /// inputs a replay shakes exactly the same way.
    pub fn update(&mut self, ctx: &mut Context) {
        if self.shake_left <= 0.0 {
            self.shake_offset = Vec2::ZERO;
            return;
        }
        let strength = self.current_shake();
        self.shake_offset = vec2(
            ctx.rng.range(-strength, strength).round(),
            ctx.rng.range(-strength, strength).round(),
        );
        self.shake_left = (self.shake_left - ctx.dt()).max(0.0);
    }

    /// How hard the screen shakes right now, in pixels (0 when still).
    fn current_shake(&self) -> f32 {
        if self.shake_left <= 0.0 || self.shake_duration <= 0.0 {
            0.0
        } else {
            self.shake_amount * (self.shake_left / self.shake_duration)
        }
    }

    /// True while the screen is shaking.
    pub fn is_shaking(&self) -> bool {
        self.shake_left > 0.0
    }

    /// The zoom, or 1.0 if it's zero, negative or not a number (which
    /// would make the world vanish or turn inside out).
    fn safe_zoom(&self) -> f32 {
        if self.zoom.is_finite() && self.zoom > 0.0 {
            self.zoom
        } else {
            1.0
        }
    }

    /// Where the world point `world` appears on the screen, in pixels.
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        (world - self.position) * self.safe_zoom() + self.screen_size * 0.5 + self.shake_offset
    }

    /// Which world point is under the screen point `screen`. Use it for the
    /// mouse: `camera.screen_to_world(ctx.input.mouse_position())`.
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        (screen - self.screen_size * 0.5 - self.shake_offset) * (1.0 / self.safe_zoom())
            + self.position
    }

    /// The part of the world that's on the screen, as a rectangle in world
    /// coordinates. Handy for skipping things that are off-screen.
    pub fn visible_area(&self) -> Rect {
        let size = self.screen_size * (1.0 / self.safe_zoom());
        Rect::from_center(self.position, size.x, size.y)
    }

    /// The camera as a [`View`]: the scale and offset that turn world
    /// positions into screen positions.
    pub(crate) fn view(&self) -> View {
        let zoom = self.safe_zoom();
        View {
            scale: zoom,
            offset: self.screen_size * 0.5 + self.shake_offset - self.position * zoom,
        }
    }
}

/// How positions given to the canvas become pixel positions:
/// `screen = position * scale + offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    pub(crate) scale: f32,
    pub(crate) offset: Vec2,
}

impl View {
    /// No change: positions are pixels.
    pub(crate) const SCREEN: View = View {
        scale: 1.0,
        offset: Vec2::ZERO,
    };

    pub(crate) fn apply(&self, p: Vec2) -> Vec2 {
        p * self.scale + self.offset
    }

    pub(crate) fn apply_rect(&self, r: Rect) -> Rect {
        let corner = self.apply(vec2(r.x, r.y));
        Rect::new(corner.x, corner.y, r.w * self.scale, r.h * self.scale)
    }

    /// The inverse: which position is drawn at screen point `screen`.
    pub(crate) fn unapply(&self, screen: Vec2) -> Vec2 {
        (screen - self.offset) * (1.0 / self.scale)
    }

    /// First `inner`, then this view: for a camera pushed inside another
    /// camera or offset.
    pub(crate) fn then(&self, inner: View) -> View {
        View {
            scale: self.scale * inner.scale,
            offset: self.apply(inner.offset),
        }
    }

    pub(crate) fn is_screen(&self) -> bool {
        *self == View::SCREEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;
    use crate::color::Color;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn a_new_camera_changes_nothing() {
        let camera = Camera2D::new(320.0, 240.0);
        assert_eq!(camera.view(), View::SCREEN);
        assert_eq!(camera.world_to_screen(vec2(12.0, 34.0)), vec2(12.0, 34.0));
    }

    #[test]
    fn world_and_screen_convert_both_ways() {
        let mut camera = Camera2D::new(320.0, 240.0);
        camera.position = vec2(1000.0, 500.0);
        camera.zoom = 2.0;
        assert_eq!(
            camera.world_to_screen(vec2(1000.0, 500.0)),
            vec2(160.0, 120.0)
        );
        assert_eq!(
            camera.world_to_screen(vec2(1010.0, 495.0)),
            vec2(180.0, 110.0)
        );
        for p in [vec2(0.0, 0.0), vec2(160.0, 120.0), vec2(319.0, 7.5)] {
            assert!(close(camera.world_to_screen(camera.screen_to_world(p)), p));
        }
        assert_eq!(camera.visible_area(), Rect::new(920.0, 440.0, 160.0, 120.0));
        // A broken zoom acts like zoom 1 instead of breaking the picture.
        camera.zoom = 0.0;
        assert_eq!(
            camera.world_to_screen(vec2(1010.0, 500.0)),
            vec2(170.0, 120.0)
        );
    }

    #[test]
    fn follow_closes_the_gap_smoothly_and_never_overshoots() {
        let mut camera = Camera2D::new(100.0, 100.0);
        camera.position = Vec2::ZERO;
        let target = vec2(100.0, 0.0);
        let mut last = 0.0;
        for _ in 0..60 {
            camera.follow(target, 0.25, 1.0 / 60.0);
            assert!(camera.position.x > last && camera.position.x < 100.0);
            last = camera.position.x;
        }
        // After 1 second (4 x smoothing) about 98% of the gap is closed.
        assert!(
            (camera.position.x - 98.17).abs() < 0.1,
            "{:?}",
            camera.position
        );
        // Frame rate doesn't matter: one big step equals many small ones.
        let mut once = Camera2D::new(100.0, 100.0);
        once.position = Vec2::ZERO;
        once.follow(target, 0.25, 1.0);
        assert!((once.position.x - camera.position.x).abs() < 0.01);
        // Smoothing 0 snaps.
        camera.follow(vec2(-5.0, 7.0), 0.0, 1.0 / 60.0);
        assert_eq!(camera.position, vec2(-5.0, 7.0));
    }

    #[test]
    fn shaking_is_random_but_repeatable_and_fades() {
        let run = |seed| {
            let mut ctx = Context::new(320, 240, seed);
            let mut camera = Camera2D::new(320.0, 240.0);
            camera.shake(6.0, 0.5);
            let mut offsets = Vec::new();
            for _ in 0..40 {
                ctx.begin_frame(1.0 / 60.0, []);
                camera.update(&mut ctx);
                offsets.push(camera.shake_offset);
            }
            offsets
        };
        let offsets = run(7);
        assert_eq!(offsets, run(7), "same seed, same shake");
        assert_ne!(offsets, run(8), "another seed shakes differently");
        assert!(offsets.iter().all(|o| o.x.abs() <= 6.0 && o.y.abs() <= 6.0));
        assert!(
            offsets[..5].iter().any(|o| *o != Vec2::ZERO),
            "it shakes at first"
        );
        assert!(
            offsets[30..].iter().all(|o| *o == Vec2::ZERO),
            "and stops after 0.5 s"
        );
    }

    #[test]
    fn the_camera_moves_what_is_drawn() {
        let mut canvas = Canvas::new(20, 10);
        let mut camera = Camera2D::new(20.0, 10.0);
        camera.position = vec2(100.0, 100.0);
        camera.zoom = 2.0;
        canvas.push_camera(&camera);
        // A 2 x 2 world square at the camera's position: 4 x 4 pixels, mid-screen.
        canvas.fill_rect(Rect::new(100.0, 100.0, 2.0, 2.0), Color::WHITE);
        canvas.pop_camera();
        canvas.fill_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::RED); // screen space again
        assert_eq!(canvas.get_pixel(10, 5), Some(Color::WHITE));
        assert_eq!(canvas.get_pixel(13, 8), Some(Color::WHITE));
        assert_eq!(canvas.get_pixel(14, 8), Some(Color::BLACK));
        assert_eq!(canvas.get_pixel(9, 5), Some(Color::BLACK));
        assert_eq!(canvas.get_pixel(0, 0), Some(Color::RED));
    }

    #[test]
    fn offsets_and_cameras_stack() {
        let mut canvas = Canvas::new(10, 10);
        canvas.push_offset(vec2(3.0, 0.0));
        canvas.push_offset(vec2(0.0, 2.0));
        canvas.set_pixel(1, 1, Color::RED); // lands at (4, 3)
        canvas.pop_offset();
        canvas.set_pixel(1, 1, Color::GREEN); // lands at (4, 1)
        canvas.pop_offset();
        canvas.pop_offset(); // one pop too many: harmless
        canvas.set_pixel(1, 1, Color::BLUE); // back to the screen
        assert_eq!(canvas.get_pixel(4, 3), Some(Color::RED));
        assert_eq!(canvas.get_pixel(4, 1), Some(Color::GREEN));
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::BLUE));

        // An offset inside a zoomed camera is in world units, so it's zoomed too.
        let mut camera = Camera2D::new(10.0, 10.0);
        camera.zoom = 2.0;
        camera.position = vec2(0.0, 0.0);
        canvas.push_camera(&camera);
        canvas.push_offset(vec2(1.0, 1.0));
        assert_eq!(canvas.world_to_screen(Vec2::ZERO), vec2(7.0, 7.0));
        canvas.pop_offset();
        canvas.pop_camera();
        assert_eq!(canvas.world_to_screen(vec2(1.0, 1.0)), vec2(1.0, 1.0));
    }

    #[test]
    fn every_shape_follows_the_camera() {
        use crate::sprite::{Blit, Sprite};
        // Draw a scene in screen space, then the same scene moved by +50
        // and seen through a camera that moves it back: identical pictures.
        let scene = |canvas: &mut Canvas, shift: Vec2| {
            let at = |x: f32, y: f32| vec2(x, y) + shift;
            canvas.fill_rect(
                Rect::new(2.0 + shift.x, 3.0 + shift.y, 5.0, 4.0),
                Color::RED,
            );
            canvas.draw_rect(
                Rect::new(1.0 + shift.x, 1.0 + shift.y, 30.0, 20.0),
                Color::GREEN,
            );
            canvas.fill_circle(at(20.0, 12.0), 5.0, Color::BLUE);
            canvas.draw_line(at(0.0, 25.0), at(39.0, 29.0), Color::YELLOW);
            canvas.fill_triangle(at(30.0, 2.0), at(38.0, 2.0), at(34.0, 9.0), Color::PURPLE);
            canvas.draw_text("A1", at(8.0, 20.0), 1, Color::WHITE);
            canvas.set_pixel(39 + shift.x as i32, 1 + shift.y as i32, Color::ORANGE);
            let s = Sprite::from_ascii(&["ab", "b."], &[('a', Color::RED), ('b', Color::WHITE)]);
            canvas.blit_sprite(&s, at(24.0, 22.0), Blit::default());
        };
        let mut plain = Canvas::new(40, 30);
        scene(&mut plain, Vec2::ZERO);
        let mut moved = Canvas::new(40, 30);
        let mut camera = Camera2D::new(40.0, 30.0);
        camera.position += vec2(50.0, 50.0);
        moved.push_camera(&camera);
        scene(&mut moved, vec2(50.0, 50.0));
        moved.pop_camera();
        assert_eq!(plain.pixels(), moved.pixels());
    }

    #[test]
    fn centered_text_stays_centered_on_the_screen() {
        let mut plain = Canvas::new(40, 20);
        plain.draw_text_centered("HI", 5.0, 2, Color::WHITE);
        let mut through = Canvas::new(40, 20);
        let mut camera = Camera2D::new(40.0, 20.0);
        camera.position += vec2(300.0, 0.0); // looking far to the right
        through.push_camera(&camera);
        through.draw_text_centered("HI", 5.0, 2, Color::WHITE);
        assert_eq!(plain.pixels(), through.pixels());

        let mut zoomed = Canvas::new(40, 20);
        camera.zoom = 2.0;
        zoomed.push_camera(&camera);
        zoomed.draw_text_centered("HI", 5.0, 1, Color::WHITE); // scale 1 x zoom 2 = 2
        zoomed.pop_camera();
        let lit: Vec<i32> = (0..40)
            .filter(|&x| (0..20).any(|y| zoomed.get_pixel(x, y) == Some(Color::WHITE)))
            .collect();
        let lit_plain: Vec<i32> = (0..40)
            .filter(|&x| (0..20).any(|y| plain.get_pixel(x, y) == Some(Color::WHITE)))
            .collect();
        assert_eq!(lit, lit_plain, "same columns: centered the same way");
    }

    #[test]
    fn zoom_scales_sizes_and_sprites() {
        use crate::sprite::{Blit, Sprite};
        let mut canvas = Canvas::new(8, 8);
        let mut camera = Camera2D::new(8.0, 8.0);
        camera.position = vec2(1.0, 1.0); // world (0, 0) is at screen (2, 2) with zoom 2
        camera.zoom = 2.0;
        canvas.push_camera(&camera);
        let dot = Sprite::from_ascii(&["a"], &[('a', Color::RED)]);
        canvas.blit_sprite(
            &dot,
            Vec2::ZERO,
            Blit {
                scale: 1.5,
                ..Default::default()
            },
        );
        canvas.pop_camera();
        // 1 pixel x scale 1.5 x zoom 2 = 3 x 3 pixels from (2, 2).
        for (x, y) in [(2, 2), (4, 4), (2, 4)] {
            assert_eq!(canvas.get_pixel(x, y), Some(Color::RED), "({x}, {y})");
        }
        assert_eq!(canvas.get_pixel(5, 5), Some(Color::BLACK));
        assert_eq!(canvas.get_pixel(1, 2), Some(Color::BLACK));
    }
}

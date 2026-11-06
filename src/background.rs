//! Scrolling backgrounds: a picture that repeats with no seams, moving
//! slower than the world for depth (*parallax*).
//!
//! ```
//! # use duckforge::prelude::*;
//! # let sky_png = Sprite::from_pixels(64, 32, vec![Color::BLUE; 64 * 32]);
//! # let hills_png = sky_png.clone();
//! # let camera = Camera2D::new(320.0, 240.0);
//! # let mut canvas = Canvas::new(320, 240);
//! // let sky_png = Sprite::load_png("assets/sky.png")?;
//! let sky = Background::new(sky_png).parallax(0.2);
//! let hills = Background::new(hills_png).parallax(0.5).offset(vec2(0.0, 120.0));
//! // in draw, before push_camera:
//! canvas.draw_background(&sky, &camera);
//! canvas.draw_background(&hills, &camera);
//! ```
//!
//! `parallax(0.5)` moves the picture half as far as the camera, in both
//! directions; `parallax_xy(0.5, 0.1)` gives each its own amount (a tower
//! climbed upwards, say). 0 keeps it fixed to the screen, 1 moves it with
//! the world. Copies sit at whole pixels, edge to edge, so there are no
//! gaps or seams, wherever the camera is.

use crate::camera::{Camera2D, View};
use crate::canvas::Canvas;
use crate::math::{Vec2, vec2};
use crate::sprite::{Blit, Sprite};

/// A repeating background picture. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct Background {
    image: Sprite,
    parallax: Vec2,
    offset: Vec2,
    repeat_x: bool,
    repeat_y: bool,
}

impl Background {
    /// A background of `image`, repeating sideways, fixed to the screen
    /// until you give it a [`parallax`](Background::parallax).
    pub fn new(image: Sprite) -> Background {
        Background {
            image,
            parallax: Vec2::ZERO,
            offset: Vec2::ZERO,
            repeat_x: true,
            repeat_y: false,
        }
    }

    /// How far it moves when the camera moves, in both directions: 0.0
    /// stays on the screen, 0.5 moves half as far as the world (far away),
    /// 1.0 moves with the world.
    pub fn parallax(self, amount: f32) -> Background {
        self.parallax_xy(amount, amount)
    }

    /// The same, with its own amount sideways (`x`) and up and down (`y`).
    pub fn parallax_xy(mut self, x: f32, y: f32) -> Background {
        self.parallax = vec2(finite(x), finite(y));
        self
    }

    /// Where the picture's top-left corner is when the camera is at the
    /// start (looking at the top-left of the world): `vec2(0.0, 120.0)`
    /// puts hills on the lower half of a 240-pixel screen.
    pub fn offset(mut self, offset: Vec2) -> Background {
        self.offset = vec2(finite(offset.x), finite(offset.y));
        self
    }

    /// Whether it repeats sideways (it does unless told not to).
    pub fn repeat_x(mut self, repeat: bool) -> Background {
        self.repeat_x = repeat;
        self
    }

    /// Whether it also repeats up and down (it doesn't unless told to).
    pub fn repeat_y(mut self, repeat: bool) -> Background {
        self.repeat_y = repeat;
        self
    }

    /// The top-left corner of one copy on the screen, in whole pixels, for
    /// a camera.
    fn origin(&self, camera: &Camera2D) -> (i64, i64) {
        // How far the camera has moved from the start, where the world's
        // top-left is at the screen's top-left.
        let scrolled = camera.position - camera.screen_size * 0.5;
        let at = self.offset
            + vec2(-scrolled.x * self.parallax.x, -scrolled.y * self.parallax.y)
            + camera.shake_offset();
        let whole = |v: f32| (v as f64).round().clamp(-1e12, 1e12) as i64;
        (whole(at.x), whole(at.y))
    }
}

/// `v`, or 0 if it isn't a number.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Where the copies go along one side of the screen: from the first one
/// that reaches onto it, every `size` pixels, to the far edge.
fn copies(origin: i64, size: i64, screen: i64, repeat: bool) -> Vec<i64> {
    if !repeat {
        return vec![origin];
    }
    let mut at = origin.rem_euclid(size);
    if at > 0 {
        at -= size;
    }
    let mut places = Vec::new();
    while at < screen {
        places.push(at);
        at += size;
    }
    places
}

impl Canvas {
    /// Draws a [`Background`] for `camera`: in screen pixels, wherever
    /// it's called (usually first, right after `clear`, before
    /// `push_camera`). Backgrounds aren't zoomed with the camera.
    pub fn draw_background(&mut self, background: &Background, camera: &Camera2D) {
        let (w, h) = (background.image.width(), background.image.height());
        if w == 0 || h == 0 {
            return;
        }
        let (x, y) = background.origin(camera);
        let view = self.view();
        self.set_view(View::SCREEN);
        let across = copies(x, w as i64, self.width() as i64, background.repeat_x);
        let down = copies(y, h as i64, self.height() as i64, background.repeat_y);
        for &top in &down {
            for &left in &across {
                self.blit_sprite(
                    &background.image,
                    vec2(left as f32, top as f32),
                    Blit::default(),
                );
            }
        }
        self.set_view(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    /// A 7 x 5 picture where every pixel is different, so any gap, seam or
    /// wrong position shows.
    fn picture() -> Sprite {
        let pixels = (0..35)
            .map(|i| Color::rgb(i as u8 * 7, 255 - i as u8 * 7, 100))
            .collect();
        Sprite::from_pixels(7, 5, pixels)
    }

    fn camera_at(x: f32, y: f32) -> Camera2D {
        let mut camera = Camera2D::new(40.0, 30.0);
        camera.position = vec2(x, y);
        camera
    }

    /// Checks every pixel of `canvas` against the picture repeated from
    /// (`ox`, `oy`), in the rows it covers.
    fn assert_repeats(canvas: &Canvas, ox: i32, oy: i32, rows: std::ops::Range<i32>) {
        let picture = picture();
        for y in rows {
            for x in 0..40 {
                let want = picture.get_pixel(
                    (x - ox).rem_euclid(7) as usize,
                    (y - oy).rem_euclid(5) as usize,
                );
                assert_eq!(canvas.get_pixel(x, y), want, "pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn it_repeats_sideways_with_no_seams_and_moves_by_its_parallax() {
        let sky = Background::new(picture())
            .parallax(0.5)
            .offset(vec2(0.0, 10.0));
        for camera_x in [20.0, 33.0, 160.4, -75.0] {
            let mut canvas = Canvas::new(40, 30);
            canvas.draw_background(&sky, &camera_at(camera_x, 15.0));
            // The camera has moved camera_x - 20 from the start; the sky,
            // half as far the other way.
            let ox = (-(camera_x - 20.0) * 0.5_f32).round() as i32;
            assert_repeats(&canvas, ox, 10, 10..15);
            assert_eq!(
                canvas.get_pixel(3, 9),
                Some(Color::BLACK),
                "not repeated up and down"
            );
            assert_eq!(canvas.get_pixel(3, 15), Some(Color::BLACK));
        }
    }

    #[test]
    fn climbing_a_tower_moves_it_down_by_its_vertical_parallax() {
        // The camera goes up 40 pixels; the far wall (0.25 up and down,
        // fixed sideways) goes down 10, repeating to fill the screen.
        let wall = Background::new(picture())
            .parallax_xy(0.0, 0.25)
            .repeat_y(true);
        let mut start = Canvas::new(40, 30);
        start.draw_background(&wall, &camera_at(20.0, 15.0));
        assert_repeats(&start, 0, 0, 0..30);
        let mut higher = Canvas::new(40, 30);
        higher.draw_background(&wall, &camera_at(20.0, -25.0));
        assert_repeats(&higher, 0, 10, 0..30);
        // Sideways it stays put.
        let mut aside = Canvas::new(40, 30);
        aside.draw_background(&wall, &camera_at(500.0, 15.0));
        assert_repeats(&aside, 0, 0, 0..30);
    }

    #[test]
    fn it_stays_in_screen_pixels_inside_a_camera() {
        let fixed = Background::new(picture()).repeat_y(true);
        let camera = camera_at(300.0, 200.0);
        let mut canvas = Canvas::new(40, 30);
        canvas.push_camera(&camera);
        canvas.draw_background(&fixed, &camera);
        canvas.fill_rect(crate::math::Rect::new(300.0, 200.0, 1.0, 1.0), Color::WHITE);
        canvas.pop_camera();
        assert_repeats(&canvas, 0, 0, 0..15);
        assert_eq!(
            canvas.get_pixel(20, 15),
            Some(Color::WHITE),
            "the camera still works after"
        );
    }
}

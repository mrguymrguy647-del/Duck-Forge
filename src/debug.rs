//! The debug overlay: **F1** in a window, or `--debug` on the command line
//! (which also works with `--headless`, so the overlay shows up in
//! snapshots, sheets and GIFs).
//!
//! It shows, in screen pixels on top of the game:
//!
//! - the FPS and the frame number,
//! - the camera's position and zoom (the first camera the game pushed),
//! - where the mouse is, on the screen and in the world,
//! - outlines of every shape the game registered during the update with
//!   [`Context::debug_rect`] and [`Context::debug_circle`]: hitboxes,
//!   trigger zones, anything you want to see.

use crate::camera::Camera2D;
use crate::canvas::Canvas;
use crate::color::Color;
use crate::engine::Context;
use crate::math::{Rect, Vec2, vec2};

/// A shape the game asked to see outlined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DebugShape {
    Rect(Rect),
    Circle(Vec2, f32),
}

/// Outline colors: bright and unlike most game colors.
const OUTLINE: Color = Color::rgb(255, 0, 255);
const CIRCLE: Color = Color::rgb(0, 255, 255);

/// Draws the overlay onto `canvas`, which already holds the game's picture.
pub(crate) fn draw_overlay(
    canvas: &mut Canvas,
    ctx: &Context,
    shapes: &[DebugShape],
    camera: Option<&Camera2D>,
) {
    let to_screen = |p: Vec2| camera.map_or(p, |c| c.world_to_screen(p));
    let zoom = camera.map_or(1.0, |c| {
        c.world_to_screen(vec2(1.0, 0.0)).x - c.world_to_screen(Vec2::ZERO).x
    });

    for shape in shapes {
        match *shape {
            DebugShape::Rect(rect) => {
                let corner = to_screen(vec2(rect.x, rect.y));
                canvas.draw_rect(
                    Rect::new(corner.x, corner.y, rect.w * zoom, rect.h * zoom),
                    OUTLINE,
                );
            }
            DebugShape::Circle(center, radius) => {
                ring(canvas, to_screen(center), radius * zoom, CIRCLE);
            }
        }
    }

    let mouse = ctx.input.mouse_position();
    let mut lines = vec![format!("FPS {:.0}  FRAME {}", ctx.fps(), ctx.frame())];
    match camera {
        Some(c) => {
            lines.push(format!(
                "CAMERA {:.0}, {:.0}  ZOOM {:.2}",
                c.position.x, c.position.y, c.zoom
            ));
            let world = c.screen_to_world(mouse);
            lines.push(format!(
                "MOUSE {:.0}, {:.0}  WORLD {:.0}, {:.0}",
                mouse.x, mouse.y, world.x, world.y
            ));
        }
        None => {
            lines.push(String::from("CAMERA NONE"));
            lines.push(format!("MOUSE {:.0}, {:.0}", mouse.x, mouse.y));
        }
    }
    if !shapes.is_empty() {
        lines.push(format!("SHAPES {}", shapes.len()));
    }
    let text = lines.join("\n");
    let size = vec2(
        Canvas::text_width(&text, 1) + 4.0,
        Canvas::line_height(1) * (lines.len() - 1) as f32 + Canvas::text_height(1) + 4.0,
    );
    canvas.fill_rect(
        Rect::new(0.0, 0.0, size.x, size.y),
        Color::rgba(0, 0, 0, 170),
    );
    canvas.draw_text(&text, vec2(2.0, 2.0), 1, Color::YELLOW);
}

/// A circle's outline, drawn with short lines.
fn ring(canvas: &mut Canvas, center: Vec2, radius: f32, color: Color) {
    if !(radius.is_finite() && radius > 0.0) {
        return;
    }
    let segments = ((radius * 0.8) as usize).clamp(12, 64);
    let point = |i: usize| {
        let angle = i as f32 / segments as f32 * std::f32::consts::TAU;
        center + vec2(angle.cos(), angle.sin()) * radius
    };
    for i in 0..segments {
        canvas.draw_line(point(i), point(i + 1), color);
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::{HeadlessOptions, Simulation, run_headless};

    /// A game that marks a 10 x 10 box at world (100, 100) for the overlay,
    /// and looks at it through a camera zoomed 2x.
    struct Marked {
        camera: Camera2D,
    }

    impl Game for Marked {
        fn update(&mut self, ctx: &mut Context) {
            ctx.debug_rect(Rect::new(100.0, 100.0, 10.0, 10.0));
            ctx.debug_circle(vec2(100.0, 100.0), 30.0);
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            canvas.push_camera(&self.camera);
            canvas.fill_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::WHITE);
            canvas.pop_camera();
        }
    }

    fn game() -> Marked {
        let mut camera = Camera2D::new(320.0, 240.0);
        camera.position = vec2(100.0, 100.0); // the box's corner is mid-screen
        camera.zoom = 2.0;
        Marked { camera }
    }

    const MAGENTA: Color = Color::rgb(255, 0, 255);

    #[test]
    fn the_overlay_is_off_until_asked_for() {
        let mut sim = Simulation::new(Config::default(), game());
        sim.step();
        let picture = sim.picture();
        assert_eq!(picture.get_pixel(160, 120), Some(Color::BLACK));
        assert_eq!(picture.get_pixel(1, 1), Some(Color::BLACK), "no panel");
    }

    #[test]
    fn outlines_follow_the_camera_and_the_panel_is_on_the_screen() {
        let mut sim = Simulation::new(Config::default(), game());
        sim.set_debug_overlay(true);
        sim.step();
        let picture = sim.picture();
        // World (100, 100) is screen (160, 120); 10 world pixels are 20 screen pixels.
        assert_eq!(
            picture.get_pixel(160, 120),
            Some(MAGENTA),
            "top-left corner"
        );
        assert_eq!(
            picture.get_pixel(179, 139),
            Some(MAGENTA),
            "bottom-right corner"
        );
        assert_eq!(
            picture.get_pixel(170, 130),
            Some(Color::BLACK),
            "an outline, not filled"
        );
        // The circle's radius is 30 world pixels: 60 on screen.
        assert_eq!(picture.get_pixel(220, 120), Some(Color::rgb(0, 255, 255)));
        // The panel's see-through black darkens the top-left corner, and
        // has yellow text in it.
        let panel: Vec<Color> = (0..120).filter_map(|x| picture.get_pixel(x, 4)).collect();
        assert!(panel.contains(&Color::YELLOW), "text in the panel");
    }

    #[test]
    fn headless_debug_puts_the_overlay_in_the_saved_pictures() {
        let folder = std::env::temp_dir()
            .join(format!("duckforge-tests-{}", std::process::id()))
            .join("debug-overlay");
        let options = HeadlessOptions {
            frames: 3,
            out: Some(folder.clone()),
            debug: true,
            ..HeadlessOptions::default()
        };
        let mut sim = run_headless(Config::default(), game(), &options).unwrap();
        assert_eq!(sim.picture().get_pixel(160, 120), Some(MAGENTA));
        let saved = Canvas::load_png_for_tests(&folder.join("last.png"));
        assert_eq!(saved.get_pixel(160, 120), Some(MAGENTA));
    }
}

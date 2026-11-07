//! The debug overlay: **F1** in a window, or `--debug` on the command line
//! (which also works with `--headless`, so the overlay shows up in
//! snapshots, sheets and GIFs).
//!
//! It shows, in screen pixels on top of the game, in a small see-through
//! box in one corner (bottom-right unless
//! [`Config::debug_corner`](crate::engine::Config::debug_corner) or
//! `--debug-corner` says otherwise):
//!
//! - the frame number and the FPS,
//! - the camera's position and zoom (the first camera the game pushed),
//! - where the mouse is in the world,
//! - outlines of every shape the game registered during the update with
//!   [`Context::debug_rect`] and [`Context::debug_circle`]: hitboxes,
//!   trigger zones, anything you want to see,
//! - in red, every rule from [`Context::check`] that failed, and when,
//! - green outlines, with their names, of everything the game labelled with
//!   [`Canvas::label`](crate::canvas::Canvas::label).

use crate::camera::Camera2D;
use crate::canvas::Canvas;
use crate::color::Color;
use crate::engine::{Context, Corner};
use crate::math::{Rect, Vec2, vec2};
use crate::scene::Label;

/// A shape the game asked to see outlined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DebugShape {
    Rect(Rect),
    Circle(Vec2, f32),
}

/// Outline colors: bright and unlike most game colors.
const OUTLINE: Color = Color::rgb(255, 0, 255);
const CIRCLE: Color = Color::rgb(0, 255, 255);
const LABEL: Color = Color::rgb(0, 255, 0);

/// Draws the overlay onto `canvas`, which already holds the game's picture,
/// and returns where its info box is (so `--grid` can keep its numbers
/// out of it).
/// A label's name: one line of the smallest text, plus a pixel all round.
const NAME_HEIGHT: f32 = 7.0;

/// Where a label's name goes: just above its box, or just below it when
/// there's no room above.
fn name_position(boxed: Rect) -> (f32, f32) {
    let above = boxed.y - NAME_HEIGHT;
    if above >= 0.0 {
        (boxed.x + 1.0, above + 1.0)
    } else {
        (boxed.x + 1.0, boxed.bottom() + 2.0)
    }
}

pub(crate) fn draw_overlay(
    canvas: &mut Canvas,
    ctx: &Context,
    shapes: &[DebugShape],
    labels: &[Label],
    camera: Option<&Camera2D>,
    corner: Corner,
) -> Rect {
    // Labels: a green outline where they are on screen, with the name
    // just outside it (above, or below at the top of the screen) on a dark
    // strip, so it never covers what's labelled and reads on any background.
    let on_screen: Vec<&Label> = labels.iter().filter(|l| l.on_screen).collect();
    for label in &on_screen {
        canvas.draw_rect(label.screen, LABEL);
    }
    for label in &on_screen {
        let (x, y) = name_position(label.screen);
        let width = Canvas::text_width(&label.name, 1);
        let strip = Rect::new(x - 1.0, y - 1.0, width + 2.0, NAME_HEIGHT);
        canvas.fill_rect(strip, Color::BLACK.with_alpha(170));
        canvas.draw_text(&label.name, vec2(x, y), 1, LABEL);
    }
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

    let info = info_text(ctx, camera);
    // Failed checks get a red line each, under the info.
    let failed: Vec<String> = ctx
        .failed_checks()
        .iter()
        .take(4)
        .map(|f| format!("CHECK FAILED F{}: {}", f.frame, f.name))
        .collect();
    let text = if failed.is_empty() {
        info.clone()
    } else {
        format!("{info}\n{}", failed.join("\n"))
    };
    let lines = text.lines().count();
    let size = vec2(
        Canvas::text_width(&text, 1) + 4.0,
        Canvas::line_height(1) * (lines - 1) as f32 + Canvas::text_height(1) + 4.0,
    );
    let (width, height) = (canvas.width() as f32, canvas.height() as f32);
    let x = match corner {
        Corner::TopLeft | Corner::BottomLeft => 0.0,
        Corner::TopRight | Corner::BottomRight => width - size.x,
    };
    let y = match corner {
        Corner::TopLeft | Corner::TopRight => 0.0,
        Corner::BottomLeft | Corner::BottomRight => height - size.y,
    };
    // See-through, so the game shows through the box.
    canvas.fill_rect(Rect::new(x, y, size.x, size.y), Color::rgba(0, 0, 0, 120));
    canvas.draw_text(&info, vec2(x + 2.0, y + 2.0), 1, Color::YELLOW);
    if !failed.is_empty() {
        let below = y + 2.0 + Canvas::line_height(1) * info.lines().count() as f32;
        canvas.draw_text(
            &failed.join("\n"),
            vec2(x + 2.0, below),
            1,
            Color::rgb(255, 80, 80),
        );
    }
    Rect::new(x, y, size.x, size.y)
}

/// The overlay's two short lines, like
/// `"FRAME 600 FPS 60"` and `"CAM 685,600 X2 MOUSE 565,530"`. The mouse
/// is given in world coordinates when there's a camera.
fn info_text(ctx: &Context, camera: Option<&Camera2D>) -> String {
    let first = format!("FRAME {} FPS {:.0}", ctx.frame(), ctx.fps());
    // In world coordinates under a camera; "-" when there's no mouse.
    let mouse = ctx
        .input
        .mouse_position()
        .map_or(String::from("-"), |mouse| {
            let at = camera.map_or(mouse, |c| c.screen_to_world(mouse));
            format!("{:.0},{:.0}", at.x, at.y)
        });
    let second = match camera {
        Some(c) => format!(
            "CAM {:.0},{:.0} X{} MOUSE {mouse}",
            c.position.x,
            c.position.y,
            short_number(c.zoom),
        ),
        None => format!("MOUSE {mouse}"),
    };
    format!("{first}\n{second}")
}

/// A number with at most two decimals and no trailing zeros: 2, 1.5, 0.25.
fn short_number(value: f32) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
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
    use crate::canvas::Canvas;
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
        // The info box is in the bottom-right corner by default, and the
        // top-left corner (where games put their score) is untouched.
        assert!(yellow_in(picture, 220, 220, 100, 20), "text bottom-right");
        assert!(!yellow_in(picture, 0, 0, 160, 30), "nothing top-left");
    }

    /// Whether any pixel in the area is the overlay's yellow text.
    fn yellow_in(picture: &Canvas, x: i32, y: i32, w: i32, h: i32) -> bool {
        (y..y + h).any(|py| (x..x + w).any(|px| picture.get_pixel(px, py) == Some(Color::YELLOW)))
    }

    #[test]
    fn label_names_never_cover_what_they_label() {
        struct Small;
        impl Game for Small {
            fn update(&mut self, _: &mut Context) {}
            fn draw(&self, canvas: &mut Canvas) {
                canvas.clear(Color::BLACK);
                canvas.label("bee", Rect::new(100.0, 100.0, 12.0, 12.0));
                canvas.label("top", Rect::new(40.0, 1.0, 12.0, 12.0)); // no room above
            }
        }
        let mut sim = Simulation::new(Config::default(), Small);
        sim.set_debug_overlay(true);
        sim.step();
        let picture = sim.picture();
        let green_in = |x0: i32, y0: i32, w: i32, h: i32| {
            (y0..y0 + h)
                .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
                .filter(|&(x, y)| picture.get_pixel(x, y) == Some(super::LABEL))
                .count()
        };
        // Inside each box (within the outline): nothing.
        assert_eq!(green_in(101, 101, 10, 10), 0, "bee's box is clear");
        assert_eq!(green_in(41, 2, 10, 10), 0, "top's box is clear");
        // The names: above the bee, below the one at the top.
        assert!(green_in(100, 92, 16, 8) > 5, "bee's name above it");
        assert!(green_in(40, 14, 16, 8) > 5, "top's name below it");
    }

    #[test]
    fn the_info_box_goes_in_the_chosen_corner_and_stays_small() {
        let areas = [
            (Corner::TopLeft, (0, 0)),
            (Corner::TopRight, (200, 0)),
            (Corner::BottomLeft, (0, 220)),
            (Corner::BottomRight, (200, 220)),
        ];
        for (corner, (x, y)) in areas {
            let config = Config {
                debug_corner: corner,
                ..Config::default()
            };
            let mut sim = Simulation::new(config, game());
            sim.set_debug_overlay(true);
            sim.step();
            let picture = sim.picture();
            assert!(yellow_in(picture, x, y, 120, 20), "{corner:?}");
            // Two lines of text: everything else in that half of the screen is untouched.
            let other_y = if y == 0 { 20 } else { 0 };
            assert!(
                !yellow_in(picture, x, other_y, 120, 200),
                "{corner:?} is compact"
            );
        }
    }

    #[test]
    fn the_box_is_see_through() {
        struct White;
        impl Game for White {
            fn update(&mut self, _: &mut Context) {}
            fn draw(&self, canvas: &mut Canvas) {
                canvas.clear(Color::WHITE);
            }
        }
        let mut sim = Simulation::new(Config::default(), White);
        sim.set_debug_overlay(true);
        sim.step();
        let corner = sim.picture().get_pixel(319, 239).unwrap();
        assert!(
            corner != Color::WHITE && corner != Color::BLACK,
            "{corner:?}: dimmed, not hidden"
        );
    }

    #[test]
    fn two_short_lines() {
        let mut camera = Camera2D::new(320.0, 240.0);
        camera.position = vec2(685.0, 600.0);
        camera.zoom = 1.5;
        let mut ctx = Context::new(320, 240, 1);
        ctx.begin_frame(1.0 / 60.0, []);
        // No mouse yet.
        assert_eq!(super::info_text(&ctx, None), "FRAME 1 FPS 0\nMOUSE -");
        ctx.input
            .update_mouse(Some(vec2(0.0, 0.0)), vec2(0.0, 0.0), [], 0.0);
        let text = super::info_text(&ctx, Some(&camera));
        assert_eq!(text, "FRAME 1 FPS 0\nCAM 685,600 X1.5 MOUSE 578,520");
        assert_eq!(super::info_text(&ctx, None), "FRAME 1 FPS 0\nMOUSE 0,0");
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

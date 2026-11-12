//! A model made with Duck Mold, on a turntable: a robot, facing you and
//! slowly turning, in a top hat.
//!
//! Run it with:   cargo run --example turntable
//! Controls:      Space stops and starts the turntable, Left/Right turn it
//!                by hand, Escape quits.
//!
//! The robot is `assets/robot.glb`, made by Duck Mold from the recipe in
//! `art/robot.toml` (`art/README.md` says how). Loading it is one line:
//!
//! ```ignore
//! let robot = Mesh::load_glb("examples/turntable/assets/robot.glb")?;
//! ```
//!
//! and it's drawn like any mesh. A Duck Mold model faces forward, +z. The
//! camera here stands on the robot's +z side looking back at it, so the
//! robot faces it with no turn at all. (A camera made with `Camera3D::new`
//! looks along +z, and would see the robot's back.)
//!
//! The hat isn't part of the model. The model file keeps the robot's
//! *sockets*, the named points where its recipe joined its parts, and the
//! game asks for one, `head.top`, and puts the hat there, moved and turned
//! with the robot:
//!
//! ```ignore
//! let model = GlbModel::load("examples/turntable/assets/robot.glb")?;
//! let top = model.socket("head.top").unwrap();
//! let hat_at = robot_place.apply(top.position());
//! ```

use std::f32::consts::TAU;

use duckforge::LoadError;
use duckforge::glb::GlbModel;
use duckforge::prelude::*;

const ASSETS: &str = "examples/turntable/assets";

const SKY: Color = Color::from_hex(0x2A_32_44);
const FLOOR_LIGHT: Color = Color::from_hex(0x4A_52_60);
const FLOOR_DARK: Color = Color::from_hex(0x3E_45_52);
const TURNTABLE: Color = Color::from_hex(0x6C_74_80);
const HAT: Color = Color::from_hex(0x5B_3A_8C);
/// The hat's crown, and its brim, in metres: tall enough to cover the
/// robot's antenna, which also stands on `head.top`.
const CROWN: Vec3 = vec3(0.3, 0.34, 0.3);
const BRIM: Vec3 = vec3(0.58, 0.04, 0.58);
/// The turntable's thickness: the robot stands on top of it.
const BASE: f32 = 0.08;

/// One full turn every 20 seconds, on its own.
const TURN_SPEED: f32 = TAU / 20.0;
/// One full turn every 4 seconds, by hand.
const HAND_SPEED: f32 = TAU / 4.0;

struct Turntable {
    camera: Camera3D,
    robot: Mesh,
    /// Where the hat goes: the robot's `head.top` socket, in the robot's
    /// own space.
    head_top: Vec3,
    hat: Mesh,
    /// The robot's box, from its lowest corner to its highest.
    bounds: (Vec3, Vec3),
    base: Mesh,
    floor: Mesh,
    /// How far it has turned, in radians: 0 faces the camera.
    angle: f32,
    turning: bool,
}

impl Turntable {
    fn new() -> Result<Turntable, LoadError> {
        let model = GlbModel::load(format!("{ASSETS}/robot.glb"))?;
        let robot = model.to_mesh();
        // `duck-mold inspect art/robot.toml --sockets` lists the names.
        let head_top = model
            .socket("head.top")
            .expect("Duck Mold models keep their parts' sockets")
            .position();
        Ok(Turntable {
            // In front of the robot (its +z side), a little above its
            // middle, looking back at it.
            camera: Camera3D::looking_at(vec3(0.0, 1.35, 3.3), vec3(0.0, 0.85, 0.0)),
            bounds: robot.bounds(),
            robot,
            head_top,
            hat: Mesh::cylinder(HAT, 16),
            base: Mesh::cylinder(TURNTABLE, 32),
            floor: Mesh::checkerboard(12, 1.0, FLOOR_LIGHT, FLOOR_DARK),
            angle: 0.0,
            turning: true,
        })
    }

    /// Where the robot is: on the turntable, turned by `angle`.
    fn robot_place(&self) -> Transform {
        Transform::at(vec3(0.0, BASE, 0.0)).rotated(vec3(0.0, self.angle, 0.0))
    }

    /// The hat's crown and brim: standing on the robot's `head.top`,
    /// moved and turned with the robot. That socket faces straight up, so
    /// the hat needs no turn of its own.
    fn hat_places(&self) -> [Transform; 2] {
        let place = self.robot_place();
        let on_head = place.apply(self.head_top);
        // The cylinder is centred on its middle: lift each half its height.
        [CROWN, BRIM].map(|size| {
            Transform::at(on_head + vec3(0.0, size.y / 2.0, 0.0))
                .rotated(place.rotation)
                .scaled(size)
        })
    }

    /// The rectangle the robot covers on screen, for its label: the
    /// smallest one round the eight corners of its box.
    fn on_screen(&self, canvas: &Canvas) -> Option<Rect> {
        let (low, high) = self.bounds;
        let place = self.robot_place();
        let mut corners = Vec::new();
        for x in [low.x, high.x] {
            for y in [low.y, high.y] {
                for z in [low.z, high.z] {
                    let (point, _) = self.camera.project(place.apply(vec3(x, y, z)), canvas)?;
                    corners.push(point);
                }
            }
        }
        let left = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let right = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let top = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let bottom = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        Some(Rect::new(left, top, right - left, bottom - top))
    }
}

impl Game for Turntable {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Space) {
            self.turning = !self.turning;
        }
        // Right pushes the robot's front to your right. (A positive turn
        // swings its front, +z, towards +x: from in front, that's your left.)
        let by_hand = ctx.input.axis(Key::Right, Key::Left) * HAND_SPEED;
        let on_its_own = if self.turning { TURN_SPEED } else { 0.0 };
        self.angle = (self.angle + (on_its_own + by_hand) * ctx.dt()).rem_euclid(TAU);
        if on_its_own + by_hand == 0.0 {
            ctx.idle_ok(); // stopped on purpose, not frozen
        }
        ctx.report("angle_degrees", self.angle.to_degrees());
        ctx.report("turning", self.turning);
        ctx.report("triangles", self.robot.triangles.len());
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(SKY);
        // The sun comes from above, a little left, and from the camera's
        // side, so the robot's face is lit.
        canvas.set_lighting(Lighting {
            sun_direction: vec3(-0.4, 1.0, 0.8),
            ambient: 0.45,
            ..Lighting::default()
        });
        canvas.draw_mesh(&self.floor, &Transform::default(), &self.camera);
        let turntable = Transform::at(vec3(0.0, BASE / 2.0, 0.0))
            .rotated(vec3(0.0, self.angle, 0.0))
            .scaled(vec3(1.3, BASE, 1.3));
        canvas.draw_mesh(&self.base, &turntable, &self.camera);
        canvas.draw_mesh(&self.robot, &self.robot_place(), &self.camera);
        for place in self.hat_places() {
            canvas.draw_mesh(&self.hat, &place, &self.camera);
        }
        if let Some(rect) = self.on_screen(canvas) {
            canvas.label("robot", rect);
        }

        let about = format!(
            "A ROBOT MADE IN DUCK MOLD: {} TRIANGLES, A HAT ON HEAD.TOP",
            self.robot.triangles.len()
        );
        canvas.draw_text(&about, vec2(4.0, 4.0), 1, Color::WHITE);
        let keys = if self.turning {
            "SPACE: STOP   LEFT/RIGHT: TURN"
        } else {
            "SPACE: START   LEFT/RIGHT: TURN"
        };
        canvas.draw_text(keys, vec2(4.0, 12.0), 1, Color::GRAY);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        title: String::from("Turntable: a Duck Mold model - duckforge"),
        ..Config::default()
    };
    duckforge::run(config, Turntable::new()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulation() -> Simulation<Turntable> {
        Simulation::new(
            Config::default(),
            Turntable::new().expect("the robot loads"),
        )
    }

    /// How many pixels show the robot's eyes: the yellow of its "glow"
    /// colour (#FFE08A), lit. Nothing else in the picture is yellow.
    fn eye_pixels(sim: &mut Simulation<Turntable>) -> usize {
        let picture = sim.picture();
        let mut count = 0;
        for y in 0..picture.height() as i32 {
            for x in 0..picture.width() as i32 {
                let pixel = picture.get_pixel(x, y).unwrap();
                let (r, g, b) = (pixel.r as f32, pixel.g as f32, pixel.b as f32);
                if r > 150.0 && g > 120.0 && b < g * 0.75 {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn the_robot_faces_the_camera_at_the_start() {
        let mut sim = simulation();
        sim.step();
        assert_eq!(sim.number("triangles"), 384.0);
        assert!(
            eye_pixels(&mut sim) > 4,
            "its eyes are on its front, facing us"
        );
        // It's in the middle of the picture, and fills a good part of it.
        let robot = sim
            .scene()
            .labels
            .iter()
            .find(|l| l.name == "robot")
            .cloned();
        let robot = robot.expect("the robot is labelled");
        assert!(robot.on_screen);
        let middle = robot.screen.x + robot.screen.w / 2.0;
        assert!((middle - 160.0).abs() < 8.0, "centred ({middle})");
        assert!(
            robot.screen.h > 120.0,
            "big enough to see ({})",
            robot.screen.h
        );
    }

    #[test]
    fn half_a_turn_later_its_back_is_to_the_camera() {
        let mut sim = simulation();
        sim.run(600); // 10 seconds: half of a 20-second turn
        let angle = sim.number("angle_degrees");
        assert!((angle - 180.0).abs() < 1.0, "{angle}");
        assert_eq!(eye_pixels(&mut sim), 0, "no eyes on its back");
        sim.run(600);
        assert!(eye_pixels(&mut sim) > 4, "and round to the front again");
    }

    /// Where the hat's purple is in the picture: how many pixels, and
    /// their middle. Nothing else in the picture is purple.
    fn hat_pixels(sim: &mut Simulation<Turntable>) -> (usize, Vec2) {
        let picture = sim.picture();
        let (mut count, mut sum) = (0, vec2(0.0, 0.0));
        for y in 0..picture.height() as i32 {
            for x in 0..picture.width() as i32 {
                let pixel = picture.get_pixel(x, y).unwrap();
                let (r, g, b) = (i32::from(pixel.r), i32::from(pixel.g), i32::from(pixel.b));
                if b > g + 30 && r > g + 10 {
                    count += 1;
                    sum += vec2(x as f32, y as f32);
                }
            }
        }
        (count, sum * (1.0 / count.max(1) as f32))
    }

    #[test]
    fn the_hat_stays_on_the_robots_head_as_it_turns() {
        let mut sim = simulation();
        sim.step();
        let robot = sim
            .scene()
            .labels
            .iter()
            .find(|l| l.name == "robot")
            .cloned()
            .expect("the robot is labelled");
        let (count, middle) = hat_pixels(&mut sim);
        assert!(count > 100, "the hat shows ({count} pixels)");
        // Centred over the robot, at the top of it.
        let centre = robot.screen.x + robot.screen.w / 2.0;
        assert!((middle.x - centre).abs() < 4.0, "{middle:?}");
        assert!(
            middle.y < robot.screen.y + robot.screen.h * 0.2,
            "{middle:?}"
        );

        // Half a turn later the hat is still there, on the same head.
        sim.run(600);
        let (count_after, after) = hat_pixels(&mut sim);
        assert!(count_after > 100);
        assert!((after.x - middle.x).abs() < 4.0 && (after.y - middle.y).abs() < 4.0);
    }

    #[test]
    fn space_stops_it_and_the_arrows_turn_it_by_hand() {
        let mut sim = simulation();
        sim.run(60);
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        assert!(!sim.flag("turning"));
        let stopped = sim.number("angle_degrees");
        sim.run(60);
        assert_eq!(
            sim.number("angle_degrees"),
            stopped,
            "stopped means stopped"
        );
        // Right turns the other way from the turntable: back towards 0.
        sim.press(Key::Right);
        sim.run(30);
        sim.release(Key::Right);
        let turned = sim.number("angle_degrees");
        assert!(turned > 270.0, "past 0 and round to {turned}");
    }
}

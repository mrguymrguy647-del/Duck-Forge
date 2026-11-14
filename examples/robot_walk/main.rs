//! A model made with Duck Mold that moves: a robot that stands idle,
//! walks where the arrow keys send it, and waves. Its hat stays on.
//!
//! Run it with:   cargo run --example robot_walk
//! Controls:      the arrow keys walk it about, Space makes it wave,
//!                Escape quits.
//!
//! The robot is `assets/robot.glb`, made by Duck Mold from the recipe in
//! `art/robot.toml` (`art/README.md` says how). The recipe holds its
//! animations: `idle`, `walk` and `wave`. Playing them takes three lines:
//!
//! ```ignore
//! let mut robot = AnimatedModel::load("examples/robot_walk/assets/robot.glb")?; // in new()
//! if walking { robot.play("walk") } else { robot.play("idle") }                 // in update()
//! robot.update(ctx.dt());
//! canvas.draw_mesh(robot.mesh(), &place, &camera);                              // in draw()
//! ```
//!
//! `play` is safe to call every frame: asking for the animation that is
//! already playing changes nothing. Going from one animation to another
//! isn't a snap: the robot changes over in a moment.
//!
//! An animation moves the robot's parts, not the robot: `walk` swings
//! its legs in place. The game moves it along the floor, by changing
//! the `Transform` it is drawn with, and turns it to face the way it
//! goes.
//!
//! The hat isn't part of the model. The game asks where the socket
//! `head.top` is **now** (`robot.socket("head.top")`) and puts the hat
//! there, so it stays on while the head nods.

use std::f32::consts::{PI, TAU};

use duckforge::LoadError;
use duckforge::glb::AnimatedModel;
use duckforge::prelude::*;

const ASSETS: &str = "examples/robot_walk/assets";

const SKY: Color = Color::from_hex(0x2A_32_44);
const FLOOR_LIGHT: Color = Color::from_hex(0x4A_52_60);
const FLOOR_DARK: Color = Color::from_hex(0x3E_45_52);
const HAT: Color = Color::from_hex(0x5B_3A_8C);
/// The hat's crown, and its brim, in metres: tall enough to cover the
/// robot's antenna, which also stands on `head.top`.
const CROWN: Vec3 = vec3(0.3, 0.34, 0.3);
const BRIM: Vec3 = vec3(0.58, 0.04, 0.58);

/// How fast it walks, in metres a second. The walk animation takes two
/// steps in 0.8 seconds, each about 40 cm long, which is a metre a
/// second: at this speed its feet don't slide.
const WALK_SPEED: f32 = 1.0;
/// How fast it turns to face the way it walks: a full turn in a second.
const TURN_SPEED: f32 = TAU;
/// How far from the middle of the floor it may go.
const EDGE: f32 = 2.0;

struct RobotWalk {
    camera: Camera3D,
    robot: AnimatedModel,
    hat: Mesh,
    floor: Mesh,
    /// Where it stands.
    position: Vec3,
    /// The way it faces, in radians: 0 is towards the camera (+z).
    facing: f32,
}

impl RobotWalk {
    fn new() -> Result<RobotWalk, LoadError> {
        let mut robot = AnimatedModel::load(format!("{ASSETS}/robot.glb"))?;
        robot.play("idle");
        Ok(RobotWalk {
            // In front of the robot (its +z side) and above it, looking
            // back and down at the floor.
            camera: Camera3D::looking_at(vec3(0.0, 2.4, 5.0), vec3(0.0, 0.8, 0.0)),
            robot,
            hat: Mesh::cylinder(HAT, 16),
            floor: Mesh::checkerboard(12, 1.0, FLOOR_LIGHT, FLOOR_DARK),
            position: Vec3::ZERO,
            facing: 0.0,
        })
    }

    /// Where the robot is drawn: where it stands, facing its way.
    fn place(&self) -> Transform {
        Transform::at(self.position).rotated(vec3(0.0, self.facing, 0.0))
    }

    /// True while it is waving: from Space until the wave has played.
    fn waving(&self) -> bool {
        self.robot.playing() == Some("wave") && !self.robot.finished()
    }

    /// The hat's crown and brim, on the robot's `head.top` as it is now:
    /// the socket moves with the head, and leans with it when it nods.
    fn hat_places(&self) -> [Transform; 2] {
        let top = self
            .robot
            .socket("head.top")
            .expect("Duck Mold models keep their parts' sockets");
        let place = self.place();
        let up = top.direction();
        // How far the head leans forward or back. (A turn around x takes
        // up towards the back.)
        let lean = (-up.z).atan2(up.y);
        [CROWN, BRIM].map(|size| {
            // The cylinder is centred on its middle: lift each half its
            // height, along the way the socket faces.
            let middle = top.position() + up * (size.y / 2.0);
            Transform::at(place.apply(middle))
                .rotated(vec3(lean, self.facing, 0.0))
                .scaled(size)
        })
    }
}

impl Game for RobotWalk {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Space) && !self.waving() {
            self.robot.play("wave");
        }

        // The camera looks back at the robot from +z, so your right is
        // -x and "away" is -z.
        let step = vec3(
            ctx.input.axis(Key::Right, Key::Left),
            0.0,
            ctx.input.axis(Key::Up, Key::Down),
        );
        let walking = step != Vec3::ZERO && !self.waving();
        if walking {
            let step = step.normalized();
            self.position += step * (WALK_SPEED * ctx.dt());
            self.position.x = self.position.x.clamp(-EDGE, EDGE);
            self.position.z = self.position.z.clamp(-EDGE, EDGE);
            // Turn towards the way it goes, the short way round. (A
            // positive turn swings its front, +z, towards +x.)
            let wanted = step.x.atan2(step.z);
            let off = (wanted - self.facing + PI).rem_euclid(TAU) - PI;
            let most = TURN_SPEED * ctx.dt();
            self.facing = (self.facing + off.clamp(-most, most)).rem_euclid(TAU);
        }

        // What it plays. `play` changes nothing if that is playing
        // already, so it can be asked every frame.
        if !self.waving() {
            self.robot.play(if walking { "walk" } else { "idle" });
        }
        self.robot.update(ctx.dt());

        ctx.report("x", self.position.x);
        ctx.report("z", self.position.z);
        ctx.report("facing_degrees", self.facing.to_degrees());
        ctx.report("animation", self.robot.playing().unwrap_or("none"));
        ctx.report("seconds_in", self.robot.time());
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
        // An animated model's mesh is a mesh like any other.
        canvas.draw_mesh(self.robot.mesh(), &self.place(), &self.camera);
        for place in self.hat_places() {
            canvas.draw_mesh(&self.hat, &place, &self.camera);
        }
        if let Some((point, _)) = self
            .camera
            .project(self.position + vec3(0.0, 0.9, 0.0), canvas)
        {
            canvas.label(
                "robot",
                Rect::new(point.x - 20.0, point.y - 40.0, 40.0, 80.0),
            );
        }

        canvas.draw_text(
            "A ROBOT MADE IN DUCK MOLD: IDLE, WALK AND WAVE",
            vec2(4.0, 4.0),
            1,
            Color::WHITE,
        );
        canvas.draw_text(
            "ARROWS: WALK   SPACE: WAVE",
            vec2(4.0, 12.0),
            1,
            Color::GRAY,
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        title: String::from("Robot walk: a Duck Mold model that moves - duckforge"),
        ..Config::default()
    };
    duckforge::run(config, RobotWalk::new()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulation() -> Simulation<RobotWalk> {
        Simulation::new(
            Config::default(),
            RobotWalk::new().expect("the robot loads"),
        )
    }

    /// Where the hat's purple is in the picture: how many pixels, and
    /// their middle. Nothing else in the picture is purple.
    fn hat_pixels(sim: &mut Simulation<RobotWalk>) -> (usize, Vec2) {
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

    /// Where the robot's `head.top` is in the picture, now.
    fn head_top_on_screen(sim: &mut Simulation<RobotWalk>) -> Vec2 {
        let game = sim.game();
        let top = game.robot.socket("head.top").unwrap().position();
        let (at, camera) = (game.place().apply(top), game.camera);
        let (point, _) = camera
            .project(at, sim.picture())
            .expect("the robot is in view");
        point
    }

    #[test]
    fn it_stands_idle_until_an_arrow_walks_it() {
        let mut sim = simulation();
        sim.run(30);
        assert_eq!(sim.text("animation"), "idle");
        assert_eq!((sim.number("x"), sim.number("z")), (0.0, 0.0));

        // Right is your right: -x, seen from the camera.
        sim.press(Key::Right);
        sim.run(60);
        assert_eq!(sim.text("animation"), "walk");
        let x = sim.number("x");
        assert!((x + 1.0).abs() < 0.02, "a metre in a second: {x}");
        assert_eq!(sim.number("z"), 0.0);
        // It has turned to face the way it walks.
        let facing = sim.number("facing_degrees");
        assert!((facing - 270.0).abs() < 1.0, "{facing}");

        sim.release(Key::Right);
        sim.run(30);
        assert_eq!(sim.text("animation"), "idle");
        assert_eq!(sim.number("x"), x, "standing still");
    }

    #[test]
    fn walking_swings_its_legs_and_standing_brings_them_back() {
        let mut sim = simulation();
        sim.step();
        // The lowest and highest corner in front of and behind it, in its
        // own space: at rest its feet reach 21 cm forward.
        let reach = |sim: &Simulation<RobotWalk>| {
            let (low, high) = sim.game().robot.mesh().bounds();
            (low.z, high.z)
        };
        let (back, front) = reach(&sim);
        sim.press(Key::Up);
        // The walk starts on a step, with one leg forward and one back.
        // (It changes over to the walk in 0.15 seconds.)
        let mut longest: f32 = 0.0;
        for _ in 0..48 {
            sim.step();
            let (low, high) = reach(&sim);
            longest = longest.max(high - low);
        }
        assert!(
            longest > (front - back) + 0.25,
            "its legs spread: {longest} against {}",
            front - back
        );
        // Its feet stay on the ground while they do.
        let (low, _) = sim.game().robot.mesh().bounds();
        assert!(low.y.abs() < 0.01, "{}", low.y);

        sim.release(Key::Up);
        sim.run(120);
        let (low, high) = reach(&sim);
        assert!((high - low - (front - back)).abs() < 0.02, "standing again");
    }

    #[test]
    fn space_makes_it_wave_and_then_it_stands_idle_again() {
        let mut sim = simulation();
        sim.run(10);
        let (_, at_rest) = sim.game().robot.mesh().bounds();
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        assert_eq!(sim.text("animation"), "wave");
        // Half a second in, its arm is up: higher than its antenna.
        sim.run(30);
        let (_, waving) = sim.game().robot.mesh().bounds();
        assert!(
            waving.y > at_rest.y + 0.05,
            "{} against {}",
            waving.y,
            at_rest.y
        );
        // It doesn't walk off while it waves.
        sim.press(Key::Left);
        sim.run(20);
        assert_eq!(sim.number("x"), 0.0);
        sim.release(Key::Left);
        // The wave lasts a second and a half; then it is idle again.
        sim.run(60);
        assert_eq!(sim.text("animation"), "idle");
        sim.run(30);
        let (_, after) = sim.game().robot.mesh().bounds();
        assert!((after.y - at_rest.y).abs() < 0.02, "its arm is down again");
    }

    #[test]
    fn the_hat_stays_on_its_head_whatever_it_does() {
        let mut sim = simulation();
        sim.step();
        let (count, _) = hat_pixels(&mut sim);
        assert!(count > 40, "the hat shows ({count} pixels)");
        // Standing, walking away and to the side, and waving: the hat's
        // middle is always just above the top of the head.
        let check = |sim: &mut Simulation<RobotWalk>, when: &str| {
            let (count, hat) = hat_pixels(sim);
            assert!(count > 20, "{when}: the hat shows ({count} pixels)");
            let head = head_top_on_screen(sim);
            assert!(
                (hat.x - head.x).abs() < 4.0 && hat.y < head.y && hat.y > head.y - 16.0,
                "{when}: the hat is at {hat:?}, the top of the head at {head:?}"
            );
        };
        check(&mut sim, "standing");
        sim.press(Key::Up);
        for _ in 0..6 {
            sim.run(10);
            check(&mut sim, "walking away");
        }
        sim.release(Key::Up);
        sim.press(Key::Left);
        for _ in 0..6 {
            sim.run(10);
            check(&mut sim, "walking left");
        }
        sim.release(Key::Left);
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        for _ in 0..9 {
            sim.run(10);
            check(&mut sim, "waving");
        }
    }

    #[test]
    fn it_stops_at_the_edge_of_the_floor() {
        let mut sim = simulation();
        sim.press(Key::Down);
        sim.run(60 * 5);
        assert_eq!(sim.number("z"), f64::from(EDGE));
        assert_eq!(sim.text("animation"), "walk");
    }

    #[test]
    fn the_same_keys_give_the_same_robot() {
        // Time only passes in `update`, by the `dt` the game is given:
        // two runs with the same keys end with every corner in the same
        // place.
        let run = || {
            let mut sim = simulation();
            sim.press(Key::Up);
            sim.run(37);
            sim.press(Key::Right);
            sim.run(41);
            sim.release(Key::Up);
            sim.release(Key::Right);
            sim.press(Key::Space);
            sim.run(23);
            let game = sim.into_game();
            (
                game.robot.mesh().vertices.clone(),
                game.position,
                game.facing,
            )
        };
        assert_eq!(run(), run());
    }
}

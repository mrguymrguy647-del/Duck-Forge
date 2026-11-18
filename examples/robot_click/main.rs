//! A Duck Mold robot that a game can hear and touch: it walks round the
//! floor, each footstep makes a sound and a puff of dust, a click on it
//! says which part was clicked, which flashes, and it bumps into a wall
//! in its way, stops, and goes round another way.
//!
//! Run it with:   cargo run --example robot_click
//! Controls:      click the robot. Escape quits.
//!
//! The robot is `assets/robot.glb`, made by Duck Mold from the recipe in
//! `art/robot.toml` (`art/README.md` says how). Two things in the recipe
//! are for games:
//!
//! - **Events**: its walk has `event = "footstep"` on the two keys where a
//!   foot comes down. After each update, `robot.events()` lists the
//!   events the animation passed, each once:
//!
//!   ```ignore
//!   robot.update(ctx.dt());
//!   for event in robot.events() {
//!       if event.name == "footstep" { ctx.play(step_sound) }
//!   }
//!   ```
//!
//! - **Hit shapes**: `colliders = "parts"` gives each part a box, a ball
//!   or a capsule that moves with it. A click is a ray from the eye
//!   through the mouse; the robot says which part it reaches first:
//!
//!   ```ignore
//!   if let Some(hit) = ctx.mouse_ray(&camera).and_then(|ray| robot.hit_ray(ray, &place)) {
//!       hit.part // "head", "left_arm/hand"
//!   }
//!   ```
//!
//! - **A body for physics**: the same hit shapes, as the robot stands at
//!   rest, give it a block for Duck Forge's physics, so it stands on the
//!   floor and can't walk through the wall:
//!
//!   ```ignore
//!   let body = world.add(robot.model().physics_body(feet));
//!   // each frame: set its speed, step the world, and draw it where it is
//!   world.step(dt);
//!   let place = Transform::at(robot.model().stands_at(world.get(body).unwrap()));
//!   ```

use std::f32::consts::{PI, TAU};

use duckforge::LoadError;
use duckforge::glb::AnimatedModel;
use duckforge::physics::{Body, BodyId, PhysicsWorld};
use duckforge::prelude::*;

const ASSETS: &str = "examples/robot_click/assets";

const SKY: Color = Color::from_hex(0x2A_32_44);
const FLOOR_LIGHT: Color = Color::from_hex(0x4A_52_60);
const FLOOR_DARK: Color = Color::from_hex(0x3E_45_52);
/// A part that was clicked flashes this colour.
const FLASH: Color = Color::from_hex(0xFF_F0_9A);
/// A puff of dust, when it is new.
const DUST: Color = Color::from_hex(0xB8_B0_A0);
/// The wall in its way.
const WALL: Color = Color::from_hex(0x8A_6E_52);
/// The wall's middle and size, in metres: across the side of its square
/// from (1.5, 1) to (-1.5, 1).
const WALL_AT: Vec3 = vec3(0.0, 0.4, 1.0);
const WALL_SIZE: Vec3 = vec3(0.3, 0.8, 1.4);
/// How long it pushes against something before it gives up and heads for
/// the next corner, in seconds.
const GIVE_UP: f32 = 0.8;

/// How fast it walks, in metres a second: its walk takes two steps of
/// about 40 cm in 0.8 seconds, so its feet don't slide.
const WALK_SPEED: f32 = 1.0;
/// How fast it turns to face where it goes: half round in a second.
const TURN_SPEED: f32 = PI;
/// The corners of the square it walks round, in metres.
const ROUND: [Vec3; 4] = [
    vec3(1.5, 0.0, 1.0),
    vec3(-1.5, 0.0, 1.0),
    vec3(-1.5, 0.0, -1.5),
    vec3(1.5, 0.0, -1.5),
];
/// How long a clicked part flashes, and how long a puff lasts, in
/// seconds.
const FLASH_TIME: f32 = 0.6;
const PUFF_TIME: f32 = 0.5;

/// The sound of a footstep: a short, soft thud of noise.
fn step_sound() -> Sound {
    Sound {
        name: "step",
        wave: Wave::Noise,
        freq: 180.0,
        slide_to: 80.0,
        ms: 70.0,
        volume: 0.25,
        attack: 1.0,
        decay: 60.0,
    }
}

/// A puff of dust where a foot came down.
struct Puff {
    at: Vec3,
    age: f32,
}

struct RobotClick {
    camera: Camera3D,
    robot: AnimatedModel,
    floor: Mesh,
    /// Every triangle's own colour, to put back after a flash.
    colors: Vec<Color>,
    position: Vec3,
    /// The way it faces, in radians: 0 is towards the camera (+z).
    facing: f32,
    /// The corner of `ROUND` it is walking to.
    heading_to: usize,
    puffs: Vec<Puff>,
    /// The world its body is in: the floor, the wall and the robot.
    world: PhysicsWorld<Vec3>,
    body: BodyId,
    wall: Mesh,
    /// How long it has been stopped by something in its way.
    blocked_for: f32,
    /// How many times it has bumped into the wall.
    bumps: u32,
    /// The part last clicked, and how long ago.
    clicked: Option<(String, f32)>,
    /// What the last click found, to show: a part's name, or that it
    /// missed.
    said: String,
    footsteps: u32,
}

impl RobotClick {
    fn new() -> Result<RobotClick, LoadError> {
        let mut robot = AnimatedModel::load(format!("{ASSETS}/robot.glb"))?;
        robot.play("walk");
        let colors = robot
            .mesh()
            .triangles
            .iter()
            .map(|triangle| triangle.color)
            .collect();
        // The floor (its top at y = 0), the wall, and the robot's body: a
        // block round its hit shapes, standing on the floor where it starts.
        let mut world = PhysicsWorld::new(vec3(0.0, -9.8, 0.0));
        world.add(Body::block(vec3(0.0, -0.5, 0.0), vec3(12.0, 1.0, 12.0)).fixed());
        world.add(Body::block(WALL_AT, WALL_SIZE).fixed());
        let body = world.add(
            robot
                .model()
                .physics_body(ROUND[3])
                .with_friction(0.0)
                .with_bounce(0.0),
        );
        Ok(RobotClick {
            camera: Camera3D::looking_at(vec3(0.0, 3.0, 6.0), vec3(0.0, 0.6, -0.2)),
            robot,
            world,
            body,
            wall: Mesh::cube(WALL),
            blocked_for: 0.0,
            bumps: 0,
            floor: Mesh::checkerboard(12, 1.0, FLOOR_LIGHT, FLOOR_DARK),
            colors,
            position: ROUND[3],
            facing: 0.0,
            heading_to: 0,
            puffs: Vec::new(),
            clicked: None,
            said: String::from("CLICK THE ROBOT"),
            footsteps: 0,
        })
    }

    /// Where the robot is drawn: where it stands, facing its way.
    fn place(&self) -> Transform {
        Transform::at(self.position).rotated(vec3(0.0, self.facing, 0.0))
    }

    /// Walks on towards the next corner, turning to face it. Its body
    /// moves in the physics world, so the wall stops it; stopped for a
    /// moment, it heads for the next corner instead.
    fn walk(&mut self, dt: f32) {
        let mut to = ROUND[self.heading_to] - self.position;
        to.y = 0.0;
        let far = to.length();
        if far < 0.05 {
            self.heading_to = (self.heading_to + 1) % ROUND.len();
            return;
        }
        let way = to * (1.0 / far);
        let speed = WALK_SPEED.min(far / dt.max(1e-6));
        if let Some(body) = self.world.get_mut(self.body) {
            body.velocity.x = way.x * speed;
            body.velocity.z = way.z * speed;
        }
        let before = self.position;
        self.world.step(dt);
        if let Some(body) = self.world.get(self.body) {
            self.position = self.robot.model().stands_at(body);
        }
        // Did it get where it meant to?
        let moved = vec3(self.position.x - before.x, 0.0, self.position.z - before.z).length();
        if moved < speed * dt * 0.3 {
            if self.blocked_for == 0.0 {
                self.bumps += 1;
            }
            self.blocked_for += dt;
            if self.blocked_for > GIVE_UP {
                self.heading_to = (self.heading_to + 1) % ROUND.len();
                self.blocked_for = 0.0;
            }
        } else {
            self.blocked_for = 0.0;
        }
        // Turn towards the way it goes, the short way round. (A positive
        // turn swings its front, +z, towards +x.)
        let wanted = way.x.atan2(way.z);
        let off = (wanted - self.facing + PI).rem_euclid(TAU) - PI;
        let most = TURN_SPEED * dt;
        self.facing = (self.facing + off.clamp(-most, most)).rem_euclid(TAU);
    }

    /// The foot that came down: the one further forward.
    fn landing_foot(&self) -> Vec3 {
        let place = self.place();
        ["left_leg/foot.bottom", "right_leg/foot.bottom"]
            .map(|name| {
                self.robot
                    .socket(name)
                    .expect("the robot's feet have sockets")
                    .position()
            })
            .into_iter()
            .max_by(|a, b| a.z.total_cmp(&b.z))
            .map(|foot| place.apply(foot))
            .unwrap_or(self.position)
    }

    /// Paints the clicked part's triangles: the flash colour on and off,
    /// then its own colours again.
    fn paint(&mut self) {
        let flashing = self.clicked.as_ref().and_then(|(part, since)| {
            let on = *since < FLASH_TIME && ((since / 0.1) as u32).is_multiple_of(2);
            on.then(|| self.robot.model().part(part).map(|part| part.range()))
                .flatten()
        });
        let mesh = self.robot.mesh_mut();
        for (triangle, own) in mesh.triangles.iter_mut().zip(&self.colors) {
            triangle.color = *own;
        }
        if let Some(range) = flashing {
            for triangle in &mut mesh.triangles[range] {
                triangle.color = FLASH;
            }
        }
    }
}

impl Game for RobotClick {
    fn update(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        self.walk(dt);
        self.robot.update(dt);

        // Each footstep, once: a sound, and a puff where the foot is.
        let steps = self
            .robot
            .events()
            .iter()
            .filter(|event| event.name == "footstep")
            .count();
        for _ in 0..steps {
            ctx.play(step_sound());
            self.puffs.push(Puff {
                at: self.landing_foot(),
                age: 0.0,
            });
            self.footsteps += 1;
        }
        for puff in &mut self.puffs {
            puff.age += dt;
        }
        self.puffs.retain(|puff| puff.age < PUFF_TIME);

        // A click: the ray from the eye through the mouse, and the first
        // part of the robot, as it stands now, on it.
        if ctx.input.was_mouse_pressed(MouseButton::Left) {
            let place = self.place();
            match ctx
                .mouse_ray(&self.camera)
                .and_then(|ray| self.robot.hit_ray(ray, &place))
            {
                Some(hit) => {
                    self.said = format!("YOU CLICKED ITS {}", hit.part.to_uppercase());
                    self.clicked = Some((hit.part, 0.0));
                }
                None => self.said = String::from("MISSED: CLICK THE ROBOT"),
            }
        }
        if let Some((_, since)) = &mut self.clicked {
            *since += dt;
        }
        self.paint();

        ctx.report("x", self.position.x);
        ctx.report("z", self.position.z);
        ctx.report("footsteps", f64::from(self.footsteps));
        ctx.report("bumps", f64::from(self.bumps));
        ctx.report("puffs", self.puffs.len() as f64);
        ctx.report(
            "clicked",
            self.clicked
                .as_ref()
                .map_or("none", |(part, _)| part.as_str()),
        );
        ctx.report("said", self.said.as_str());
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(SKY);
        canvas.set_lighting(Lighting {
            sun_direction: vec3(-0.4, 1.0, 0.8),
            ambient: 0.45,
            ..Lighting::default()
        });
        canvas.draw_mesh(&self.floor, &Transform::default(), &self.camera);
        canvas.draw_mesh(
            &self.wall,
            &Transform::at(WALL_AT).scaled(WALL_SIZE),
            &self.camera,
        );
        // The puffs: a disc on the ground that grows and fades into the
        // floor, drawn before the robot so its feet stand in them.
        for puff in &self.puffs {
            let part = puff.age / PUFF_TIME;
            if let Some((centre, depth)) =
                self.camera.project(puff.at + vec3(0.0, 0.03, 0.0), canvas)
            {
                let radius = (0.06 + 0.12 * part) * canvas.height() as f32 / depth;
                canvas.fill_circle(centre, radius, DUST.lerp(FLOOR_LIGHT, part));
            }
        }
        // Far off it would draw a simpler version (its recipe has `lods`);
        // here it is always close, so this is the robot itself.
        self.robot.draw_mesh(canvas, &self.place(), &self.camera);
        if let Some((point, _)) = self
            .camera
            .project(self.position + vec3(0.0, 0.9, 0.0), canvas)
        {
            canvas.label(
                "robot",
                Rect::new(point.x - 20.0, point.y - 40.0, 40.0, 80.0),
            );
        }
        canvas.draw_text(&self.said, vec2(4.0, 4.0), 1, Color::WHITE);
        canvas.draw_text(
            "IT WALKS ROUND; EACH STEP MAKES A SOUND AND A PUFF; THE WALL STOPS IT",
            vec2(4.0, 12.0),
            1,
            Color::GRAY,
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        title: String::from("Robot click: what was hit, and footsteps - duckforge"),
        ..Config::default()
    };
    duckforge::run(config, RobotClick::new()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulation() -> Simulation<RobotClick> {
        Simulation::new(
            Config::default(),
            RobotClick::new().expect("the robot loads"),
        )
    }

    /// Where a part's hit shape's middle is on the screen, now.
    fn on_screen(sim: &mut Simulation<RobotClick>, part: &str) -> Vec2 {
        let game = sim.game();
        let place = game.place();
        let shape = game
            .robot
            .hit_shapes(&place)
            .into_iter()
            .find(|shape| shape.part == part)
            .unwrap_or_else(|| panic!("the robot has a hit shape for {part}"));
        let middle = match shape.volume {
            duckforge::hits::HitVolume::Box { center, .. }
            | duckforge::hits::HitVolume::Ball { center, .. } => center,
            duckforge::hits::HitVolume::Capsule { a, b, .. } => (a + b) * 0.5,
        };
        let camera = game.camera;
        camera
            .world_to_screen(middle, sim.picture())
            .expect("in view")
    }

    fn click(sim: &mut Simulation<RobotClick>, at: Vec2) {
        sim.move_mouse(at);
        sim.press_mouse(MouseButton::Left);
        sim.step();
        sim.release_mouse(MouseButton::Left);
    }

    #[test]
    fn the_wall_stops_it_and_it_goes_round_another_way() {
        let mut sim = simulation();
        let wall_low = WALL_AT - WALL_SIZE * 0.5;
        let wall_high = WALL_AT + WALL_SIZE * 0.5;
        for _ in 0..600 {
            sim.step();
            let game = sim.game();
            // Its body never goes into the wall.
            let body = game.world.get(game.body).unwrap();
            let duckforge::physics::Shape::Block { half_size } = body.shape else {
                panic!("a block");
            };
            let (low, high) = (body.position - half_size, body.position + half_size);
            let inside = (0..3).all(|axis| {
                let get = |v: Vec3| [v.x, v.y, v.z][axis];
                get(low) < get(wall_high) - 0.02 && get(high) > get(wall_low) + 0.02
            });
            assert!(!inside, "in the wall at {:?}", body.position);
            // And it stands on the floor.
            assert!(game.position.y.abs() < 0.02, "{:?}", game.position);
        }
        let game = sim.game();
        assert!(game.bumps >= 1, "it met the wall");
        // It went on: not still pushing against the wall.
        assert!(game.blocked_for < GIVE_UP);
    }

    /// Saves a picture of it at the wall, for the docs, to the file
    /// `ROBOT_CLICK_PICTURE` names. Run by hand.
    #[test]
    #[ignore = "run by hand, to make a picture for the docs"]
    fn a_picture_at_the_wall() {
        let Some(path) = std::env::var_os("ROBOT_CLICK_PICTURE") else {
            return;
        };
        let mut sim = simulation();
        while sim.game().bumps == 0 {
            sim.step();
        }
        sim.run(20);
        sim.picture().save_png(path).unwrap();
    }

    #[test]
    fn each_footstep_is_a_sound_and_a_puff() {
        let mut sim = simulation();
        // A little over four seconds of walking: five rounds of the
        // walk, two steps each, and the first step it starts on.
        sim.run(250);
        let steps = sim.number("footsteps");
        assert_eq!(steps, 11.0);
        let sounds = sim.sound_frames("step");
        assert_eq!(sounds.len(), 11);
        // A step every 0.4 s: 24 frames apart.
        for pair in sounds.windows(2) {
            assert!((23..=25).contains(&(pair[1] - pair[0])), "{sounds:?}");
        }
        // The puffs are on the ground, by its feet, and fade away.
        let game = sim.game();
        assert!(!game.puffs.is_empty());
        for puff in &game.puffs {
            assert!(puff.at.y.abs() < 0.02, "{:?}", puff.at);
            assert!((puff.at - game.position).length() < 0.5, "{:?}", puff.at);
        }
        sim.run(1);
        assert!(sim.number("puffs") <= 2.0);
    }

    #[test]
    fn it_walks_round_the_square() {
        let mut sim = simulation();
        let mut corners = 0;
        let mut last = 0;
        for _ in 0..(60 * 12) {
            sim.step();
            let heading = sim.game().heading_to;
            if heading != last {
                corners += 1;
                last = heading;
            }
            let (x, z) = (sim.number("x"), sim.number("z"));
            assert!(x.abs() <= 1.51 && (-1.51..=1.01).contains(&z), "{x} {z}");
        }
        // 10 metres round, at a metre a second: once round, and a bit.
        assert!(corners >= 4, "{corners}");
    }

    #[test]
    fn a_click_names_the_part_and_it_flashes() {
        let mut sim = simulation();
        sim.run(20);
        assert_eq!(sim.text("clicked"), "none");
        // The head, while it walks.
        let head = on_screen(&mut sim, "head");
        click(&mut sim, head);
        assert_eq!(sim.text("clicked"), "head");
        assert_eq!(sim.text("said"), "YOU CLICKED ITS HEAD");
        // Its triangles flash; the others keep their colours.
        let game = sim.game();
        let range = game.robot.model().part("head").unwrap().range();
        let mesh = game.robot.mesh();
        assert!(
            mesh.triangles[range.clone()]
                .iter()
                .all(|t| t.color == FLASH)
        );
        let others = mesh
            .triangles
            .iter()
            .enumerate()
            .filter(|(at, _)| !range.contains(at))
            .all(|(at, t)| t.color == game.colors[at]);
        assert!(others);
        // Then it stops flashing and has its own colours again.
        sim.run(60);
        let game = sim.game();
        let mesh = game.robot.mesh();
        assert!(
            mesh.triangles
                .iter()
                .zip(&game.colors)
                .all(|(t, own)| t.color == *own)
        );
        assert_eq!(
            sim.text("clicked"),
            "head",
            "it still says what was clicked"
        );
    }

    #[test]
    fn every_part_with_a_hit_shape_can_be_clicked_as_it_walks() {
        let mut sim = simulation();
        let mut named = Vec::new();
        for part in ["body", "head", "left_leg/foot", "right_arm/hand", "antenna"] {
            sim.run(17);
            let at = on_screen(&mut sim, part);
            // What a ray through that point reaches first: the part, or
            // something in front of it.
            let expected = {
                let (width, height) = (sim.picture().width(), sim.picture().height());
                let game = sim.game();
                let ray = game.camera.ray_through(at, &Canvas::new(width, height));
                game.robot.hit_ray(ray, &game.place()).unwrap().part
            };
            click(&mut sim, at);
            assert_eq!(sim.text("clicked"), expected, "aiming at {part}");
            named.push(expected);
        }
        assert!(
            named.iter().filter(|part| part.as_str() != "body").count() >= 3,
            "{named:?}"
        );
    }

    #[test]
    fn a_click_beside_it_misses() {
        let mut sim = simulation();
        sim.run(5);
        click(&mut sim, vec2(5.0, 200.0));
        assert_eq!(sim.text("clicked"), "none");
        assert_eq!(sim.text("said"), "MISSED: CLICK THE ROBOT");
    }

    #[test]
    fn the_same_clicks_give_the_same_game() {
        let run = || {
            let mut sim = simulation();
            sim.run(31);
            let at = on_screen(&mut sim, "head");
            click(&mut sim, at);
            sim.run(47);
            let game = sim.into_game();
            (
                game.robot.mesh().vertices.clone(),
                game.position,
                game.footsteps,
                game.clicked,
            )
        };
        assert_eq!(run(), run());
    }
}

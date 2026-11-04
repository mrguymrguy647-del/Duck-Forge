//! A 2D physics sandbox: drop balls and boxes onto a board of pegs.
//!
//! Run it with:   cargo run --release --example physics
//! Controls:      left click drops a ball, right click drops a box,
//!                Space drops a shower of balls, C clears, Escape quits.

use std::collections::{HashMap, VecDeque};

use duckforge::physics::{Body, BodyId, PhysicsWorld, Shape};
use duckforge::prelude::*;

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;
const BACKGROUND: Color = Color::from_hex(0x1A_1C_2C);
const WALL_COLOR: Color = Color::from_hex(0x56_5A_75);
const PEG_COLOR: Color = Color::from_hex(0x94_B0_C2);
const PALETTE: [Color; 5] = [
    Color::RED,
    Color::ORANGE,
    Color::YELLOW,
    Color::GREEN,
    Color::BLUE,
];

/// Pixels per second per second. Screen y points down, so gravity is +y.
const GRAVITY: f32 = 400.0;
const WALL_THICKNESS: f32 = 8.0;
/// Oldest bodies are removed once there are more than this many, to keep it fast.
const MAX_DROPPED: usize = 200;
/// Bodies slower than this (pixels per second) count as resting.
const MOVING_SPEED: f32 = 5.0;
/// How long (in seconds) a peg glows after something touches it.
const GLOW_TIME: f32 = 0.3;

struct Sandbox {
    world: PhysicsWorld<Vec2>,
    /// The bodies players dropped, oldest first.
    dropped: VecDeque<BodyId>,
    /// The color of each dropped body.
    colors: HashMap<BodyId, Color>,
    pegs: Vec<BodyId>,
    /// Seconds of glow left for each peg that was recently hit.
    glow: HashMap<BodyId, f32>,
}

impl Sandbox {
    fn new() -> Self {
        let mut world = PhysicsWorld::new(vec2(0.0, GRAVITY));

        // The floor and walls: fixed blocks.
        let t = WALL_THICKNESS;
        world.add(Body::block(vec2(WIDTH / 2.0, HEIGHT - t / 2.0), vec2(WIDTH, t)).fixed());
        world.add(Body::block(vec2(t / 2.0, HEIGHT / 2.0), vec2(t, HEIGHT)).fixed());
        world.add(Body::block(vec2(WIDTH - t / 2.0, HEIGHT / 2.0), vec2(t, HEIGHT)).fixed());

        // Rows of pegs: fixed balls, every other row shifted by half a gap.
        let mut pegs = Vec::new();
        for row in 0..5 {
            let y = 60.0 + row as f32 * 22.0;
            let shift = if row % 2 == 0 { 0.0 } else { 14.0 };
            let mut x = 30.0 + shift;
            while x < WIDTH - 24.0 {
                // Frictionless, so balls slide off instead of balancing on top.
                let peg = Body::ball(vec2(x, y), 3.0).fixed().with_friction(0.0);
                pegs.push(world.add(peg));
                x += 28.0;
            }
        }

        Self {
            world,
            dropped: VecDeque::new(),
            colors: HashMap::new(),
            pegs,
            glow: HashMap::new(),
        }
    }

    fn drop_body(&mut self, body: Body<Vec2>, rng: &mut Rng) {
        let id = self.world.add(body);
        let color = PALETTE[rng.range_i32(0, PALETTE.len() as i32) as usize];
        self.colors.insert(id, color);
        self.dropped.push_back(id);

        // Too many? Remove the oldest one.
        if self.dropped.len() > MAX_DROPPED {
            if let Some(oldest) = self.dropped.pop_front() {
                self.world.remove(oldest);
                self.colors.remove(&oldest);
            }
        }
    }

    fn drop_ball(&mut self, at: Vec2, rng: &mut Rng) {
        let radius = rng.range(3.5, 6.0);
        let ball = Body::ball(at, radius).with_bounce(0.5);
        self.drop_body(ball, rng);
    }

    fn drop_box(&mut self, at: Vec2, rng: &mut Rng) {
        let size = vec2(rng.range(8.0, 16.0), rng.range(8.0, 16.0));
        self.drop_body(Body::block(at, size).with_friction(0.6), rng);
    }

    fn clear(&mut self) {
        for id in self.dropped.drain(..) {
            self.world.remove(id);
        }
        self.colors.clear();
    }
}

impl Game for Sandbox {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.was_pressed(Key::C) {
            self.clear();
        }

        // Drop things where the mouse is (if it's inside the walls).
        let mouse = ctx.input.mouse_position();
        let inside = Rect::new(16.0, 16.0, WIDTH - 32.0, HEIGHT - 40.0);
        if inside.contains(mouse) {
            if ctx.input.was_mouse_pressed(MouseButton::Left) {
                self.drop_ball(mouse, &mut ctx.rng);
            }
            if ctx.input.was_mouse_pressed(MouseButton::Right) {
                self.drop_box(mouse, &mut ctx.rng);
            }
        }
        if ctx.input.was_pressed(Key::Space) {
            for _ in 0..15 {
                let at = vec2(ctx.rng.range(20.0, WIDTH - 20.0), ctx.rng.range(10.0, 40.0));
                self.drop_ball(at, &mut ctx.rng);
            }
        }

        self.world.step(ctx.dt());

        // Light up every peg that something touched.
        for glow in self.glow.values_mut() {
            *glow -= ctx.dt();
        }
        self.glow.retain(|_, time_left| *time_left > 0.0);
        for contact in self.world.contacts() {
            for id in [contact.a, contact.b] {
                if self.pegs.contains(&id) {
                    self.glow.insert(id, GLOW_TIME);
                }
            }
        }

        // Tell tests and headless runs what's going on (see log.json).
        let dropped = || self.dropped.iter().filter_map(|&id| self.world.get(id));
        let moving = dropped()
            .filter(|body| body.velocity.length() > MOVING_SPEED)
            .count();
        let arena = Rect::new(0.0, 0.0, WIDTH, HEIGHT);
        let escaped = dropped()
            .filter(|body| !arena.contains(body.position))
            .count();
        ctx.report("bodies", self.dropped.len());
        ctx.report("moving", moving);
        ctx.report("escaped", escaped);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(BACKGROUND);

        for (id, body) in self.world.bodies() {
            let color = if let Some(&color) = self.colors.get(&id) {
                color
            } else if let Some(&time_left) = self.glow.get(&id) {
                PEG_COLOR.lerp(Color::WHITE, time_left / GLOW_TIME)
            } else if self.pegs.contains(&id) {
                PEG_COLOR
            } else {
                WALL_COLOR
            };

            match body.shape {
                Shape::Ball { radius } => canvas.fill_circle(body.position, radius, color),
                Shape::Block { half_size } => {
                    let size = half_size * 2.0;
                    canvas.fill_rect(Rect::from_center(body.position, size.x, size.y), color);
                }
            }
        }

        canvas.draw_text(
            "LEFT CLICK: BALL   RIGHT CLICK: BOX",
            vec2(14.0, 12.0),
            1,
            Color::WHITE,
        );
        canvas.draw_text("SPACE: SHOWER   C: CLEAR", vec2(14.0, 20.0), 1, Color::GRAY);
        let count = format!("{} BODIES", self.dropped.len());
        let x = WIDTH - 14.0 - Canvas::text_width(&count, 1);
        canvas.draw_text(&count, vec2(x, 20.0), 1, Color::GRAY);
    }
}

fn config() -> Config {
    Config {
        title: String::from("Physics sandbox - duckforge"),
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Sandbox::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_things_fall_through_the_pegs_and_come_to_rest() {
        // Two balls and a box, clicked across the top (a script written by hand).
        let played = PlayTest::new(config(), Sandbox::new())
            .inputs("playthroughs/physics-three-drops.json")
            .run();
        assert_eq!(played.number("bodies"), 3.0);
        assert_eq!(played.number("escaped"), 0.0, "something left the box");
        assert_eq!(played.number("moving"), 0.0, "something never settled");
        // ...and they ended up on the floor, below the pegs.
        for (_, body) in played.game().world.bodies() {
            if !body.is_fixed() {
                assert!(body.position.y > 170.0, "stuck at {:?}", body.position);
            }
        }
    }

    #[test]
    fn a_shower_drops_fifteen_balls_and_c_clears_them() {
        let script = r#"{"events": [
            {"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"},
            {"frame": 300, "press": "C"}, {"frame": 301, "release": "C"}
        ]}"#;
        let played = PlayTest::new(config(), Sandbox::new())
            .script(script)
            .frames(320)
            .run();
        assert_eq!(
            played
                .reported_at(299, "bodies")
                .and_then(|v| v.as_number()),
            Some(15.0)
        );
        assert_eq!(
            played
                .reported_at(299, "escaped")
                .and_then(|v| v.as_number()),
            Some(0.0)
        );
        assert_eq!(played.number("bodies"), 0.0);
    }

    #[test]
    fn the_oldest_bodies_go_once_there_are_too_many() {
        // 20 showers of 15 balls: 300 balls, but at most MAX_DROPPED stay.
        let events: Vec<String> = (0..20)
            .flat_map(|i| {
                let frame = 10 + i * 20;
                [
                    format!(r#"{{"frame": {frame}, "press": "Space"}}"#),
                    format!(r#"{{"frame": {}, "release": "Space"}}"#, frame + 1),
                ]
            })
            .collect();
        let script = format!(r#"{{"events": [{}]}}"#, events.join(", "));
        let played = PlayTest::new(config(), Sandbox::new())
            .script(&script)
            .frames(600)
            .run();
        assert_eq!(played.number("bodies"), MAX_DROPPED as f64);
        assert_eq!(
            played.number("escaped"),
            0.0,
            "a crowd pushed something out"
        );
    }
}

//! A 3D runner to copy when you begin a new 3D game.
//!
//!     cargo run --example starter_runner3d
//!
//! Left/Right (or A/D) change lane, Space/Up/W jumps, Escape quits. After
//! a game over, Space or Enter starts again. F1 shows the debug overlay,
//! F10 records what you do.
//!
//! The duck runs down a three-lane road that never ends, past crates
//! (jump over them, or change lane) and gold coins. It has the parts most
//! small 3D games need:
//!
//! - a camera that follows from behind and above;
//! - textured meshes: the road and grass are long strips whose texture
//!   repeats, crates and coins are blocks from one Duck Quill atlas
//!   (`art/blocks.json`), all lit by a warm sun;
//! - the duck as a billboard: a sprite standing in the 3D world;
//! - a HUD drawn over the 3D, sounds from Duck Echo and a looping hum;
//! - labels, checks, reports, play tests in `tests.rs`, and a bot in
//!   `bot.rs` that plays it and records `playthroughs/starter_runner3d.json`.
//!
//! Units: 1 unit is about a metre, y is up, the duck runs along +z.
//!
//! Make your own with `tools/new_game.sh mygame runner3d`. Without a screen:
//!
//!     cargo run --no-default-features --example starter_runner3d -- --headless --inputs playthroughs/starter_runner3d.json --sheet-events --out out
//!     cargo test --no-default-features --example starter_runner3d

use duckforge::LoadError;
use duckforge::prelude::*;

// ---- Settings: change these first ----
//
// RUN_SPEED, SPEED_UP, JUMP_SPEED and GRAVITY are *tunable*: read with
// `ctx.tune`, so `--set run_speed=10` changes them without rebuilding.

const WIDTH: f32 = 320.0; // the screen, in pixels
const HEIGHT: f32 = 240.0;
const LANE_WIDTH: f32 = 1.5; // units between lanes
const RUN_SPEED: f32 = 8.0; // units per second at the start
const SPEED_UP: f32 = 0.15; // faster by this much every second
const MAX_SPEED: f32 = 14.0;
const JUMP_SPEED: f32 = 6.0; // up, units per second: about 1 unit high
const GRAVITY: f32 = 18.0;
const LANE_SPEED: f32 = 12.0; // how fast the duck slides to its lane
const CRATE_HEIGHT: f32 = 0.8;
const ROW_SPACING: f32 = 8.0; // units between rows of crates and coins
const LIVES: u32 = 3;

/// Where the art and sounds are (made with Duck Quill and Duck Echo).
const ASSETS: &str = "examples/starter_runner3d/assets";

/// Everything loaded from files, once.
#[derive(Clone)]
struct Art {
    duck: SpriteSheet,
    road: Mesh,
    grass: Mesh,
    crate_block: Mesh,
    coin: Mesh,
    sounds: Sounds,
}

/// Duck Echo's sounds (`art/README.md` says how they were made).
#[derive(Clone)]
struct Sounds {
    coin: Wav,
    jump: Wav,
    hit: Wav,
    hum: Wav,
}

impl Art {
    fn load() -> Result<Art, LoadError> {
        let atlas = Tileset::load(format!("{ASSETS}/blocks.json"))?;
        let sounds = Sounds {
            coin: Wav::load(format!("{ASSETS}/coin.wav"))?,
            jump: Wav::load(format!("{ASSETS}/jump.wav"))?,
            hit: Wav::load(format!("{ASSETS}/hit.wav"))?,
            hum: Wav::load(format!("{ASSETS}/hum.wav"))?,
        };
        Ok(Art {
            duck: SpriteSheet::load(format!("{ASSETS}/hero.json"))?,
            road: strip(-1.5 * LANE_WIDTH, 1.5 * LANE_WIDTH)
                .with_texture(atlas.frame("road", 0.0).clone()),
            grass: grass_sides().with_texture(atlas.frame("grass", 0.0).clone()),
            crate_block: Mesh::block(&atlas, ["crate"; 3]),
            coin: Mesh::block(&atlas, ["gold"; 3]),
            sounds,
        })
    }
}

/// How far ahead the ground goes, in units.
const GROUND_LENGTH: f32 = 60.0;

/// A flat strip of ground from `left` to `right`, `GROUND_LENGTH` long
/// from z = 0, facing up. Its texture repeats every `LANE_WIDTH` units.
fn strip(left: f32, right: f32) -> Mesh {
    let mut mesh = Mesh::new();
    add_strip(&mut mesh, left, right);
    mesh
}

fn add_strip(mesh: &mut Mesh, left: f32, right: f32) {
    let corners = [
        vec3(left, 0.0, 0.0),
        vec3(left, 0.0, GROUND_LENGTH),
        vec3(right, 0.0, GROUND_LENGTH),
        vec3(right, 0.0, 0.0),
    ];
    let [a, b, c, d] = corners.map(|p| mesh.add_vertex(p));
    // Texture points beyond 1 repeat the picture: one tile per lane.
    let (across, along) = ((right - left) / LANE_WIDTH, GROUND_LENGTH / LANE_WIDTH);
    let uvs = [
        vec2(0.0, along),
        vec2(0.0, 0.0),
        vec2(across, 0.0),
        vec2(across, along),
    ];
    mesh.add_quad_uv(a, b, c, d, uvs, Color::WHITE);
}

/// Grass to both sides of the road.
fn grass_sides() -> Mesh {
    let mut mesh = Mesh::new();
    add_strip(&mut mesh, -15.0 * LANE_WIDTH, -1.5 * LANE_WIDTH);
    add_strip(&mut mesh, 1.5 * LANE_WIDTH, 15.0 * LANE_WIDTH);
    mesh
}

/// What the game is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Playing,
    GameOver,
}

/// A crate or a coin on the road.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Thing {
    lane: i32,
    z: f32,
}

#[derive(Clone)]
struct Runner {
    state: State,
    art: Art,
    /// The lane the duck is going to: 0 (left), 1 or 2 (right).
    lane: i32,
    /// Where the duck is: across (x), up (its feet), along the road (z).
    x: f32,
    y: f32,
    z: f32,
    up_speed: f32,
    speed: f32,
    crates: Vec<Thing>,
    coins: Vec<Thing>,
    /// Where the next row of crates and coins goes.
    next_row: f32,
    collected: u32,
    lives: u32,
    /// Seconds left of blinking after a bump, when crates can't hurt.
    safe_for: f32,
    /// Seconds of play, for animations.
    time: f32,
    music: Option<LoopId>,
}

/// The x of a lane's middle.
fn lane_x(lane: i32) -> f32 {
    (lane - 1) as f32 * LANE_WIDTH
}

impl Runner {
    fn new(art: Art) -> Runner {
        Runner {
            state: State::Playing,
            art,
            lane: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            up_speed: 0.0,
            speed: RUN_SPEED,
            crates: Vec::new(),
            coins: Vec::new(),
            next_row: 20.0,
            collected: 0,
            lives: LIVES,
            safe_for: 0.0,
            time: 0.0,
            music: None,
        }
    }

    fn on_ground(&self) -> bool {
        self.y <= 0.0
    }

    /// Adds rows of crates and coins until they reach 50 units ahead. Each
    /// row has one or two crates (never three: there's always a way
    /// through) and a line of coins in a free lane.
    fn fill_road(&mut self, ctx: &mut Context) {
        while self.next_row < self.z + 50.0 {
            let z = self.next_row;
            let free = ctx.rng.range_i32(0, 3);
            for lane in 0..3 {
                let busy = lane != free && (ctx.rng.chance(0.6) || lane == (free + 1) % 3);
                if busy {
                    self.crates.push(Thing { lane, z });
                }
            }
            if ctx.rng.chance(0.7) {
                for i in 0..3 {
                    self.coins.push(Thing {
                        lane: free,
                        z: z - 2.0 + i as f32 * 1.2,
                    });
                }
            }
            self.next_row += ROW_SPACING;
        }
    }

    /// Plays one of Duck Echo's sounds.
    fn sound(&self, ctx: &mut Context, pick: fn(&Sounds) -> &Wav) {
        ctx.play_wav(pick(&self.art.sounds));
    }

    fn play(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let run_speed = ctx.tune("run_speed", RUN_SPEED);
        let speed_up = ctx.tune("speed_up", SPEED_UP);
        let jump_speed = ctx.tune("jump_speed", JUMP_SPEED);
        let gravity = ctx.tune("gravity", GRAVITY);
        self.time += dt;

        // A looping hum, from the first frame (and again after a restart).
        if self.music.is_none() {
            self.music = Some(ctx.play_loop(&self.art.sounds.hum, 0.3));
        }

        // Change lane on a press, then slide there.
        let left = [Key::Left, Key::A]
            .iter()
            .any(|&k| ctx.input.was_pressed(k));
        let right = [Key::Right, Key::D]
            .iter()
            .any(|&k| ctx.input.was_pressed(k));
        self.lane = (self.lane - left as i32 + right as i32).clamp(0, 2);
        self.x = approach(self.x, lane_x(self.lane), LANE_SPEED * dt);

        // Jump, only from the ground.
        let jump = [Key::Space, Key::Up, Key::W]
            .iter()
            .any(|&k| ctx.input.was_pressed(k));
        if jump && self.on_ground() {
            self.up_speed = jump_speed;
            self.sound(ctx, |s| &s.jump);
        }
        self.up_speed -= gravity * dt;
        self.y = (self.y + self.up_speed * dt).max(0.0);
        if self.on_ground() {
            self.up_speed = 0.0;
        }

        // Run, a little faster every second.
        self.speed = (run_speed + speed_up * self.time).min(MAX_SPEED);
        self.z += self.speed * dt;
        self.fill_road(ctx);

        // Bumping into a crate costs a life (then a second of blinking).
        self.safe_for = (self.safe_for - dt).max(0.0);
        let (x, z, feet) = (self.x, self.z, self.y);
        let touching = |thing: &Thing, depth: f32| {
            (lane_x(thing.lane) - x).abs() < 0.8 && (thing.z - z).abs() < depth
        };
        let hit = self
            .crates
            .iter()
            .position(|c| touching(c, 0.75) && feet < CRATE_HEIGHT);
        if let Some(index) = hit {
            if self.safe_for == 0.0 {
                self.crates.remove(index);
                self.lives -= 1;
                self.safe_for = 1.0;
                self.sound(ctx, |s| &s.hit);
                if self.lives == 0 {
                    self.state = State::GameOver;
                    if let Some(music) = self.music.take() {
                        ctx.stop_loop(music);
                    }
                }
            }
        }
        // Coins: keep the ones the duck isn't touching.
        let before = self.coins.len();
        self.coins.retain(|c| !(touching(c, 0.6) && feet < 1.0));
        for _ in self.coins.len()..before {
            self.collected += 1;
            self.sound(ctx, |s| &s.coin);
        }
        // Forget what's behind the camera.
        self.crates.retain(|c| c.z > z - 6.0);
        self.coins.retain(|c| c.z > z - 6.0);
    }

    /// The camera: behind the duck and above, looking a little down the road.
    fn camera(&self) -> Camera3D {
        let follow = self.x * 0.5;
        Camera3D::looking_at(
            vec3(follow, 2.6, self.z - 5.0),
            vec3(follow, 0.6, self.z + 6.0),
        )
    }
}

impl Game for Runner {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        match self.state {
            State::Playing => self.play(ctx),
            State::GameOver => {
                ctx.idle_ok(); // waiting on purpose (for --fuzz)
                let again = [Key::Space, Key::Enter]
                    .iter()
                    .any(|&k| ctx.input.was_pressed(k));
                if again {
                    *self = Runner::new(self.art.clone());
                }
            }
        }

        // Rules that must always hold.
        ctx.check("in a lane", self.x.abs() <= LANE_WIDTH + 0.001);
        ctx.check("lives 0 to 3", self.lives <= LIVES);
        ctx.check("never below the road", self.y >= 0.0);

        ctx.report("state", format!("{:?}", self.state));
        ctx.report("distance", self.z.floor());
        ctx.report("coins", self.collected);
        ctx.report("lives", self.lives);
        ctx.report("lane", self.lane);
        ctx.report("in_air", !self.on_ground());
    }

    fn draw(&self, canvas: &mut Canvas) {
        let art = &self.art;
        let camera = self.camera();
        // The sky: 2D, drawn first, so the 3D goes over it.
        canvas.clear(Color::from_hex(0x87ceeb));
        canvas.fill_rect(Rect::new(0.0, 0.0, WIDTH, 40.0), Color::from_hex(0x6cb4e4));
        canvas.set_lighting(Lighting {
            sun_direction: vec3(-0.5, 1.0, -0.3),
            sun_color: Color::from_hex(0xfff0d8),
            sun_strength: 1.0,
            ambient: 0.55,
        });

        // The ground repeats every lane-width, so drawing it a little
        // behind the duck, at a whole number of repeats, looks endless.
        let ground_z = (self.z / LANE_WIDTH).floor() * LANE_WIDTH - 6.0;
        let ground = Transform::at(vec3(0.0, 0.0, ground_z));
        canvas.draw_mesh(&art.road, &ground, &camera);
        canvas.draw_mesh(&art.grass, &ground, &camera);

        for c in &self.crates {
            let at = vec3(lane_x(c.lane), CRATE_HEIGHT / 2.0, c.z);
            let size = vec3(1.0, CRATE_HEIGHT, 1.0);
            canvas.draw_mesh(&art.crate_block, &Transform::at(at).scaled(size), &camera);
        }
        for c in &self.coins {
            let spin = Transform::at(vec3(lane_x(c.lane), 0.5, c.z))
                .rotated(vec3(0.0, self.time * 3.0, 0.0))
                .sized(0.35);
            canvas.draw_mesh(&art.coin, &spin, &camera);
        }

        // The duck: a billboard, blinking after a bump.
        let blink = self.safe_for > 0.0 && (self.time * 10.0) as i32 % 2 == 0;
        if !blink {
            let animation = match (self.on_ground(), self.state) {
                (_, State::GameOver) => "idle",
                (true, _) => "run",
                (false, _) if self.up_speed > 0.0 => "jump",
                (false, _) => "fall",
            };
            let frame = art.duck.animation(animation).frame_at(self.time);
            let look = Billboard {
                pivot: Some(art.duck.pivot()),
                ..Default::default()
            };
            canvas.draw_billboard_with(frame, vec3(self.x, self.y, self.z), 1.0, &camera, look);
        }

        // Labels, in screen pixels, for tools that can't see the picture.
        if let Some(rect) = screen_box(&camera, canvas, vec3(self.x, self.y, self.z), 0.6, 1.0) {
            canvas.label("player", rect);
        }
        // Only the next crate in each lane: further ones overlap them on
        // screen (that's perspective, not crates on top of each other).
        for lane in 0..3 {
            let next = self
                .crates
                .iter()
                .filter(|c| c.lane == lane && c.z > self.z)
                .min_by(|a, b| a.z.total_cmp(&b.z));
            if let Some(c) = next {
                let feet = vec3(lane_x(c.lane), 0.0, c.z - 0.5);
                if let Some(rect) = screen_box(&camera, canvas, feet, 1.0, CRATE_HEIGHT) {
                    canvas.label("crate", rect);
                }
            }
        }

        // The HUD, over the 3D.
        canvas.draw_text(
            &format!("COINS {}", self.collected),
            vec2(4.0, 4.0),
            1,
            Color::WHITE,
        );
        let distance = format!("{} M", self.z.floor());
        let x = WIDTH - 4.0 - Canvas::text_width(&distance, 1);
        canvas.draw_text(&distance, vec2(x, 4.0), 1, Color::WHITE);
        for life in 0..self.lives {
            let heart = Rect::new(4.0 + life as f32 * 8.0, 14.0, 6.0, 6.0);
            canvas.fill_rect(heart, Color::from_hex(0xe04848));
        }
        if self.state == State::GameOver {
            canvas.fill_rect(
                Rect::new(0.0, 90.0, WIDTH, 60.0),
                Color::BLACK.with_alpha(170),
            );
            canvas.draw_text_centered("GAME OVER", 100.0, 3, Color::WHITE);
            let line = format!(
                "{} coins, {} m: Space to run again",
                self.collected,
                self.z.floor()
            );
            canvas.draw_text_centered(&line, 130.0, 1, Color::YELLOW);
        }
    }
}

/// The screen rectangle of something `width` wide and `height` tall
/// standing at `feet`, or `None` if it's behind the camera.
fn screen_box(
    camera: &Camera3D,
    canvas: &Canvas,
    feet: Vec3,
    width: f32,
    height: f32,
) -> Option<Rect> {
    let (bottom, _) = camera.project(feet, canvas)?;
    let (top, _) = camera.project(feet + vec3(0.0, height, 0.0), canvas)?;
    let (side, _) = camera.project(feet + vec3(width / 2.0, 0.0, 0.0), canvas)?;
    let half = (side.x - bottom.x).abs();
    Some(Rect::new(
        bottom.x - half,
        top.y,
        half * 2.0,
        bottom.y - top.y,
    ))
}

fn config() -> Config {
    Config {
        title: String::from("3D runner starter - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let art = Art::load()?;
    duckforge::run(config(), Runner::new(art))
}

// Play tests: `cargo test --example starter_runner3d`. The bot that records
// playthroughs/starter_runner3d.json: `cargo test --example starter_runner3d -- --ignored`.
#[cfg(test)]
mod tests;

#[cfg(test)]
mod bot;

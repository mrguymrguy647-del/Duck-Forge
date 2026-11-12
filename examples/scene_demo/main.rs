//! A scene and a little code: the first game made with a scene file.
//!
//!     cargo run --example scene_demo
//!
//! Arrow keys (or A/D) run, Up/Space/W jumps, Escape quits.
//!
//! What's in the game is in `scenes/pond.json`, one object per line: the
//! sky and the hills, the ground (a tile map: `level.txt`, as in the
//! platformer starter), a sign, two crates to climb, coins, a bush in
//! front of everything, and the duck. Change a line while the game runs
//! (move the sign, add a coin, make a crate bigger) and it shows at once:
//! in a window, or frozen in Duck Forge Studio.
//!
//! What the game *does* is here, and it's short, because the scene does
//! the drawing, the finding and the bumping into things:
//!
//! - `scene.find("duck")` gives the duck, whichever line it's on;
//! - `scene.move_and_slide(duck, ...)` moves it, stopping at the ground
//!   and at the crates (they're `"solid": true` in the file);
//! - `scene.touching(duck, "coin")` gives the coins it's on, painted on
//!   the tile map or placed by hand: both are objects called "coin";
//! - `scene.reload(ctx, file)` takes a changed file in mid-play.
//!
//! Without a screen (or to test):
//!
//!     cargo run --no-default-features --example scene_demo -- --headless --inputs playthroughs/scene_demo.json --out out
//!     cargo test --no-default-features --example scene_demo

use std::path::Path;

use duckforge::LoadError;
use duckforge::prelude::*;

const WIDTH: f32 = 320.0; // the screen, in pixels
const HEIGHT: f32 = 240.0;
const GRAVITY: f32 = 900.0; // pixels per second, per second
const RUN_SPEED: f32 = 110.0; // pixels per second
const JUMP_SPEED: f32 = 330.0; // about 60 pixels high
const FALL_LIMIT: f32 = 400.0; // the fastest it falls

/// The scene: every object of the game, one per line.
const SCENE: &str = "examples/scene_demo/scenes/pond.json";

struct SceneDemo {
    scene: Scene,
    /// How fast the duck is going. (Where it is, is in the scene.)
    velocity: Vec2,
    on_ground: bool,
    coins: u32,
    won: bool,
}

impl SceneDemo {
    fn new() -> Result<SceneDemo, LoadError> {
        SceneDemo::with_scene(SCENE)
    }

    /// The game on a scene file (the tests play copies of the real one).
    fn with_scene(file: impl AsRef<Path>) -> Result<SceneDemo, LoadError> {
        Ok(SceneDemo {
            scene: Scene::load(file)?,
            velocity: Vec2::ZERO,
            on_ground: false,
            coins: 0,
            won: false,
        })
    }

    /// One update of the duck: run, jump, fall, bump, collect.
    fn play(&mut self, duck: Id, ctx: &mut Context) {
        let dt = ctx.dt();
        let run_speed = ctx.tune("run_speed", RUN_SPEED);
        let jump_speed = ctx.tune("jump_speed", JUMP_SPEED);
        let gravity = ctx.tune("gravity", GRAVITY);

        // Run: -1, 0 or 1 from the keys, then ease the speed towards it.
        let run = (ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D))
            .clamp(-1.0, 1.0);
        self.velocity.x = approach(self.velocity.x, run * run_speed, run_speed * 8.0 * dt);
        if run != 0.0 {
            self.scene[duck].flip = run < 0.0; // face the way it runs
        }
        // Jump, only from something to stand on.
        let jump = [Key::Up, Key::Space, Key::W]
            .iter()
            .any(|&key| ctx.input.was_pressed(key));
        if jump && self.on_ground {
            self.velocity.y = -jump_speed;
            ctx.play(Sound::jump());
        }
        self.velocity.y = (self.velocity.y + gravity * dt).min(FALL_LIMIT);

        // The scene moves the duck, and says what it bumped into: the
        // ground (the tile map) or a crate (a solid object).
        let touched = self.scene.move_and_slide(duck, self.velocity * dt);
        self.on_ground = touched.ground;
        if touched.ground || touched.ceiling {
            self.velocity.y = 0.0;
        }
        if touched.wall_left || touched.wall_right {
            self.velocity.x = 0.0;
        }

        // The scene's ends are walls, and under it is the way back to
        // where the duck starts.
        let size = self.scene.size().unwrap_or(vec2(WIDTH, HEIGHT));
        let at = self.scene[duck].at;
        self.scene[duck].at.x = at.x.clamp(8.0, size.x - 8.0);
        if at.y > size.y + 64.0 {
            let start = self
                .scene
                .starts_as(duck)
                .map_or(vec2(40.0, 0.0), |duck| duck.at);
            self.scene[duck].at = start;
            self.velocity = Vec2::ZERO;
            ctx.play(Sound::hit());
        }

        // Which of its animations the duck shows.
        let animation = if !self.on_ground && self.velocity.y < 0.0 {
            "jump"
        } else if !self.on_ground {
            "fall"
        } else if run != 0.0 {
            "run"
        } else {
            "idle"
        };
        self.scene[duck].animation = Some(animation.to_string());

        // Coins: the ones painted on the tile map and the ones placed by
        // hand are all objects called "coin".
        for coin in self.scene.touching(duck, "coin") {
            self.scene.remove(coin);
            self.coins += 1;
            ctx.play(Sound::coin());
        }
        if !self.scene.touching(duck, "flag").is_empty() {
            self.won = true;
            ctx.play(Sound::explosion());
        }
    }
}

impl Game for SceneDemo {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        self.scene.update(ctx); // the animations' clock, and the camera

        // The duck is found by name every update, so the game carries on
        // if its line is moved, changed, or even taken out of the file.
        match self.scene.find("duck") {
            Some(duck) if !self.won => self.play(duck, ctx),
            // Won, or no duck: the game waits on purpose (tell --fuzz).
            _ => ctx.idle_ok(),
        }

        // The camera glides after the duck, and stops at the scene's ends.
        let size = self.scene.size().unwrap_or(vec2(WIDTH, HEIGHT));
        if let Some(duck) = self.scene.find("duck") {
            let target = self.scene[duck].at - vec2(0.0, 8.0);
            self.scene.camera.follow(target, 0.15, ctx.dt());
            ctx.debug_rect(self.scene.rect(duck));
            ctx.report("x", self.scene[duck].at.x.round());
        }
        let camera = &mut self.scene.camera;
        camera.position.x = camera
            .position
            .x
            .clamp(WIDTH / 2.0, (size.x - WIDTH / 2.0).max(WIDTH / 2.0));
        camera.position.y = HEIGHT / 2.0;

        // A rule that must always hold: the summary, the play tests and
        // --fuzz say so if the scene file ever has a mistake in it.
        ctx.check(
            "the scene file has no mistakes",
            self.scene.problems().is_empty(),
        );
        // For tests and AI agents (log.json).
        ctx.report("coins", self.coins);
        ctx.report("coins_left", self.scene.all("coin").len());
        ctx.report("won", self.won);
        ctx.report("on_ground", self.on_ground);
        ctx.report("objects", self.scene.len());
    }

    /// A file changed (the scene, its level, a picture): the scene takes
    /// it, and the game carries on from where it is.
    fn reload(&mut self, ctx: &mut Context, file: &Path) -> bool {
        self.scene.reload(ctx, file)
    }

    fn draw(&self, canvas: &mut Canvas) {
        // Everything in the scene file, layer by layer...
        self.scene.draw(canvas);
        // ...and what's the code's own, on top: the count next to the
        // scene's coin icon, and the last words.
        canvas.draw_text(
            &format!("x {}", self.coins),
            vec2(24.0, 9.0),
            1,
            Color::WHITE,
        );
        if self.won {
            canvas.draw_text_centered("You made it!", 100.0, 2, Color::WHITE);
        }
    }
}

fn config() -> Config {
    Config {
        title: String::from("Scene Demo - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), SceneDemo::new()?)
}

// Play tests: `cargo test --example scene_demo`. The bot that records
// playthroughs/scene_demo.json: `cargo test --example scene_demo -- --ignored`.
#[cfg(test)]
mod tests;

#[cfg(test)]
mod bot;

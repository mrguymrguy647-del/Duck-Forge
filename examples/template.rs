//! A starter game to copy when you begin a new one.
//!
//!     cargo run --example template
//!
//! Arrow keys (or A/D) run, Up/Space/W jumps, P pauses, Escape quits.
//! F1 shows the debug overlay, F10 records what you do.
//!
//! It has the parts most 2D games need, each small enough to change:
//! a sprite drawn with letters, a walk animation, gravity and platforms,
//! a camera that follows the player, coins with a sound, a jump sound,
//! a see-through pause screen, reports for tests, and play tests at the
//! bottom. To start your own game: copy this file to `examples/mygame.rs`,
//! run it, then change one thing at a time.
//!
//! Without a screen (or to test):
//!
//!     cargo run --no-default-features --example template -- --headless --sheet --out out
//!     cargo test --no-default-features --example template

use duckforge::prelude::*;

// ---- Settings: change these first ----
//
// GRAVITY, RUN_SPEED and JUMP_SPEED are *tunable*: the game reads them with
// `ctx.tune`, so examples/template.tune.json (or `--set jump_speed=400`)
// can change them without rebuilding, even while the game runs.

const WIDTH: f32 = 320.0; // the screen, in pixels
const HEIGHT: f32 = 240.0;
const WORLD_WIDTH: f32 = 960.0; // the level is three screens wide
const GRAVITY: f32 = 900.0; // pixels per second, per second (2D is in pixels)
const RUN_SPEED: f32 = 110.0; // pixels per second
const JUMP_SPEED: f32 = 330.0;
const PLAYER_SIZE: Vec2 = vec2(12.0, 14.0); // the hitbox: the sprite, 2x scaled

/// The duck, drawn with one letter per pixel ('.' is see-through). Two
/// frames side by side: feet apart, then feet together.
const DUCK: [&str; 7] = [
    "..yy....yy..",
    ".ykyo..ykyo.",
    "yyyy..yyyy..",
    "yyyyy.yyyyy.",
    "yyyy..yyyy..",
    ".yy....yy...",
    "o..o...oo...",
];

/// What the game is doing. An `enum` means it can only be one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Playing,
    Paused,
    Won,
}

struct Template {
    state: State,
    /// The top-left corner of the player's hitbox, in world pixels.
    pos: Vec2,
    vel: Vec2,
    on_ground: bool,
    facing_left: bool,
    /// Seconds spent walking, to pick the animation frame.
    walk_time: f32,
    walk: Animation,
    platforms: Vec<Rect>,
    coins: Vec<Vec2>,
    collected: u32,
    camera: Camera2D,
}

impl Template {
    fn new() -> Template {
        let palette = [
            ('y', Color::YELLOW),
            ('o', Color::ORANGE),
            ('k', Color::BLACK),
        ];
        let sheet = Sprite::from_ascii(&DUCK, &palette);
        let mut platforms = vec![Rect::new(0.0, 220.0, WORLD_WIDTH, 20.0)]; // the ground
        for i in 0..8 {
            let x = 120.0 + i as f32 * 100.0;
            let y = if i % 2 == 0 { 180.0 } else { 150.0 };
            platforms.push(Rect::new(x, y, 48.0, 8.0));
        }
        // One coin above each platform.
        let coins = platforms[1..]
            .iter()
            .map(|p| vec2(p.center().x, p.y - 14.0))
            .collect();
        let pos = vec2(40.0, 220.0 - PLAYER_SIZE.y);
        let mut camera = Camera2D::new(WIDTH, HEIGHT);
        camera.position = vec2(WIDTH / 2.0, HEIGHT / 2.0);
        Template {
            state: State::Playing,
            pos,
            vel: Vec2::ZERO,
            on_ground: true,
            facing_left: false,
            walk_time: 0.0,
            walk: Animation::new(sheet.frames(6, 7), 8.0),
            platforms,
            coins,
            collected: 0,
            camera,
        }
    }

    fn hitbox(&self) -> Rect {
        Rect::new(self.pos.x, self.pos.y, PLAYER_SIZE.x, PLAYER_SIZE.y)
    }

    /// Moves the player, one axis at a time, stopping at platforms.
    fn move_player(&mut self, dt: f32) {
        // Across first: if we bump into a platform, stop at its side.
        self.pos.x = (self.pos.x + self.vel.x * dt).clamp(0.0, WORLD_WIDTH - PLAYER_SIZE.x);
        for platform in &self.platforms {
            if self.hitbox().overlaps(platform) {
                self.pos.x = if self.vel.x > 0.0 {
                    platform.left() - PLAYER_SIZE.x
                } else {
                    platform.right()
                };
            }
        }
        // Then up and down: land on top, or bump our head underneath.
        self.pos.y += self.vel.y * dt;
        self.on_ground = false;
        for platform in &self.platforms {
            if self.hitbox().overlaps(platform) {
                if self.vel.y > 0.0 {
                    self.pos.y = platform.top() - PLAYER_SIZE.y;
                    self.on_ground = true;
                } else {
                    self.pos.y = platform.bottom();
                }
                self.vel.y = 0.0;
            }
        }
    }
}

impl Game for Template {
    fn update(&mut self, ctx: &mut Context) {
        // The tunable numbers, read at the top of every update: then the
        // tune file always lists all of them (--write-tune), and edits to it
        // apply straight away while the game runs.
        let run_speed = ctx.tune("run_speed", RUN_SPEED);
        let jump_speed = ctx.tune("jump_speed", JUMP_SPEED);
        let gravity = ctx.tune("gravity", GRAVITY);

        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.was_pressed(Key::P) && self.state != State::Won {
            self.state = if self.state == State::Paused {
                State::Playing
            } else {
                State::Paused
            };
        }

        if self.state == State::Playing {
            let dt = ctx.dt();
            // Run: -1, 0 or 1 from the keys, then ease the speed towards it.
            let run = ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D);
            let target = run.clamp(-1.0, 1.0) * run_speed;
            self.vel.x = approach(self.vel.x, target, run_speed * 8.0 * dt);
            if run != 0.0 {
                self.facing_left = run < 0.0;
                self.walk_time += dt;
            }
            // Jump, only from the ground.
            let jump = [Key::Up, Key::Space, Key::W]
                .iter()
                .any(|&k| ctx.input.was_pressed(k));
            if jump && self.on_ground {
                self.vel.y = -jump_speed;
                ctx.play(Sound::jump());
            }
            self.vel.y = (self.vel.y + gravity * dt).min(400.0); // falling speed limit
            self.move_player(dt);

            // Coins: keep the ones the player isn't touching.
            let player = self.hitbox();
            let before = self.coins.len();
            self.coins
                .retain(|&coin| !circle_vs_rect(coin, 4.0, player));
            for _ in self.coins.len()..before {
                self.collected += 1;
                ctx.play(Sound::coin());
            }
            if self.coins.is_empty() {
                self.state = State::Won;
            }

            // The camera glides after the player, but never shows past the level's ends.
            let center = player.center();
            self.camera.follow(center, 0.15, dt);
            self.camera.position.x = self
                .camera
                .position
                .x
                .clamp(WIDTH / 2.0, WORLD_WIDTH - WIDTH / 2.0);
            self.camera.position.y = HEIGHT / 2.0;
        }
        self.camera.update(ctx);

        // For the debug overlay (F1) and for tests and AI agents (log.json).
        ctx.debug_rect(self.hitbox());
        ctx.report("state", format!("{:?}", self.state));
        ctx.report("coins", self.collected);
        ctx.report("x", self.pos.x.round());
        ctx.report("on_ground", self.on_ground);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::from_hex(0x87ceeb)); // sky

        // The world: everything in here moves with the camera.
        canvas.push_camera(&self.camera);
        for (i, platform) in self.platforms.iter().enumerate() {
            let color = if i == 0 {
                Color::from_hex(0x4a8c3a)
            } else {
                Color::from_hex(0x8b5a2b)
            };
            canvas.fill_rect(*platform, color);
        }
        for &coin in &self.coins {
            canvas.fill_circle(coin, 4.0, Color::from_hex(0xffd700));
        }
        let frame = if self.on_ground {
            self.walk.frame_at(self.walk_time)
        } else {
            &self.walk.frames[1]
        };
        let blit = Blit {
            flip_x: self.facing_left,
            scale: 2.0,
            ..Default::default()
        };
        canvas.blit_sprite(frame, self.pos, blit);
        canvas.pop_camera();

        // The screen: the score stays put.
        let total = self.collected as usize + self.coins.len();
        canvas.draw_text(
            &format!("Coins {}/{total}", self.collected),
            vec2(4.0, 4.0),
            1,
            Color::BLACK,
        );

        match self.state {
            State::Playing => {}
            State::Paused => {
                // A see-through dark layer over the game, then the words on top.
                canvas.fill_rect(
                    Rect::new(0.0, 0.0, WIDTH, HEIGHT),
                    Color::BLACK.with_alpha(150),
                );
                canvas.draw_text_centered("PAUSED", 100.0, 3, Color::WHITE);
                canvas.draw_text_centered("press P to carry on", 130.0, 1, Color::GRAY);
            }
            State::Won => {
                canvas.fill_rect(
                    Rect::new(0.0, 90.0, WIDTH, 50.0),
                    Color::from_hex(0x203040).with_alpha(200),
                );
                canvas.draw_text_centered("ALL COINS!", 100.0, 3, Color::YELLOW);
            }
        }
    }
}

fn config() -> Config {
    Config {
        title: String::from("Template - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Template::new())
}

// ---- Play tests: `cargo test --example template` ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_right_moves_the_player_and_keeps_them_on_the_ground() {
        let played = PlayTest::new(config(), Template::new())
            .script(r#"{"events": [{"frame": 1, "press": "Right"}, {"frame": 61, "release": "Right"}]}"#)
            .frames(90)
            .run();
        assert!(played.number("x") > 120.0, "about a second of running");
        assert!(played.flag("on_ground"));
        assert_eq!(played.text("state"), "Playing");
    }

    #[test]
    fn jumping_plays_a_sound_and_lands_again() {
        let played = PlayTest::new(config(), Template::new())
            .script(r#"{"events": [{"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"}]}"#)
            .frames(120)
            .run();
        played.assert_sound("jump", 10, 0);
        let ground: Vec<bool> = played
            .history("on_ground")
            .iter()
            .filter_map(|(_, value)| value.as_bool())
            .collect();
        assert_eq!(ground, [true, false, true], "took off, then landed");
    }

    #[test]
    fn pausing_freezes_the_game() {
        let played = PlayTest::new(config(), Template::new())
            .script(
                r#"{"events": [{"frame": 1, "press": "Right"}, {"frame": 30, "press": "P"},
                               {"frame": 31, "release": "P"}]}"#,
            )
            .frames(120)
            .run();
        assert_eq!(played.text("state"), "Paused");
        let x_at_pause = played
            .reported_at(30, "x")
            .and_then(|v| v.as_number())
            .unwrap();
        assert_eq!(played.number("x"), x_at_pause, "nothing moves while paused");
    }
}

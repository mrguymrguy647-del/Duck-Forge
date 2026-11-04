//! A shooter starter: fly a ship, hold fire, clear wave after wave of
//! enemies that sway, dive at you and shoot back.
//!
//!     cargo run --example starter_shooter
//!
//! Arrow keys (or WASD) fly, Space (or Z) fires, P pauses, Escape quits.
//! F1 shows the debug overlay, F10 records what you do.
//!
//! The parts, each small enough to change: sprites drawn with letters,
//! fast bullets that can't pass through enemies (`swept_aabb`), waves that
//! get harder, enemies that dive and shoot, particle explosions made of
//! small squares, screen shake (the camera), a scrolling star field,
//! lives with a moment of safety after a hit, sounds, reports, rules
//! (`ctx.check`) and play tests. Copy this file to `examples/mygame.rs` to
//! start your own game.
//!
//! Without a screen:
//!
//!     cargo run --no-default-features --example starter_shooter -- --headless --sheet --out out
//!     tools/verify.sh starter_shooter

use duckforge::prelude::*;

// ---- Settings: change these first ----
//
// The ones read with `ctx.tune` (ship_speed, bullet_speed, fire_delay,
// enemy_fire) can be changed in examples/starter_shooter.tune.json or with
// `--set bullet_speed=900`, without rebuilding.

const WIDTH: f32 = 240.0;
const HEIGHT: f32 = 320.0;
const SHIP_SPEED: f32 = 150.0; // pixels per second
const BULLET_SPEED: f32 = 480.0; // 8 pixels a frame: more than a bullet's length
const FIRE_DELAY: f32 = 0.12; // seconds between shots while fire is held
const ENEMY_FIRE: f32 = 0.06; // shots per second, per enemy, on wave 1
const ENEMY_SHOT_SPEED: f32 = 110.0;
const START_LIVES: u32 = 3;
const SAFE_TIME: f32 = 2.0; // seconds you can't be hit again after a hit
const BULLET: Vec2 = vec2(2.0, 6.0);
const ENEMY_SIZE: Vec2 = vec2(14.0, 10.0);

/// The ship, 7 x 7, drawn at twice the size ('.' is see-through).
const SHIP: [&str; 7] = [
    "...w...", "...w...", "..wbw..", ".wwbww.", "wwwwwww", "w.r.r.w", "..r.r..",
];

/// An enemy, two 7 x 5 frames side by side (wings up, wings down).
const BUG: [&str; 5] = [
    "g.....gg.g.g.g",
    ".ggggg..ggggg.",
    "gykgykggykgykg",
    ".ggggg.g.ggg.g",
    "g.....g.g...g.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Playing,
    Paused,
    GameOver,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Flight {
    /// Flying down from above the screen to its place in the formation.
    Entering,
    /// In the formation, swaying with the others.
    InPlace,
    /// Diving at the ship, at this velocity.
    Diving(Vec2),
}

struct Enemy {
    pos: Vec2, // top-left corner
    home: Vec2,
    hp: u32,
    flight: Flight,
}

impl Enemy {
    fn body(&self) -> Rect {
        Rect::new(self.pos.x, self.pos.y, ENEMY_SIZE.x, ENEMY_SIZE.y)
    }
}

/// A small square flying out of an explosion, fading as it goes.
struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: f32,
    color: Color,
}

struct Shooter {
    state: State,
    ship: Vec2, // the ship's center
    lives: u32,
    safe_time: f32,
    reload: f32,
    bullets: Vec<Vec2>, // top-left corners, all flying up
    shots: Vec<Vec2>,   // the enemies' bullets, flying down
    enemies: Vec<Enemy>,
    wave: u32,
    next_wave_in: f32,
    score: u32,
    time: f32,
    particles: Vec<Particle>,
    stars: Vec<(Vec2, f32)>, // position and speed
    /// Effects (particles) take their random numbers from here, not from
    /// `ctx.rng`, so adding an explosion never changes what the enemies do.
    fx_rng: Rng,
    ship_sprite: Sprite,
    bug: Animation,
    camera: Camera2D,
}

impl Shooter {
    fn new() -> Shooter {
        let mut stars_rng = Rng::new(7);
        let stars = (0..50)
            .map(|_| {
                let pos = vec2(stars_rng.range(0.0, WIDTH), stars_rng.range(0.0, HEIGHT));
                (pos, stars_rng.range(10.0, 50.0))
            })
            .collect();
        let ship_sprite = Sprite::from_ascii(
            &SHIP,
            &[
                ('w', Color::from_hex(0xd0e0ff)),
                ('b', Color::from_hex(0x4080ff)),
                ('r', Color::from_hex(0xff8030)),
            ],
        );
        let bug = Sprite::from_ascii(
            &BUG,
            &[
                ('g', Color::from_hex(0x60d060)),
                ('y', Color::YELLOW),
                ('k', Color::BLACK),
            ],
        );
        let mut camera = Camera2D::new(WIDTH, HEIGHT);
        camera.position = vec2(WIDTH / 2.0, HEIGHT / 2.0);
        Shooter {
            state: State::Ready,
            ship: vec2(WIDTH / 2.0, HEIGHT - 30.0),
            lives: START_LIVES,
            safe_time: 0.0,
            reload: 0.0,
            bullets: Vec::new(),
            shots: Vec::new(),
            enemies: Vec::new(),
            wave: 0,
            next_wave_in: 0.5,
            score: 0,
            time: 0.0,
            particles: Vec::new(),
            stars,
            fx_rng: Rng::new(1),
            ship_sprite,
            bug: Animation::new(bug.frames(7, 5), 4.0),
            camera,
        }
    }

    /// The part of the ship that counts for hits: smaller than the picture,
    /// which feels fair.
    fn ship_hitbox(&self) -> Rect {
        Rect::from_center(self.ship, 8.0, 8.0)
    }

    /// Wave 1 has 6 enemies; each wave has 2 more (up to 18), and every
    /// third wave they take another hit.
    fn start_wave(&mut self, ctx: &mut Context) {
        self.wave += 1;
        let count = (4 + 2 * self.wave).min(18);
        let hp = 1 + (self.wave - 1) / 3;
        for i in 0..count {
            let (col, row) = (i % 6, i / 6);
            let home = vec2(22.0 + col as f32 * 34.0, 40.0 + row as f32 * 24.0);
            self.enemies.push(Enemy {
                pos: home - vec2(0.0, 160.0 + row as f32 * 20.0),
                home,
                hp,
                flight: Flight::Entering,
            });
        }
        ctx.beep(660.0, 120.0);
    }

    fn explode(&mut self, at: Vec2, color: Color, count: usize) {
        for _ in 0..count {
            let angle = self.fx_rng.range(0.0, std::f32::consts::TAU);
            let speed = self.fx_rng.range(30.0, 110.0);
            self.particles.push(Particle {
                pos: at,
                vel: vec2(angle.cos(), angle.sin()) * speed,
                life: self.fx_rng.range(0.3, 0.8),
                color,
            });
        }
    }

    fn fly_ship(&mut self, ctx: &mut Context, speed: f32) {
        let input = &ctx.input;
        let mut dir = vec2(
            (input.axis(Key::Left, Key::Right) + input.axis(Key::A, Key::D)).clamp(-1.0, 1.0),
            (input.axis(Key::Up, Key::Down) + input.axis(Key::W, Key::S)).clamp(-1.0, 1.0),
        );
        if dir.length() > 0.0 {
            dir = dir.normalized();
        }
        self.ship += dir * speed * ctx.dt();
        self.ship.x = self.ship.x.clamp(8.0, WIDTH - 8.0);
        self.ship.y = self.ship.y.clamp(HEIGHT / 2.0, HEIGHT - 10.0);
    }

    /// Moves every bullet, and checks it against every enemy along the
    /// whole way it moved this frame. A plain overlap test would let a
    /// bullet jump over an enemy thinner than one frame's movement.
    fn move_bullets(&mut self, ctx: &mut Context, bullet_speed: f32) {
        let step = vec2(0.0, -bullet_speed * ctx.dt());
        let mut i = 0;
        while i < self.bullets.len() {
            let bullet = Rect::new(self.bullets[i].x, self.bullets[i].y, BULLET.x, BULLET.y);
            // The first enemy this bullet reaches, if any.
            let hit = self
                .enemies
                .iter()
                .enumerate()
                .filter_map(|(e, enemy)| {
                    swept_aabb(bullet, step, enemy.body()).map(|h| (e, h.time))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((e, _)) = hit {
                self.bullets.swap_remove(i);
                self.enemies[e].hp -= 1;
                if self.enemies[e].hp == 0 {
                    let enemy = self.enemies.swap_remove(e);
                    self.score += 10 * self.wave;
                    self.explode(enemy.body().center(), Color::from_hex(0x60d060), 14);
                    ctx.play(Sound {
                        name: "pop",
                        wave: Wave::Noise,
                        freq: 900.0,
                        slide_to: 200.0,
                        ms: 140.0,
                        volume: 0.3,
                        ..Default::default()
                    });
                }
                continue;
            }
            self.bullets[i] += step;
            if self.bullets[i].y < -BULLET.y {
                self.bullets.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn move_enemies(&mut self, ctx: &mut Context, enemy_fire: f32) {
        let dt = ctx.dt();
        let sway = (self.time * 1.2).sin() * 24.0;
        let fire_chance = enemy_fire * (1.0 + 0.3 * self.wave.saturating_sub(1) as f32) * dt;
        let dive_chance = 0.02 * self.wave as f32 * dt;
        for enemy in &mut self.enemies {
            let place = enemy.home + vec2(sway, 0.0);
            match enemy.flight {
                Flight::Entering => {
                    enemy.pos.x = place.x;
                    enemy.pos.y += 90.0 * dt;
                    if enemy.pos.y >= place.y {
                        enemy.pos.y = place.y;
                        enemy.flight = Flight::InPlace;
                    }
                }
                Flight::InPlace => {
                    enemy.pos = place;
                    if ctx.rng.chance(dive_chance) {
                        let aim = (self.ship - enemy.body().center()).normalized();
                        enemy.flight = Flight::Diving(aim * 130.0);
                    }
                }
                Flight::Diving(vel) => {
                    enemy.pos += vel * dt;
                    if enemy.pos.y > HEIGHT {
                        // Off the bottom: come back in from the top.
                        enemy.pos.y = -ENEMY_SIZE.y - 20.0;
                        enemy.flight = Flight::Entering;
                    }
                }
            }
            let on_screen = enemy.pos.y > 0.0;
            if on_screen && ctx.rng.chance(fire_chance) {
                let gun = enemy.body().center() + vec2(-1.0, 5.0);
                self.shots.push(gun);
            }
        }
        for shot in &mut self.shots {
            shot.y += ENEMY_SHOT_SPEED * dt;
        }
        self.shots.retain(|shot| shot.y < HEIGHT);
    }

    fn take_hits(&mut self, ctx: &mut Context) {
        self.safe_time = (self.safe_time - ctx.dt()).max(0.0);
        if self.safe_time > 0.0 {
            return;
        }
        let ship = self.ship_hitbox();
        let shot = self
            .shots
            .iter()
            .position(|&s| Rect::new(s.x, s.y, 2.0, 4.0).overlaps(&ship));
        let rammed = self.enemies.iter().position(|e| e.body().overlaps(&ship));
        if shot.is_none() && rammed.is_none() {
            return;
        }
        if let Some(s) = shot {
            self.shots.swap_remove(s);
        }
        if let Some(e) = rammed {
            let enemy = self.enemies.swap_remove(e);
            self.explode(enemy.body().center(), Color::from_hex(0x60d060), 14);
        }
        self.lives -= 1;
        self.safe_time = SAFE_TIME;
        self.explode(self.ship, Color::from_hex(0xff8030), 24);
        self.camera.shake(6.0, 0.4);
        if self.lives == 0 {
            self.state = State::GameOver;
            ctx.play(Sound::explosion());
        } else {
            ctx.play(Sound::hit());
        }
    }
}

impl Game for Shooter {
    fn update(&mut self, ctx: &mut Context) {
        // Tunable numbers first, so the tune file always lists them all.
        let ship_speed = ctx.tune("ship_speed", SHIP_SPEED);
        let bullet_speed = ctx.tune("bullet_speed", BULLET_SPEED);
        let fire_delay = ctx.tune("fire_delay", FIRE_DELAY);
        let enemy_fire = ctx.tune("enemy_fire", ENEMY_FIRE);
        let dt = ctx.dt();
        let fire = ctx.input.is_down(Key::Space) || ctx.input.is_down(Key::Z);

        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        match self.state {
            State::Ready => {
                if ctx.input.was_pressed(Key::Space) || ctx.input.was_pressed(Key::Enter) {
                    self.state = State::Playing;
                    self.reload = fire_delay; // the start press doesn't fire
                }
            }
            State::Playing if ctx.input.was_pressed(Key::P) => self.state = State::Paused,
            State::Paused if ctx.input.was_pressed(Key::P) => self.state = State::Playing,
            State::GameOver
                if ctx.input.was_pressed(Key::Space) || ctx.input.was_pressed(Key::Enter) =>
            {
                *self = Shooter::new();
            }
            _ => {}
        }

        let score_before = self.score;
        if self.state == State::Playing {
            self.time += dt;
            self.fly_ship(ctx, ship_speed);
            self.reload -= dt;
            if fire && self.reload <= 0.0 {
                self.reload = fire_delay;
                self.bullets.push(self.ship - vec2(BULLET.x / 2.0, 12.0));
                ctx.play(Sound {
                    name: "laser",
                    wave: Wave::Square,
                    freq: 1400.0,
                    slide_to: 500.0,
                    ms: 60.0,
                    volume: 0.12,
                    ..Default::default()
                });
            }
            // Enemies first, then bullets, so a bullet is checked against
            // where the enemies are now.
            self.move_enemies(ctx, enemy_fire);
            self.move_bullets(ctx, bullet_speed);
            self.take_hits(ctx);
            if self.enemies.is_empty() {
                self.next_wave_in -= dt;
                if self.next_wave_in <= 0.0 {
                    self.next_wave_in = 1.5;
                    self.start_wave(ctx);
                }
            }
        } else {
            // Waiting for a key (title, pause, game over) is expected.
            ctx.idle_ok();
        }

        // Effects keep moving, even on the game-over screen.
        if self.state != State::Paused {
            for particle in &mut self.particles {
                particle.pos += particle.vel * dt;
                particle.vel = particle.vel * (1.0 - 2.0 * dt); // slow down
                particle.life -= dt;
            }
            self.particles.retain(|p| p.life > 0.0);
            for (pos, speed) in &mut self.stars {
                pos.y = (pos.y + *speed * dt) % HEIGHT;
            }
        }
        self.camera.update(ctx);

        // Rules that must always hold (the summary, play tests and --fuzz check them).
        let screen = Rect::new(0.0, 0.0, WIDTH, HEIGHT);
        ctx.check("ship on screen", screen.contains(self.ship));
        ctx.check("score never goes down", self.score >= score_before);
        ctx.check("lives never above the start", self.lives <= START_LIVES);
        let inside = self.bullets.iter().any(|b| {
            let bullet = Rect::new(b.x, b.y, BULLET.x, BULLET.y);
            self.enemies.iter().any(|e| e.body().overlaps(&bullet))
        });
        ctx.check("no bullet inside an enemy", !inside);

        // For the debug overlay (F1).
        ctx.debug_rect(self.ship_hitbox());
        for enemy in &self.enemies {
            ctx.debug_rect(enemy.body());
        }

        // For tests and tools (log.json).
        ctx.report("state", format!("{:?}", self.state));
        ctx.report("score", self.score);
        ctx.report("lives", self.lives);
        ctx.report("wave", self.wave);
        ctx.report("enemies", self.enemies.len());
        ctx.report("x", self.ship.x.round());
        ctx.report("y", self.ship.y.round());
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::from_hex(0x05060f));

        // The world moves with the camera, so screen shake moves it all.
        canvas.push_camera(&self.camera);
        for &(pos, speed) in &self.stars {
            let shade = (speed * 4.0) as u8;
            canvas.fill_rect(
                Rect::new(pos.x, pos.y, 1.0, 1.0),
                Color::rgb(shade, shade, shade + 40),
            );
        }
        let wings = self.bug.frame_at(self.time);
        for enemy in &self.enemies {
            canvas.blit_sprite(
                wings,
                enemy.pos,
                Blit {
                    scale: 2.0,
                    ..Default::default()
                },
            );
            canvas.label("enemy", enemy.body());
        }
        for &bullet in &self.bullets {
            canvas.fill_rect(
                Rect::new(bullet.x, bullet.y, BULLET.x, BULLET.y),
                Color::from_hex(0xfff080),
            );
        }
        for &shot in &self.shots {
            let shot = Rect::new(shot.x, shot.y, 2.0, 4.0);
            canvas.fill_rect(shot, Color::from_hex(0xff5050));
            canvas.label("enemy_shot", shot);
        }
        for particle in &self.particles {
            let alpha = (particle.life.min(0.5) * 2.0 * 255.0) as u8;
            let square = Rect::from_center(particle.pos, 2.0, 2.0);
            canvas.fill_rect(square, particle.color.with_alpha(alpha));
        }
        // Blink while safe after a hit; gone when the game is over.
        let blink = self.safe_time > 0.0 && (self.safe_time * 10.0) as i32 % 2 == 0;
        if self.state != State::GameOver && !blink {
            let corner = self.ship - vec2(7.0, 7.0);
            canvas.blit_sprite(
                &self.ship_sprite,
                corner,
                Blit {
                    scale: 2.0,
                    ..Default::default()
                },
            );
        }
        canvas.label("ship", self.ship_hitbox());
        canvas.pop_camera();

        // The HUD, in screen pixels.
        let hud = format!(
            "SCORE {}   WAVE {}   LIVES {}",
            self.score, self.wave, self.lives
        );
        canvas.draw_text(&hud, vec2(4.0, 4.0), 1, Color::WHITE);
        let (title, hint) = match self.state {
            State::Playing if self.enemies.is_empty() && self.wave > 0 => ("WAVE CLEAR", ""),
            State::Playing => return,
            State::Ready => ("SHOOTER", "press Space to start"),
            State::Paused => ("PAUSED", "press P to carry on"),
            State::GameOver => ("GAME OVER", "press Space to play again"),
        };
        canvas.draw_text_centered(title, 130.0, 3, Color::YELLOW);
        canvas.draw_text_centered(hint, 160.0, 1, Color::GRAY);
    }
}

fn config() -> Config {
    Config {
        title: String::from("Shooter starter - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        scale: 2, // a tall window: 480 x 640 on screen
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Shooter::new())
}

// ---- Play tests: `cargo test --example starter_shooter` ----

#[cfg(test)]
mod tests {
    use super::*;

    /// Press Space on frame 1 to start, then hold `held` from frame 10.
    fn start_then_hold(held: &[&str]) -> String {
        let mut events = vec![
            String::from(r#"{"frame": 1, "press": "Space"}"#),
            String::from(r#"{"frame": 2, "release": "Space"}"#),
        ];
        for key in held {
            events.push(format!(r#"{{"frame": 10, "press": "{key}"}}"#));
        }
        format!(r#"{{"events": [{}]}}"#, events.join(", "))
    }

    /// A game already playing, with one enemy in place straight above the ship.
    fn one_enemy_above() -> Shooter {
        let mut game = Shooter::new();
        game.state = State::Playing;
        game.wave = 1;
        let pos = vec2(game.ship.x - ENEMY_SIZE.x / 2.0, 60.0);
        game.enemies = vec![Enemy {
            pos,
            home: pos,
            hp: 1,
            flight: Flight::Diving(Vec2::ZERO), // stays put
        }];
        game
    }

    #[test]
    fn it_waits_for_space() {
        let played = PlayTest::new(config(), Shooter::new()).frames(300).run();
        assert_eq!(played.text("state"), "Ready");
        assert_eq!(played.number("wave"), 0.0);
    }

    #[test]
    fn holding_fire_shoots_at_the_fire_rate() {
        let played = PlayTest::new(config(), Shooter::new())
            .script(&start_then_hold(&["Space"]))
            .tune("enemy_fire", 0.0)
            .frames(70)
            .run();
        // Frames 10 to 70 is one second: one shot every 0.12 s.
        let shots = played.sound_frames("laser").len();
        assert!((8..=9).contains(&shots), "{shots} shots");
    }

    #[test]
    fn even_very_fast_bullets_hit() {
        // At 3000 pixels per second a bullet moves 50 pixels a frame, five
        // times the enemy's height. A plain overlap test would miss.
        let played = PlayTest::new(config(), one_enemy_above())
            .script(r#"{"events": [{"frame": 1, "press": "Space"}]}"#)
            .tune("bullet_speed", 3000.0)
            .tune("enemy_fire", 0.0)
            .frames(10)
            .run();
        assert_eq!(played.number("score"), 10.0);
        played.assert_sound("pop", 1, 5);
    }

    #[test]
    fn clearing_a_wave_starts_the_next() {
        let played = PlayTest::new(config(), one_enemy_above())
            .script(r#"{"events": [{"frame": 1, "press": "Space"}]}"#)
            .tune("enemy_fire", 0.0)
            .frames(150)
            .run();
        assert_eq!(played.number("wave"), 2.0);
        // Wave 2 arrives with 8 enemies (and the ship keeps shooting them).
        let counts: Vec<f64> = played
            .history("enemies")
            .iter()
            .filter_map(|(_, n)| n.as_number())
            .collect();
        assert_eq!(counts[..3], [1.0, 0.0, 8.0], "1, shot it, then 8");
    }

    #[test]
    fn a_hit_costs_a_life_explodes_and_gives_a_moment_of_safety() {
        let mut game = one_enemy_above();
        game.shots = vec![game.ship]; // an enemy shot right on the ship
        let mut sim = Simulation::new(config(), game);
        sim.step();
        assert_eq!(sim.number("lives"), 2.0);
        sim.assert_sound("hit", 1, 0);
        assert!(!sim.game().particles.is_empty(), "an explosion");
        for _ in 0..60 {
            let ship = sim.game().ship;
            sim.game_mut().shots.push(ship); // keep shooting it
            sim.step();
        }
        assert_eq!(sim.number("lives"), 2.0, "safe for {SAFE_TIME} s");
    }

    /// Makes playthroughs/starter_shooter.json: a bot holds fire, flies
    /// under the nearest enemy, and steps aside from shots about to hit.
    /// `cargo test --example starter_shooter -- --ignored`
    #[test]
    #[ignore]
    fn record_playthrough() {
        let mut sim = Simulation::new(config(), Shooter::new());
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        sim.step();
        sim.press(Key::Space); // and hold it: auto-fire
        while sim.text("state") == "Playing" && sim.context().frame() < 90 * 60 {
            let game = sim.game();
            let me = game.ship;
            let danger = game
                .shots
                .iter()
                .find(|s| s.y < me.y && s.y > me.y - 70.0 && (s.x - me.x).abs() < 12.0);
            let target = match danger {
                Some(shot) if shot.x > me.x || me.x > WIDTH - 30.0 => me.x - 30.0,
                Some(_) => me.x + 30.0,
                None => game
                    .enemies
                    .iter()
                    .filter(|e| e.flight == Flight::InPlace)
                    .map(|e| e.body().center().x)
                    .min_by(|a, b| (a - me.x).abs().total_cmp(&(b - me.x).abs()))
                    .unwrap_or(WIDTH / 2.0),
            };
            let keys = [
                (Key::Left, target < me.x - 2.0),
                (Key::Right, target > me.x + 2.0),
            ];
            for (key, held) in keys {
                if held {
                    sim.press(key);
                } else {
                    sim.release(key);
                }
            }
            sim.step();
        }
        for key in [Key::Left, Key::Right, Key::Space] {
            sim.release(key);
        }
        sim.run(90); // the explosion and GAME OVER
        sim.recording()
            .save("playthroughs/starter_shooter.json")
            .unwrap();
    }
}

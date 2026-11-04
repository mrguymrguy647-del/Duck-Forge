//! A top-down starter: walk around a walled map, pick up every coin, and
//! keep away from the enemies that chase you when you come close.
//!
//!     cargo run --example starter_topdown
//!
//! Arrow keys (or WASD) walk, P pauses, Space starts again after the end,
//! Escape quits. F1 shows the debug overlay (hitboxes, and how far each
//! enemy can see), F10 records what you do.
//!
//! The parts, each small enough to change: a level drawn as text (MAP),
//! sprites drawn with letters, 8-direction movement (set DIAGONALS to false
//! for 4), sliding along walls, enemies that wander and then chase, coins,
//! lives with a moment of safety after a hit, a camera that follows you,
//! sounds, reports, rules (`ctx.check`) and play tests (in tests.rs). Copy
//! this folder to `examples/mygame/` to start your own game.
//!
//! Without a screen:
//!
//!     cargo run --no-default-features --example starter_topdown -- --headless --sheet --out out
//!     tools/verify.sh starter_topdown

use duckforge::prelude::*;

// ---- Settings: change these first ----
//
// PLAYER_SPEED, ENEMY_SPEED and SIGHT are read with `ctx.tune`, so
// examples/starter_topdown.tune.json or `--set enemy_speed=70` changes them
// without rebuilding.

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;
const TILE: f32 = 16.0; // each letter of MAP is a 16 x 16 pixel square
const BODY: f32 = 12.0; // the player's and the enemies' hitbox (a square)
const PLAYER_SPEED: f32 = 90.0; // pixels per second
const ENEMY_SPEED: f32 = 50.0;
const SIGHT: f32 = 100.0; // enemies chase you when you're this close
const DIAGONALS: bool = true; // false: only up, down, left or right at a time
const START_LIVES: u32 = 3;
const SAFE_TIME: f32 = 1.5; // seconds you can't be hit again after a hit

/// The level: '#' wall, '.' floor, 'p' where you start, 'c' a coin, 'e' an
/// enemy. Every row must be as long as the first.
const MAP: [&str; 30] = [
    "########################################",
    "#...........#..............#...........#",
    "#.p......c..#....c.........#........c..#",
    "#...........#..............#...........#",
    "#....#......#.......##.....#.....#.....#",
    "#..........................#......e....#",
    "#...................e......#...........#",
    "#...........#...........c..............#",
    "#...........#.......................c..#",
    "#...........#..............#...........#",
    "#####..############..############..#####",
    "#...........#..............#...........#",
    "#...........#..............#....c......#",
    "#..c........#..........................#",
    "#...........#.......#..................#",
    "#.....#..............#.....#..e........#",
    "#.....e....................#.....#.....#",
    "#.........c.#.......c......#...........#",
    "#...........#..............#.........c.#",
    "#...........#..............#...........#",
    "########..#############..##########..###",
    "#.................#....................#",
    "#.................#...........c........#",
    "#.............c...#....................#",
    "#........#................#............#",
    "#.........e....................#.......#",
    "#.................#..............e.....#",
    "#..c..............#....c.............c.#",
    "#.................#....................#",
    "########################################",
];

/// The hero, two 6 x 6 frames side by side ('.' is see-through).
const HERO: [&str; 6] = [
    "..ss....ss..",
    ".kssk..kssk.",
    ".bbbb..bbbb.",
    "sbbbbssbbbbs",
    ".bbbb..bbbb.",
    ".b..b...bb..",
];

/// An enemy, two frames.
const BLOB: [&str; 6] = [
    "..rr....rr..",
    ".rrrr..rrrr.",
    "rwkrwkrwkrwk",
    "rrrrrrrrrrrr",
    "rrrrrrrrrrrr",
    "r.rr.r.r..r.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Playing,
    Paused,
    Won,
    Lost,
}

struct Enemy {
    body: Rect,
    /// Where it walks when it can't see you, and for how much longer.
    wander: Vec2,
    wander_time: f32,
    chasing: bool,
}

struct TopDown {
    state: State,
    walls: Vec<Vec<bool>>,
    player: Rect,
    facing_left: bool,
    /// For 4-direction movement: was a left/right key pressed last?
    last_horizontal: bool,
    walk_time: f32,
    enemies: Vec<Enemy>,
    coins: Vec<Vec2>,
    total_coins: u32,
    collected: u32,
    lives: u32,
    safe_time: f32,
    time: f32,
    hero: Animation,
    blob: Animation,
    camera: Camera2D,
}

impl TopDown {
    fn new() -> TopDown {
        let mut walls = Vec::new();
        let (mut player, mut enemies, mut coins) = (Rect::new(0.0, 0.0, 0.0, 0.0), vec![], vec![]);
        for (ty, row) in MAP.iter().enumerate() {
            walls.push(row.chars().map(|c| c == '#').collect());
            for (tx, c) in row.chars().enumerate() {
                // A body standing in the middle of the tile.
                let center = vec2((tx as f32 + 0.5) * TILE, (ty as f32 + 0.5) * TILE);
                let body = Rect::from_center(center, BODY, BODY);
                match c {
                    'p' => player = body,
                    'c' => coins.push(center),
                    'e' => enemies.push(Enemy {
                        body,
                        wander: Vec2::ZERO,
                        wander_time: 0.0,
                        chasing: false,
                    }),
                    _ => {}
                }
            }
        }
        let hero = Sprite::from_ascii(
            &HERO,
            &[
                ('s', Color::from_hex(0xf2c79a)),
                ('k', Color::BLACK),
                ('b', Color::from_hex(0x3a6fd8)),
            ],
        );
        let blob = Sprite::from_ascii(
            &BLOB,
            &[
                ('r', Color::from_hex(0xd8443a)),
                ('w', Color::WHITE),
                ('k', Color::BLACK),
            ],
        );
        let mut camera = Camera2D::new(WIDTH, HEIGHT);
        camera.position = player.center();
        TopDown {
            state: State::Playing,
            walls,
            player,
            facing_left: false,
            last_horizontal: true,
            walk_time: 0.0,
            enemies,
            total_coins: coins.len() as u32,
            coins,
            collected: 0,
            lives: START_LIVES,
            safe_time: 0.0,
            time: 0.0,
            hero: Animation::new(hero.frames(6, 6), 8.0),
            blob: Animation::new(blob.frames(6, 6), 4.0),
            camera,
        }
    }

    fn world(&self) -> Rect {
        Rect::new(
            0.0,
            0.0,
            MAP[0].len() as f32 * TILE,
            MAP.len() as f32 * TILE,
        )
    }

    /// A wall tile that `rect` overlaps, if any. Outside the map counts as wall.
    fn wall_touching(&self, rect: Rect) -> Option<Rect> {
        let tiles =
            |from: f32, to: f32| (from / TILE).floor() as i32..=((to - 0.01) / TILE).floor() as i32;
        for ty in tiles(rect.top(), rect.bottom()) {
            for tx in tiles(rect.left(), rect.right()) {
                let wall = ty < 0
                    || tx < 0
                    || self
                        .walls
                        .get(ty as usize)
                        .and_then(|row| row.get(tx as usize))
                        .copied()
                        .unwrap_or(true);
                if wall {
                    return Some(Rect::new(tx as f32 * TILE, ty as f32 * TILE, TILE, TILE));
                }
            }
        }
        None
    }

    /// Moves `body` by `step`, one axis at a time, stopping at walls: so
    /// pushing diagonally into a wall slides along it.
    fn slide(&self, mut body: Rect, step: Vec2) -> Rect {
        body.x += step.x;
        if let Some(wall) = self.wall_touching(body) {
            body.x = if step.x > 0.0 {
                wall.left() - body.w
            } else {
                wall.right()
            };
        }
        body.y += step.y;
        if let Some(wall) = self.wall_touching(body) {
            body.y = if step.y > 0.0 {
                wall.top() - body.h
            } else {
                wall.bottom()
            };
        }
        body
    }

    fn move_player(&mut self, ctx: &mut Context, speed: f32) {
        let input = &ctx.input;
        let mut dir = vec2(
            (input.axis(Key::Left, Key::Right) + input.axis(Key::A, Key::D)).clamp(-1.0, 1.0),
            (input.axis(Key::Up, Key::Down) + input.axis(Key::W, Key::S)).clamp(-1.0, 1.0),
        );
        if [Key::Left, Key::Right, Key::A, Key::D]
            .iter()
            .any(|&k| input.was_pressed(k))
        {
            self.last_horizontal = true;
        }
        if [Key::Up, Key::Down, Key::W, Key::S]
            .iter()
            .any(|&k| input.was_pressed(k))
        {
            self.last_horizontal = false;
        }
        if !DIAGONALS && dir.x != 0.0 && dir.y != 0.0 {
            // One direction at a time: the one pressed last.
            if self.last_horizontal {
                dir.y = 0.0;
            } else {
                dir.x = 0.0;
            }
        }
        if dir.length() > 0.0 {
            dir = dir.normalized(); // so diagonals aren't faster
            self.walk_time += ctx.dt();
            if dir.x != 0.0 {
                self.facing_left = dir.x < 0.0;
            }
        }
        self.player = self.slide(self.player, dir * speed * ctx.dt());
    }

    fn move_enemies(&mut self, ctx: &mut Context, speed: f32, sight: f32) {
        let target = self.player.center();
        for i in 0..self.enemies.len() {
            let enemy = &mut self.enemies[i];
            let to_player = target - enemy.body.center();
            enemy.chasing = to_player.length() < sight;
            let dir = if enemy.chasing {
                to_player.normalized()
            } else {
                // Wander: a new random direction (or a rest) every second or two.
                enemy.wander_time -= ctx.dt();
                if enemy.wander_time <= 0.0 {
                    enemy.wander_time = ctx.rng.range(1.0, 2.5);
                    enemy.wander = if ctx.rng.chance(0.3) {
                        Vec2::ZERO
                    } else {
                        let angle = ctx.rng.range(0.0, std::f32::consts::TAU);
                        vec2(angle.cos(), angle.sin())
                    };
                }
                enemy.wander * 0.5
            };
            let moved = self.slide(self.enemies[i].body, dir * speed * ctx.dt());
            self.enemies[i].body = moved;
        }
    }

    fn take_hits(&mut self, ctx: &mut Context) {
        self.safe_time = (self.safe_time - ctx.dt()).max(0.0);
        if self.safe_time > 0.0 {
            return;
        }
        let Some(enemy) = self.enemies.iter().find(|e| e.body.overlaps(&self.player)) else {
            return;
        };
        // Knocked back, away from the enemy (and never into a wall).
        let away = (self.player.center() - enemy.body.center()).normalized();
        for _ in 0..4 {
            self.player = self.slide(self.player, away * 3.0);
        }
        self.lives -= 1;
        self.safe_time = SAFE_TIME;
        self.camera.shake(4.0, 0.3);
        if self.lives == 0 {
            self.state = State::Lost;
            ctx.play(Sound::explosion());
        } else {
            ctx.play(Sound::hit());
        }
    }
}

impl Game for TopDown {
    fn update(&mut self, ctx: &mut Context) {
        // Tunable numbers first, so the tune file always lists them all.
        let player_speed = ctx.tune("player_speed", PLAYER_SPEED);
        let enemy_speed = ctx.tune("enemy_speed", ENEMY_SPEED);
        let sight = ctx.tune("sight", SIGHT);

        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        match self.state {
            State::Playing => {
                if ctx.input.was_pressed(Key::P) {
                    self.state = State::Paused;
                }
            }
            State::Paused => {
                if ctx.input.was_pressed(Key::P) {
                    self.state = State::Playing;
                }
            }
            State::Won | State::Lost => {
                if ctx.input.was_pressed(Key::Space) || ctx.input.was_pressed(Key::Enter) {
                    *self = TopDown::new();
                }
            }
        }

        if self.state == State::Playing {
            self.time += ctx.dt();
            self.move_player(ctx, player_speed);
            self.move_enemies(ctx, enemy_speed, sight);
            self.take_hits(ctx);

            // Coins: keep the ones the player isn't touching.
            let before = self.coins.len();
            let player = self.player;
            self.coins
                .retain(|&coin| !circle_vs_rect(coin, 4.0, player));
            for _ in self.coins.len()..before {
                self.collected += 1;
                ctx.play(Sound::coin());
            }
            if self.coins.is_empty() {
                self.state = State::Won;
                ctx.beep(880.0, 300.0);
            }
        } else {
            // Paused or finished: waiting for a key is expected (for --fuzz).
            ctx.idle_ok();
        }

        // The camera follows the player, but never shows past the map's edges.
        let world = self.world();
        self.camera.follow(self.player.center(), 0.15, ctx.dt());
        self.camera.position.x = self
            .camera
            .position
            .x
            .clamp(WIDTH / 2.0, world.w - WIDTH / 2.0);
        self.camera.position.y = self
            .camera
            .position
            .y
            .clamp(HEIGHT / 2.0, world.h - HEIGHT / 2.0);
        self.camera.update(ctx);

        // Rules that must always hold (the summary, play tests and --fuzz check them).
        ctx.check(
            "player inside the map",
            world.contains(self.player.center()),
        );
        ctx.check(
            "player never inside a wall",
            self.wall_touching(self.player).is_none(),
        );
        let enemies_clear = self
            .enemies
            .iter()
            .all(|e| self.wall_touching(e.body).is_none());
        ctx.check("enemies never inside a wall", enemies_clear);
        ctx.check(
            "coins add up",
            self.collected + self.coins.len() as u32 == self.total_coins,
        );
        ctx.check("lives never above the start", self.lives <= START_LIVES);

        // For the debug overlay (F1): hitboxes, and how far chasers see.
        ctx.debug_rect(self.player);
        for enemy in &self.enemies {
            ctx.debug_rect(enemy.body);
            if enemy.chasing {
                ctx.debug_circle(enemy.body.center(), sight);
            }
        }

        // For tests and tools (log.json). The position only changes when
        // you walk, so a key press always shows up here.
        ctx.report("state", format!("{:?}", self.state));
        ctx.report("coins", self.collected);
        ctx.report("lives", self.lives);
        ctx.report("x", self.player.x.round());
        ctx.report("y", self.player.y.round());
        ctx.report("chasing", self.enemies.iter().filter(|e| e.chasing).count());
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::from_hex(0x1a1a24));

        canvas.push_camera(&self.camera);
        // Only the tiles the camera can see.
        let view = self.camera.visible_area();
        let (tx0, ty0) = (
            (view.left() / TILE).floor().max(0.0) as usize,
            (view.top() / TILE).floor().max(0.0) as usize,
        );
        let (tx1, ty1) = (
            (view.right() / TILE) as usize + 1,
            (view.bottom() / TILE) as usize + 1,
        );
        for (ty, row) in self.walls.iter().enumerate().take(ty1).skip(ty0) {
            for (tx, &wall) in row.iter().enumerate().take(tx1).skip(tx0) {
                let tile = Rect::new(tx as f32 * TILE, ty as f32 * TILE, TILE, TILE);
                if wall {
                    canvas.fill_rect(tile, Color::from_hex(0x5a5a78));
                    canvas.fill_rect(
                        Rect::new(tile.x, tile.y, TILE, 3.0),
                        Color::from_hex(0x8080a0),
                    );
                } else if (tx + ty) % 2 == 0 {
                    canvas.fill_rect(tile, Color::from_hex(0x24243a));
                }
            }
        }
        for &coin in &self.coins {
            canvas.fill_circle(coin, 4.0, Color::from_hex(0xffd700));
            canvas.label("coin", Rect::from_center(coin, 8.0, 8.0));
        }
        let scaled = Blit {
            scale: 2.0,
            ..Default::default()
        };
        for enemy in &self.enemies {
            let frame = self.blob.frame_at(self.time);
            canvas.blit_sprite(frame, enemy.body.center() - vec2(6.0, 6.0), scaled);
            canvas.label("enemy", enemy.body);
        }
        // Blink while safe after a hit.
        let blink = self.safe_time > 0.0 && (self.safe_time * 10.0) as i32 % 2 == 0;
        if !blink {
            let frame = self.hero.frame_at(self.walk_time);
            let blit = Blit {
                flip_x: self.facing_left,
                ..scaled
            };
            canvas.blit_sprite(frame, self.player.center() - vec2(6.0, 6.0), blit);
        }
        canvas.label("player", self.player);
        canvas.pop_camera();

        // The HUD, in screen pixels, on a dark strip so it's readable.
        canvas.fill_rect(
            Rect::new(0.0, 0.0, WIDTH, 12.0),
            Color::BLACK.with_alpha(170),
        );
        let hud = format!(
            "COINS {}/{}   LIVES {}",
            self.collected, self.total_coins, self.lives
        );
        canvas.draw_text(&hud, vec2(4.0, 3.0), 1, Color::WHITE);
        let (title, hint, color) = match self.state {
            State::Playing => return,
            State::Paused => ("PAUSED", "press P to carry on", Color::WHITE),
            State::Won => ("ALL COINS!", "press Space to play again", Color::YELLOW),
            State::Lost => (
                "CAUGHT!",
                "press Space to try again",
                Color::from_hex(0xff6060),
            ),
        };
        canvas.fill_rect(
            Rect::new(0.0, 90.0, WIDTH, 56.0),
            Color::BLACK.with_alpha(170),
        );
        canvas.draw_text_centered(title, 100.0, 3, color);
        canvas.draw_text_centered(hint, 130.0, 1, Color::GRAY);
    }
}

fn config() -> Config {
    Config {
        title: String::from("Top-down starter - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), TopDown::new())
}

// ---- Tests ----
//
// The play tests are in tests.rs, next to this file
// (`cargo test --example starter_topdown`), and the bot that records
// playthroughs/starter_topdown.json is in bot.rs.

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bot;

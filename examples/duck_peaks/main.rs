//! Duck Peaks: started from the platformer starter with tools/new_game.
//!
//! Climb the mountain at sunset: run and jump up rock terraces and planks,
//! over two chasms of water, collect the ten coins and reach the flag on
//! the snowy summit.
//!
//!     cargo run --example duck_peaks
//!
//! Arrow keys (or A/D) run, Up/Space/W jumps, P pauses, Escape quits.
//!
//! - The level is text (`level.txt`, 100 x 30 tiles, two screens tall)
//!   read into a tile map with `level.legend.json` (and read again when
//!   they change); the camera follows the duck up as well as across.
//! - Three painted layers, from Duck Quill (`art/`): a sunset sky, far
//!   mountains and a pine forest, each moving at its own speed sideways
//!   and up and down (`parallax_xy`), so the climb has depth.
//! - The duck and the tiles (with new rock and snow) are Duck Quill
//!   sprite sheets; the music is a Duck Echo loop (`art/music.toml`).
//!
//! Without a screen (or to test):
//!
//!     cargo run --no-default-features --example duck_peaks -- --headless --inputs playthroughs/duck_peaks.json --sheet-events --out out
//!     cargo test --no-default-features --example duck_peaks

use duckforge::LoadError;
use duckforge::prelude::*;

// ---- Settings: change these first ----
//
// GRAVITY, RUN_SPEED and JUMP_SPEED are *tunable*: the game reads them with
// `ctx.tune`, so `--set jump_speed=400` (or a tune file, made with
// --write-tune) can change them without rebuilding, even while it runs.

const WIDTH: f32 = 320.0; // the screen, in pixels
const HEIGHT: f32 = 240.0;
const TILE: f32 = 16.0; // one letter of the level, in pixels
const GRAVITY: f32 = 900.0; // pixels per second, per second
const RUN_SPEED: f32 = 110.0; // pixels per second
const JUMP_SPEED: f32 = 330.0; // about 60 pixels high: almost four tiles
const FALL_LIMIT: f32 = 400.0; // the fastest it falls
const PLAYER_SIZE: Vec2 = vec2(10.0, 14.0); // the hitbox, a little smaller than the picture

/// Where the art is (`quill export` and `quill render` made these).
const ASSETS: &str = "examples/duck_peaks/assets";

/// The level: one letter per 16 x 16 tile, in `level.txt` next to this
/// file; `level.legend.json` says what each letter is (and where the
/// tiles' pictures are). Change either, even while the game runs: it reads
/// them again (see `reload`). Duck Forge Studio paints it in Build mode.
const LEVEL: &str = "examples/duck_peaks/level.txt";

/// Everything loaded from files, once, when the game starts.
#[derive(Clone)]
struct Art {
    duck: SpriteSheet,
    /// The tiles, also as a sprite sheet for the coins and the flag.
    tile_sheet: SpriteSheet,
    tiles: Tileset,
    sky: Background,
    mountains: Background,
    forest: Background,
    music: Wav,
}

impl Art {
    fn load() -> Result<Art, LoadError> {
        let tile_sheet = SpriteSheet::load(format!("{ASSETS}/tiles.json"))?;
        Ok(Art {
            duck: SpriteSheet::load(format!("{ASSETS}/hero.json"))?,
            tiles: Tileset::from_sheet(&tile_sheet),
            tile_sheet,
            // Far things move less. The camera's scroll goes from 240 (at
            // the bottom of the level, where it starts) to 0 (the summit),
            // and each offset is chosen so its layer sits right at the
            // start: the sky covers the screen all the way up, the
            // mountains' peaks are near the top, and the forest stands on
            // the ground (and sinks out of sight as the duck climbs).
            sky: Background::new(Sprite::load_png(format!("{ASSETS}/sky.png"))?)
                .parallax_xy(0.05, 0.25),
            mountains: Background::new(Sprite::load_png(format!("{ASSETS}/mountains.png"))?)
                .parallax_xy(0.2, 0.3)
                .offset(vec2(0.0, 122.0)),
            forest: Background::new(Sprite::load_png(format!("{ASSETS}/forest.png"))?)
                .parallax_xy(0.5, 0.6)
                .offset(vec2(0.0, 224.0)),
            music: Wav::load(format!("{ASSETS}/music.wav"))?,
        })
    }
}

/// Where the duck starts: the level's Start, or the top left corner if it
/// has none (while it's being painted, say).
fn start_of(map: &TileMap) -> Vec2 {
    map.spawns("player")
        .first()
        .copied()
        .unwrap_or(vec2(TILE / 2.0, TILE))
}

/// What the game is doing. An `enum` means it can only be one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Playing,
    Paused,
    Won,
}

#[derive(Clone)]
struct DuckPeaks {
    state: State,
    art: Art,
    map: TileMap,
    /// The duck's hitbox, in world pixels.
    player: Rect,
    vel: Vec2,
    on_ground: bool,
    facing_left: bool,
    /// Coins not yet collected: the bottom centre of each.
    coins: Vec<Vec2>,
    collected: u32,
    /// Times the duck fell into the water.
    splashes: u32,
    flag: Vec2,
    /// Seconds of play, for animations (kept here: `draw` has no `ctx`).
    time: f32,
    camera: Camera2D,
    /// The music, once it has started.
    music: Option<LoopId>,
}

impl DuckPeaks {
    fn new(art: Art) -> DuckPeaks {
        // A mistake in the level's files stops the game with a message
        // saying what and where.
        let map = TileMap::load_level(LEVEL).unwrap_or_else(|error| panic!("{error}"));
        Self::on_map(art, map)
    }

    /// The game on `map`: the level's files, or (in tests) a tiny level
    /// of their own.
    fn on_map(art: Art, map: TileMap) -> Self {
        let start = start_of(&map);
        let mut game = DuckPeaks {
            state: State::Playing,
            art,
            player: Rect::new(0.0, 0.0, PLAYER_SIZE.x, PLAYER_SIZE.y),
            vel: Vec2::ZERO,
            on_ground: true,
            facing_left: false,
            coins: map.spawns("coin"),
            collected: 0,
            splashes: 0,
            // No flag (being painted in, say): it's out of the way.
            flag: map
                .spawns("flag")
                .first()
                .copied()
                .unwrap_or(vec2(-100.0, -100.0)),
            time: 0.0,
            camera: Camera2D::new(WIDTH, HEIGHT),
            music: None,
            map,
        };
        game.put_player_at(start);
        // The camera starts at the bottom of the level, on the left.
        let bottom = game.map.bounds().bottom();
        game.camera.position = vec2(WIDTH / 2.0, bottom - HEIGHT / 2.0);
        game
    }

    /// Stands the duck with its feet at `feet`.
    fn put_player_at(&mut self, feet: Vec2) {
        self.player.x = feet.x - PLAYER_SIZE.x / 2.0;
        self.player.y = feet.y - PLAYER_SIZE.y;
        self.vel = Vec2::ZERO;
    }

    /// Where the duck's feet are: the bottom centre of its hitbox.
    fn feet(&self) -> Vec2 {
        vec2(self.player.center().x, self.player.bottom())
    }

    /// The coin's area: the tile it sits in.
    fn coin_rect(coin: Vec2) -> Rect {
        Rect::new(coin.x - TILE / 2.0, coin.y - TILE, TILE, TILE)
    }

    fn play(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let run_speed = ctx.tune("run_speed", RUN_SPEED);
        let jump_speed = ctx.tune("jump_speed", JUMP_SPEED);
        let gravity = ctx.tune("gravity", GRAVITY);
        self.time += dt;
        // The music starts on the first frame and loops until the end.
        if self.music.is_none() {
            self.music = Some(ctx.play_loop(&self.art.music, 0.5));
        }

        // Run: -1, 0 or 1 from the keys, then ease the speed towards it.
        let run = (ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D))
            .clamp(-1.0, 1.0);
        self.vel.x = approach(self.vel.x, run * run_speed, run_speed * 8.0 * dt);
        if run != 0.0 {
            self.facing_left = run < 0.0;
        }
        // Jump, only from the ground.
        let jump = [Key::Up, Key::Space, Key::W]
            .iter()
            .any(|&k| ctx.input.was_pressed(k));
        if jump && self.on_ground {
            self.vel.y = -jump_speed;
            ctx.play(Sound::jump());
        }
        self.vel.y = (self.vel.y + gravity * dt).min(FALL_LIMIT);

        // The tile map moves the duck and says what it bumped into.
        let touched = self.map.move_and_slide(&mut self.player, self.vel * dt);
        self.on_ground = touched.ground;
        if touched.ground || touched.ceiling {
            self.vel.y = 0.0;
        }
        if touched.wall_left || touched.wall_right {
            self.vel.x = 0.0;
        }
        // The level's ends are walls.
        let level = self.map.bounds();
        self.player.x = self
            .player
            .x
            .clamp(level.left(), level.right() - PLAYER_SIZE.x);

        // Fell in the water (below the level): back to the start.
        if self.player.top() > level.bottom() {
            self.splashes += 1;
            ctx.play(Sound::hit());
            let start = start_of(&self.map);
            self.put_player_at(start);
        }

        // Coins: keep the ones the duck isn't touching.
        let player = self.player;
        let before = self.coins.len();
        self.coins
            .retain(|&coin| !DuckPeaks::coin_rect(coin).overlaps(&player));
        for _ in self.coins.len()..before {
            self.collected += 1;
            ctx.play(Sound::coin());
        }
        // The flag ends the level.
        if DuckPeaks::coin_rect(self.flag).overlaps(&player) {
            self.state = State::Won;
            ctx.play(Sound::explosion());
        }

        // The camera glides after the duck, up and across, but never
        // shows past the level's edges.
        self.camera.follow(player.center(), 0.15, dt);
        self.camera.position.x = self
            .camera
            .position
            .x
            .clamp(WIDTH / 2.0, (level.right() - WIDTH / 2.0).max(WIDTH / 2.0));
        self.camera.position.y = self.camera.position.y.clamp(
            HEIGHT / 2.0,
            (level.bottom() - HEIGHT / 2.0).max(HEIGHT / 2.0),
        );
    }
}

impl Game for DuckPeaks {
    fn update(&mut self, ctx: &mut Context) {
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
            self.play(ctx);
        } else {
            // Paused (or won), the game waits on purpose: tell --fuzz, so
            // it doesn't call that a freeze.
            ctx.idle_ok();
        }
        self.camera.update(ctx);

        // Rules that must always hold. If one ever breaks, the run's
        // summary, the play tests and --fuzz say so, with the frame.
        let room = self.map.bounds().expand(TILE * 4.0);
        ctx.check(
            "player inside the level",
            room.contains(self.player.center()),
        );
        ctx.check(
            "never inside a wall",
            self.map.kind_at(self.player.center()) != TileKind::Solid,
        );

        // For the debug overlay (F1) and for tests and AI agents (log.json).
        ctx.debug_rect(self.player);
        ctx.report("state", format!("{:?}", self.state));
        ctx.report("coins", self.collected);
        ctx.report("splashes", self.splashes);
        ctx.report("x", self.player.x.round());
        ctx.report("on_ground", self.on_ground);
    }

    /// The level's files changed (painted in Duck Forge Studio, or edited
    /// by hand while the game runs): read them again and carry on. The
    /// duck stays where it is, collected coins stay collected, new coins
    /// appear and removed ones go.
    fn reload(&mut self, _ctx: &mut Context, file: &std::path::Path) -> bool {
        if !self.map.reads_from(file) {
            return false;
        }
        let newer = match TileMap::load_level(LEVEL) {
            Ok(map) => map,
            Err(error) => {
                eprintln!("{error}"); // a mistake in the files: keep the old level
                return false;
            }
        };
        let coins = self.map.spawn_changes(&newer, "coin");
        self.coins.retain(|coin| !coins.removed.contains(coin));
        self.coins.extend(coins.added);
        if let Some(&flag) = newer.spawns("flag").first() {
            self.flag = flag;
        }
        self.map = newer;
        true
    }

    fn draw(&self, canvas: &mut Canvas) {
        let art = &self.art;
        canvas.clear(Color::from_hex(0x3b3f7a));
        // Far away first: the backgrounds, in screen pixels.
        canvas.draw_background(&art.sky, &self.camera);
        canvas.draw_background(&art.mountains, &self.camera);
        canvas.draw_background(&art.forest, &self.camera);

        // The world: everything in here moves with the camera.
        canvas.push_camera(&self.camera);
        self.map.draw(canvas, &art.tiles, self.time);
        for &coin in &self.coins {
            art.tile_sheet
                .draw(canvas, "coin", self.time, coin, Blit::default());
            // Labels name things for tools that can't see the picture:
            // --describe, scene files, and the F1 overlay.
            canvas.label("coin", DuckPeaks::coin_rect(coin));
        }
        art.tile_sheet
            .draw(canvas, "flag", self.time, self.flag, Blit::default());
        canvas.label("flag", DuckPeaks::coin_rect(self.flag));
        let animation = match (self.on_ground, self.vel) {
            (true, v) if v.x.abs() > 10.0 => "run",
            (true, _) => "idle",
            (false, v) if v.y < 0.0 => "jump",
            (false, _) => "fall",
        };
        let blit = Blit {
            flip_x: self.facing_left,
            ..Default::default()
        };
        // Drawn by its pivot (its feet), so it stands on the ground in
        // every animation.
        art.duck
            .draw(canvas, animation, self.time, self.feet(), blit);
        canvas.label("player", self.player);
        canvas.pop_camera();

        // The screen: the score stays put.
        let total = self.collected as usize + self.coins.len();
        canvas.draw_text(
            &format!("Coins {}/{total}", self.collected),
            vec2(4.0, 4.0),
            1,
            Color::WHITE,
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
                canvas.draw_text_centered("YOU MADE IT!", 100.0, 3, Color::YELLOW);
                canvas.draw_text_centered(
                    &format!("{} of {total} coins", self.collected),
                    126.0,
                    1,
                    Color::WHITE,
                );
            }
        }
    }
}

fn config() -> Config {
    Config {
        title: String::from("Duck Peaks - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let art = Art::load()?;
    duckforge::run(config(), DuckPeaks::new(art))
}

// Play tests: `cargo test --example duck_peaks`. The bot that records
// playthroughs/duck_peaks.json: `cargo test --example duck_peaks -- --ignored`.
#[cfg(test)]
mod tests;

#[cfg(test)]
mod bot;

//! Dodge: move left and right to dodge the falling blocks. They come
//! faster and faster; how long can you last?
//!
//! This game was written by following docs/AI_GUIDE.md, on a computer with
//! no screen, using only headless runs, contact sheets, log.json and play
//! tests. Later it got sprites drawn with letters, a camera that shakes when
//! you're hit, see-through layers and sound, all checked the same way.
//!
//! Run it with:   cargo run --example dodge
//! Controls:      Left/Right or A/D move, Space starts and restarts, Escape quits.
//! Without a screen, with pictures and sound:
//!     cargo run --no-default-features --example dodge -- --headless \
//!         --inputs playthroughs/dodge.json --sheet --gif --out dodge-run

use duckforge::prelude::*;

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;
const BACKGROUND: Color = Color::from_hex(0x10_14_24);

const PLAYER_SIZE: f32 = 12.0;
const PLAYER_Y: f32 = HEIGHT - 24.0;
const PLAYER_SPEED: f32 = 200.0;

const BLOCK_SIZE: f32 = 14.0;
const START_FALL_SPEED: f32 = 90.0;
const START_SPAWN_TIME: f32 = 0.8;
const START_LIVES: u32 = 3;
/// After a hit, the player can't be hit again for this long (and flickers).
const SAFE_TIME: f32 = 1.0;
/// Every this many blocks dodged, a little fanfare.
const MILESTONE: u32 = 10;

/// The player: a duck seen from above, two frames for waddling. Each
/// frame is 6 x 6 letters, drawn twice as big: exactly the 12 x 12 hitbox.
const DUCK: [&str; 6] = [
    "..yy....yy..",
    ".yyyyo.yyyyo",
    "yyyyy.yyyyy.",
    "yyyyy.yyyyy.",
    ".yyy...yyy..",
    "o..o...oo...",
];
/// A falling rock: 7 x 7 letters, twice as big is the 14 x 14 hitbox.
const ROCK: [&str; 7] = [
    "..rrr..", ".rRrrr.", "rRrrrrd", "rrrrrdd", "rrrrddd", ".rrddd.", "..ddd..",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Playing,
    GameOver,
}

/// The pictures, made once and shared by every game (cloning a sprite
/// doesn't copy its pixels).
struct Art {
    waddle: Animation,
    rock: Sprite,
}

impl Art {
    fn new() -> Art {
        let duck = Sprite::from_ascii(&DUCK, &[('y', Color::YELLOW), ('o', Color::ORANGE)]);
        let rock = Sprite::from_ascii(
            &ROCK,
            &[
                ('r', Color::from_hex(0xd04040)),
                ('R', Color::from_hex(0xf08080)), // the shiny bit
                ('d', Color::from_hex(0x802020)), // the shadowed side
            ],
        );
        Art {
            waddle: Animation::new(duck.frames(6, 6), 10.0),
            rock,
        }
    }
}

struct Dodge {
    state: State,
    player: Rect,
    blocks: Vec<Rect>,
    spawn_timer: f32,
    time: f32,
    lives: u32,
    /// Seconds of safety left after a hit.
    safe_time: f32,
    hits: u32,
    dodged: u32,
    /// Seconds spent moving, for the waddle.
    walk_time: f32,
    /// Shakes the screen on a hit. It never moves otherwise: a new camera
    /// shows the screen exactly as it is.
    camera: Camera2D,
    art: Art,
}

impl Dodge {
    fn new() -> Self {
        Self {
            state: State::Ready,
            player: Rect::new(
                (WIDTH - PLAYER_SIZE) / 2.0,
                PLAYER_Y,
                PLAYER_SIZE,
                PLAYER_SIZE,
            ),
            blocks: Vec::new(),
            spawn_timer: 0.0,
            time: 0.0,
            lives: START_LIVES,
            safe_time: 0.0,
            hits: 0,
            dodged: 0,
            walk_time: 0.0,
            camera: Camera2D::new(WIDTH, HEIGHT),
            art: Art::new(),
        }
    }

    fn fall_speed(&self) -> f32 {
        START_FALL_SPEED + self.time * 4.0
    }

    fn spawn_time(&self) -> f32 {
        START_SPAWN_TIME / (1.0 + self.time * 0.05)
    }

    fn play(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        self.time += dt;

        let direction = ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D);
        self.player.x += direction.clamp(-1.0, 1.0) * PLAYER_SPEED * dt;
        if direction != 0.0 {
            self.walk_time += dt;
        }
        self.player.x = self.player.x.clamp(0.0, WIDTH - PLAYER_SIZE);

        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            self.spawn_timer = self.spawn_time();
            let x = ctx.rng.range(0.0, WIDTH - BLOCK_SIZE);
            self.blocks
                .push(Rect::new(x, -BLOCK_SIZE, BLOCK_SIZE, BLOCK_SIZE));
        }

        let fall = self.fall_speed() * dt;
        for block in &mut self.blocks {
            block.y += fall;
        }

        self.safe_time = (self.safe_time - dt).max(0.0);
        let hit = self.blocks.iter().any(|block| block.overlaps(&self.player));
        if hit && self.safe_time == 0.0 {
            self.hits += 1;
            self.lives -= 1;
            self.safe_time = SAFE_TIME;
            if self.lives == 0 {
                self.state = State::GameOver;
                self.blocks.clear(); // keep the GAME OVER text readable
                self.camera.shake(6.0, 0.6);
                ctx.play(Sound::explosion());
                return;
            }
            self.camera.shake(3.0, 0.3);
            ctx.play(Sound::hit());
        }

        let before = self.blocks.len();
        self.blocks.retain(|block| block.top() < HEIGHT);
        let dodged_now = (before - self.blocks.len()) as u32;
        if dodged_now > 0 && (self.dodged + dodged_now) / MILESTONE > self.dodged / MILESTONE {
            ctx.play(Sound::coin());
        }
        self.dodged += dodged_now;

        // Outlines for the debug overlay (F1, or --debug when headless).
        ctx.debug_rect(self.player);
        for block in &self.blocks {
            ctx.debug_rect(*block);
        }
    }
}

impl Game for Dodge {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        match self.state {
            State::Ready => {
                if ctx.input.was_pressed(Key::Space) {
                    self.state = State::Playing;
                    ctx.beep(660.0, 80.0);
                }
            }
            State::Playing => self.play(ctx),
            State::GameOver => {
                if ctx.input.was_pressed(Key::Space) {
                    *self = Dodge::new();
                    self.state = State::Playing;
                    ctx.beep(660.0, 80.0);
                }
            }
        }
        self.camera.update(ctx);

        // For tests and log.json: things that matter, that change now and then.
        ctx.report("state", format!("{:?}", self.state));
        ctx.report("lives", self.lives);
        ctx.report("hits", self.hits);
        ctx.report("dodged", self.dodged);
        ctx.report("seconds", self.time.floor() as u32);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(BACKGROUND);

        // The playfield goes through the camera, so it shakes on a hit.
        canvas.push_camera(&self.camera);
        // The ground the duck stands on.
        canvas.fill_rect(
            Rect::new(-10.0, PLAYER_Y + PLAYER_SIZE, WIDTH + 20.0, 20.0),
            Color::from_hex(0x1c2a3c),
        );
        for block in &self.blocks {
            // A see-through shadow on the ground grows as the rock gets closer.
            let closeness = (block.bottom() / PLAYER_Y).clamp(0.0, 1.0);
            let shadow = Rect::new(
                block.x + 2.0,
                PLAYER_Y + PLAYER_SIZE + 1.0,
                BLOCK_SIZE - 4.0,
                3.0,
            );
            canvas.fill_rect(shadow, Color::BLACK.with_alpha((closeness * 140.0) as u8));
            canvas.blit_sprite(
                &self.art.rock,
                vec2(block.x, block.y),
                Blit {
                    scale: 2.0,
                    ..Default::default()
                },
            );
            canvas.label("rock", *block);
        }
        // Flicker while safe after a hit: half see-through 4 times a second.
        let faded = self.safe_time > 0.0 && (self.safe_time * 8.0) as u32 % 2 == 1;
        let duck = Blit {
            scale: 2.0,
            opacity: if faded { 0.3 } else { 1.0 },
            ..Default::default()
        };
        let frame = self.art.waddle.frame_at(self.walk_time);
        canvas.blit_sprite(frame, vec2(self.player.x, self.player.y), duck);
        canvas.label("duck", self.player);
        canvas.pop_camera();

        // The score sits on its own dark strip, so falling blocks can't hide
        // it. It's see-through, so you still glimpse rocks coming in.
        canvas.fill_rect(
            Rect::new(0.0, 0.0, WIDTH, 13.0),
            Color::BLACK.with_alpha(190),
        );
        canvas.draw_text(
            &format!("TIME {:.1}", self.time),
            vec2(4.0, 4.0),
            1,
            Color::WHITE,
        );
        let lives = format!("LIVES {}", self.lives);
        let x = WIDTH - 4.0 - Canvas::text_width(&lives, 1);
        canvas.draw_text(&lives, vec2(x, 4.0), 1, Color::WHITE);

        match self.state {
            State::Ready => {
                canvas.draw_text_centered("DODGE", 90.0, 4, Color::WHITE);
                canvas.draw_text_centered("press space to start", 130.0, 1, Color::GRAY);
            }
            State::GameOver => {
                canvas.fill_rect(
                    Rect::new(0.0, 80.0, WIDTH, 64.0),
                    Color::BLACK.with_alpha(150),
                );
                canvas.draw_text_centered("GAME OVER", 90.0, 3, Color::WHITE);
                canvas.draw_text_centered("press space to try again", 130.0, 1, Color::GRAY);
            }
            State::Playing => {}
        }
    }
}

fn config() -> Config {
    Config {
        title: String::from("Dodge - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Dodge::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Press Space on frame 10 to start, then do nothing.
    const START: &str = r#"{"events": [
        {"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"}
    ]}"#;

    #[test]
    fn it_waits_for_space() {
        let played = PlayTest::new(config(), Dodge::new()).frames(300).run();
        assert_eq!(played.text("state"), "Ready");
        assert_eq!(played.number("seconds"), 0.0);
    }

    #[test]
    fn standing_still_loses_within_half_a_minute() {
        let played = PlayTest::new(config(), Dodge::new())
            .script(START)
            .frames(30 * 60)
            .run();
        assert_eq!(played.text("state"), "GameOver");
        assert_eq!(played.number("hits"), 3.0);
        // ...but not straight away: there's time to learn the controls.
        // (`history` starts with the value on frame 1, `hits` = 0, so skip it.)
        let first_hit = played.history("hits").get(1).map(|&(frame, _)| frame);
        assert!(
            first_hit.is_some_and(|frame| frame > 3 * 60),
            "hit at {first_hit:?}"
        );
    }

    #[test]
    fn every_hit_sounds_and_the_last_one_explodes() {
        let played = PlayTest::new(config(), Dodge::new())
            .script(START)
            .frames(30 * 60)
            .run();
        played.assert_sound("beep", 10, 0); // the start
        let hit_frames: Vec<u64> = played
            .history("hits")
            .iter()
            .skip(1)
            .map(|&(f, _)| f)
            .collect();
        assert_eq!(hit_frames.len(), 3);
        // Two hits go "hit", the third (game over) goes "explosion", on the same frames.
        assert_eq!(played.sound_frames("hit"), hit_frames[..2]);
        assert_eq!(played.sound_frames("explosion"), hit_frames[2..]);
    }

    #[test]
    fn dodging_ten_blocks_plays_a_fanfare() {
        let played = PlayTest::new(config(), Dodge::new())
            .script(START)
            .frames(30 * 60)
            .run();
        let tenth = played
            .history("dodged")
            .iter()
            .find(|(_, dodged)| dodged.as_number().unwrap_or(0.0) >= MILESTONE as f64)
            .map(|&(frame, _)| frame)
            .expect("ten blocks fell past");
        played.assert_sound("coin", tenth, 0);
    }

    #[test]
    fn a_hit_gives_a_moment_of_safety() {
        // Standing still, hits come at least SAFE_TIME apart.
        let played = PlayTest::new(config(), Dodge::new())
            .script(START)
            .frames(30 * 60)
            .run();
        let hit_frames: Vec<u64> = played
            .history("hits")
            .iter()
            .skip(1) // frame 1: hits = 0
            .map(|&(frame, _)| frame)
            .collect();
        for pair in hit_frames.windows(2) {
            assert!(
                pair[1] - pair[0] >= (SAFE_TIME * 60.0) as u64,
                "{hit_frames:?}"
            );
        }
    }

    /// A simple player: step away from the nearest block that's about to
    /// land on you, otherwise drift back to the middle.
    fn careful_player(seconds: u64) -> Simulation<Dodge> {
        let mut sim = Simulation::new(config(), Dodge::new());
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        for _ in 0..seconds * 60 {
            let game = sim.game();
            let me = game.player;
            // Blocks about to land, or already level with the player (don't
            // walk back into those).
            let danger = game
                .blocks
                .iter()
                .filter(|block| block.top() < me.bottom() && block.bottom() > me.top() - 90.0)
                .filter(|block| block.right() > me.left() - 6.0 && block.left() < me.right() + 6.0)
                .max_by(|a, b| a.y.total_cmp(&b.y));
            let target = match danger {
                // Go to whichever side of the block has more room.
                Some(block) if block.center().x > WIDTH / 2.0 => block.left() - 30.0,
                Some(block) => block.right() + 30.0,
                None => WIDTH / 2.0,
            };
            let (left, right) = (target < me.center().x - 3.0, target > me.center().x + 3.0);
            for (key, held) in [(Key::Left, left), (Key::Right, right)] {
                if held {
                    sim.press(key);
                } else {
                    sim.release(key);
                }
            }
            sim.step();
        }
        sim
    }

    /// Makes `playthroughs/dodge.json`: the careful player plays for 20
    /// seconds, then stops moving until the game is over. Run it with
    /// `cargo test --example dodge -- --ignored record`.
    #[test]
    #[ignore]
    fn record_playthrough() {
        let mut sim = careful_player(20);
        sim.release(Key::Left);
        sim.release(Key::Right);
        while sim.text("state") != "GameOver" {
            sim.step();
        }
        sim.run(60); // a second of GAME OVER at the end
        sim.recording().save("playthroughs/dodge.json").unwrap();
    }

    #[test]
    fn the_recorded_playthrough_still_plays_the_same() {
        let played = PlayTest::new(config(), Dodge::new())
            .inputs("playthroughs/dodge.json")
            .run();
        assert_eq!(played.text("state"), "GameOver");
        assert!(
            played.number("dodged") >= 20.0,
            "it dodged for a while first"
        );
        assert_eq!(played.sound_frames("explosion").len(), 1);
    }

    #[test]
    fn a_careful_player_can_survive_a_minute() {
        let sim = careful_player(60);
        assert_eq!(
            sim.text("state"),
            "Playing",
            "the game is too hard to survive"
        );
        assert!(sim.number("dodged") > 60.0);
    }

    #[test]
    fn it_gets_harder_over_time() {
        let sim = careful_player(60);
        let game = sim.game();
        assert!(game.fall_speed() > 1.5 * START_FALL_SPEED);
        assert!(game.spawn_time() < 0.5 * START_SPAWN_TIME);
    }
}

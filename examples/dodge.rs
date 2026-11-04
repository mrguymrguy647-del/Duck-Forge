//! Dodge: move left and right to dodge the falling blocks. They come
//! faster and faster; how long can you last?
//!
//! This game was written by following docs/AI_GUIDE.md, on a computer with
//! no screen, using only headless runs, contact sheets, log.json and play
//! tests. It's a good template for a new game.
//!
//! Run it with:   cargo run --example dodge
//! Controls:      Left/Right or A/D move, Space starts and restarts, Escape quits.

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
/// After a hit, the player can't be hit again for this long (and blinks).
const SAFE_TIME: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Playing,
    GameOver,
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
                return;
            }
        }

        let before = self.blocks.len();
        self.blocks.retain(|block| block.top() < HEIGHT);
        self.dodged += (before - self.blocks.len()) as u32;
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
                }
            }
            State::Playing => self.play(ctx),
            State::GameOver => {
                if ctx.input.was_pressed(Key::Space) {
                    *self = Dodge::new();
                    self.state = State::Playing;
                }
            }
        }

        // For tests and log.json: things that matter, that change now and then.
        ctx.report("state", format!("{:?}", self.state));
        ctx.report("lives", self.lives);
        ctx.report("hits", self.hits);
        ctx.report("dodged", self.dodged);
        ctx.report("seconds", self.time.floor() as u32);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(BACKGROUND);
        for block in &self.blocks {
            canvas.fill_rect(*block, Color::RED);
        }
        // Blink while safe after a hit: visible 4 times a second.
        let blink_off = self.safe_time > 0.0 && (self.safe_time * 8.0) as u32 % 2 == 1;
        if !blink_off {
            canvas.fill_rect(self.player, Color::YELLOW);
        }

        // The score sits on its own dark strip, so falling blocks can't hide it.
        canvas.fill_rect(Rect::new(0.0, 0.0, WIDTH, 13.0), Color::BLACK);
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

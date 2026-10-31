//! Breakout: a complete little game built with duckforge.
//!
//! Run it with:   cargo run --release --example breakout
//! Controls:      Left/Right or A/D move the paddle, Space launches the ball,
//!                P pauses, R restarts, F12 saves a screenshot, Escape quits.

use std::f32::consts::TAU;

use duckforge::prelude::*;

// ---- Tuning knobs. Change these and see what happens! ----

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;
const BACKGROUND: Color = Color::from_hex(0x14_14_28);

const PADDLE_WIDTH: f32 = 48.0;
const PADDLE_HEIGHT: f32 = 6.0;
const PADDLE_Y: f32 = 222.0;
const PADDLE_SPEED: f32 = 240.0;

const BALL_SIZE: f32 = 5.0;
const BALL_START_SPEED: f32 = 150.0;
const BALL_MAX_SPEED: f32 = 280.0;
const BALL_SPEEDUP_PER_BRICK: f32 = 3.0;
/// How steeply the ball leaves the paddle when it hits the very edge (in radians, about 57 degrees).
const MAX_BOUNCE_ANGLE: f32 = 1.0;

const BRICK_COLUMNS: usize = 10;
const BRICK_WIDTH: f32 = 28.0;
const BRICK_HEIGHT: f32 = 10.0;
const BRICK_GAP: f32 = 2.0;
const BRICK_TOP: f32 = 24.0;
/// One color per row of bricks, top to bottom. Add a color to add a row.
const ROW_COLORS: [Color; 6] = [
    Color::RED,
    Color::ORANGE,
    Color::YELLOW,
    Color::GREEN,
    Color::BLUE,
    Color::PURPLE,
];

const START_LIVES: u32 = 3;
const PARTICLES_PER_BRICK: usize = 12;
const PARTICLE_LIFE: f32 = 0.8;
const GRAVITY: f32 = 300.0;

/// The game is always in exactly one of these states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// The ball sits on the paddle, waiting for Space.
    Serving,
    Playing,
    Paused,
    GameOver,
    Won,
}

struct Brick {
    rect: Rect,
    color: Color,
    points: u32,
}

/// A little spark flying off a broken brick. Purely for looks.
struct Particle {
    position: Vec2,
    velocity: Vec2,
    /// Seconds left before it disappears.
    life: f32,
    color: Color,
}

/// The whole state of the game.
struct Breakout {
    state: State,
    paddle: Rect,
    ball: Rect,
    ball_velocity: Vec2,
    ball_speed: f32,
    bricks: Vec<Brick>,
    particles: Vec<Particle>,
    score: u32,
    lives: u32,
}

impl Breakout {
    fn new() -> Self {
        let mut game = Self {
            state: State::Serving,
            paddle: Rect::new(
                (WIDTH - PADDLE_WIDTH) / 2.0,
                PADDLE_Y,
                PADDLE_WIDTH,
                PADDLE_HEIGHT,
            ),
            ball: Rect::new(0.0, 0.0, BALL_SIZE, BALL_SIZE),
            ball_velocity: Vec2::ZERO,
            ball_speed: BALL_START_SPEED,
            bricks: build_bricks(),
            particles: Vec::new(),
            score: 0,
            lives: START_LIVES,
        };
        game.put_ball_on_paddle();
        game
    }

    fn put_ball_on_paddle(&mut self) {
        self.ball.x = self.paddle.center().x - BALL_SIZE / 2.0;
        self.ball.y = self.paddle.top() - BALL_SIZE - 1.0;
    }

    fn move_paddle(&mut self, ctx: &Context) {
        let direction = ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D);
        self.paddle.x += direction.clamp(-1.0, 1.0) * PADDLE_SPEED * ctx.dt();
        self.paddle.x = self.paddle.x.clamp(0.0, WIDTH - PADDLE_WIDTH);
    }

    fn launch_ball(&mut self, rng: &mut Rng) {
        // Straight up, tilted by a random angle so no two serves are the same.
        let angle = rng.range(-0.5, 0.5);
        self.ball_velocity = direction_from_angle(angle) * self.ball_speed;
        self.state = State::Playing;
    }

    fn update_ball(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();

        // Move one axis at a time. If the ball hits something while moving
        // sideways we flip the x velocity; while moving up/down, the y velocity.
        let step = self.ball_velocity.x * dt;
        self.ball.x += step;
        let hit_wall = self.ball.left() < 0.0 || self.ball.right() > WIDTH;
        if hit_wall || self.break_brick_under_ball(&mut ctx.rng) {
            self.ball.x -= step; // undo the move...
            self.ball_velocity.x = -self.ball_velocity.x; // ...and bounce
        }

        let step = self.ball_velocity.y * dt;
        self.ball.y += step;
        let hit_ceiling = self.ball.top() < 0.0;
        if hit_ceiling || self.break_brick_under_ball(&mut ctx.rng) {
            self.ball.y -= step;
            self.ball_velocity.y = -self.ball_velocity.y;
        }

        // Only bounce while the ball is moving down, so it can never get stuck in the paddle.
        if self.ball_velocity.y > 0.0 && self.ball.overlaps(&self.paddle) {
            self.bounce_off_paddle();
        }

        if self.ball.top() > HEIGHT {
            self.lose_life();
        } else if self.bricks.is_empty() {
            self.state = State::Won;
        }
    }

    fn bounce_off_paddle(&mut self) {
        // Where along the paddle did the ball land? -1.0 = left edge, 0.0 = middle, 1.0 = right edge.
        let offset = (self.ball.center().x - self.paddle.center().x) / (PADDLE_WIDTH / 2.0);
        // The further from the middle, the steeper the bounce. This is what lets the player aim.
        let angle = offset.clamp(-1.0, 1.0) * MAX_BOUNCE_ANGLE;
        self.ball_velocity = direction_from_angle(angle) * self.ball_speed;
        self.ball.y = self.paddle.top() - BALL_SIZE;
    }

    /// Breaks the first brick the ball overlaps. Returns `true` if a brick was hit.
    fn break_brick_under_ball(&mut self, rng: &mut Rng) -> bool {
        // `position` gives `Some(index)` of the first brick that matches, or `None`.
        let Some(index) = self
            .bricks
            .iter()
            .position(|brick| brick.rect.overlaps(&self.ball))
        else {
            return false;
        };

        // `remove` takes the brick out of the Vec and hands us ownership of it.
        let brick = self.bricks.remove(index);
        self.score += brick.points;
        self.ball_speed = (self.ball_speed + BALL_SPEEDUP_PER_BRICK).min(BALL_MAX_SPEED);
        self.ball_velocity = self.ball_velocity.normalized() * self.ball_speed;
        self.spawn_particles(brick.rect.center(), brick.color, rng);
        true
    }

    fn lose_life(&mut self) {
        self.lives -= 1;
        if self.lives == 0 {
            self.state = State::GameOver;
        } else {
            self.state = State::Serving;
            self.ball_speed = BALL_START_SPEED;
        }
    }

    fn spawn_particles(&mut self, at: Vec2, color: Color, rng: &mut Rng) {
        for _ in 0..PARTICLES_PER_BRICK {
            let angle = rng.range(0.0, TAU);
            let speed = rng.range(30.0, 120.0);
            self.particles.push(Particle {
                position: at,
                velocity: vec2(angle.cos(), angle.sin()) * speed,
                life: rng.range(0.3, PARTICLE_LIFE),
                color,
            });
        }
    }

    fn update_particles(&mut self, dt: f32) {
        for particle in &mut self.particles {
            particle.velocity.y += GRAVITY * dt;
            particle.position += particle.velocity * dt;
            particle.life -= dt;
        }
        // Throw away the particles whose time is up.
        self.particles.retain(|particle| particle.life > 0.0);
    }

    fn draw_hud(&self, canvas: &mut Canvas) {
        canvas.draw_text(
            &format!("SCORE {}", self.score),
            vec2(4.0, 4.0),
            1,
            Color::WHITE,
        );
        let lives = format!("LIVES {}", self.lives);
        let x = WIDTH - 4.0 - Canvas::text_width(&lives, 1);
        canvas.draw_text(&lives, vec2(x, 4.0), 1, Color::WHITE);

        let (title, hint) = match self.state {
            State::Playing => return, // nothing to show while playing
            State::Serving => ("", "PRESS SPACE TO LAUNCH"),
            State::Paused => ("PAUSED", "PRESS P TO CONTINUE"),
            State::GameOver => ("GAME OVER", "PRESS SPACE TO TRY AGAIN"),
            State::Won => ("YOU WIN!", "PRESS SPACE TO PLAY AGAIN"),
        };
        canvas.draw_text_centered(title, 120.0, 3, Color::WHITE);
        canvas.draw_text_centered(hint, 145.0, 1, Color::GRAY);
    }
}

/// A unit vector pointing up the screen, tilted right by `angle` radians
/// (or left, if the angle is negative).
fn direction_from_angle(angle: f32) -> Vec2 {
    // Screen y grows downward, so "up" is negative y.
    vec2(angle.sin(), -angle.cos())
}

fn build_bricks() -> Vec<Brick> {
    let rows = ROW_COLORS.len();
    let total_width = BRICK_COLUMNS as f32 * (BRICK_WIDTH + BRICK_GAP) - BRICK_GAP;
    let left = (WIDTH - total_width) / 2.0;

    let mut bricks = Vec::with_capacity(rows * BRICK_COLUMNS);
    for (row, &color) in ROW_COLORS.iter().enumerate() {
        for column in 0..BRICK_COLUMNS {
            let x = left + column as f32 * (BRICK_WIDTH + BRICK_GAP);
            let y = BRICK_TOP + row as f32 * (BRICK_HEIGHT + BRICK_GAP);
            bricks.push(Brick {
                rect: Rect::new(x, y, BRICK_WIDTH, BRICK_HEIGHT),
                color,
                points: ((rows - row) * 10) as u32, // higher rows are worth more
            });
        }
    }
    bricks
}

impl Game for Breakout {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.was_pressed(Key::R) {
            *self = Breakout::new(); // swap the whole game for a fresh one
            return;
        }

        match self.state {
            State::Serving => {
                self.move_paddle(ctx);
                self.put_ball_on_paddle();
                if ctx.input.was_pressed(Key::Space) {
                    self.launch_ball(&mut ctx.rng);
                }
            }
            State::Playing => {
                if ctx.input.was_pressed(Key::P) {
                    self.state = State::Paused;
                } else {
                    self.move_paddle(ctx);
                    self.update_ball(ctx);
                }
            }
            State::Paused => {
                if ctx.input.was_pressed(Key::P) {
                    self.state = State::Playing;
                }
            }
            State::GameOver | State::Won => {
                if ctx.input.was_pressed(Key::Space) || ctx.input.was_pressed(Key::Enter) {
                    *self = Breakout::new();
                }
            }
        }

        if self.state != State::Paused {
            self.update_particles(ctx.dt());
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(BACKGROUND);

        for brick in &self.bricks {
            canvas.fill_rect(brick.rect, brick.color);
            // A lighter strip along the top edge makes the bricks look less flat.
            let highlight = Rect::new(brick.rect.x, brick.rect.y, brick.rect.w, 2.0);
            canvas.fill_rect(highlight, brick.color.lerp(Color::WHITE, 0.35));
        }

        for particle in &self.particles {
            // Fade from the brick's color into the background as the particle dies.
            let color = BACKGROUND.lerp(particle.color, particle.life / PARTICLE_LIFE);
            canvas.fill_rect(Rect::from_center(particle.position, 2.0, 2.0), color);
        }

        canvas.fill_rect(self.paddle, Color::WHITE);
        canvas.fill_rect(self.ball, Color::WHITE);

        self.draw_hud(canvas);
    }
}

fn config() -> Config {
    Config {
        title: String::from("Breakout - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Breakout::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays a short game: serve, steer right for a while, then left, then
    /// wait. Returns the simulation, ten seconds of game time later.
    fn play(seed: u32) -> Simulation<Breakout> {
        let config = Config {
            seed: Some(seed),
            ..config()
        };
        let mut sim = Simulation::new(config, Breakout::new());
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        sim.press(Key::Right);
        sim.run(40);
        sim.release(Key::Right);
        sim.press(Key::Left);
        sim.run(90);
        sim.release(Key::Left);
        sim.run(469); // 600 frames in all
        sim
    }

    /// How many pixels differ between two pictures of the same size.
    fn pixels_that_differ(a: &Canvas, b: &Canvas) -> usize {
        a.pixels()
            .iter()
            .zip(b.pixels())
            .filter(|(x, y)| x != y)
            .count()
    }

    #[test]
    fn the_same_seed_and_inputs_give_the_same_game() {
        let mut first = play(7);
        let mut second = play(7);
        assert_eq!(first.context().frame(), 600);
        let differ = pixels_that_differ(first.picture(), second.picture());
        assert_eq!(differ, 0, "{differ} pixels are different in the last frame");
        assert_eq!(first.game().score, second.game().score);
    }

    #[test]
    fn a_different_seed_gives_a_different_game() {
        // The serve angle is random, so another seed sends the ball elsewhere.
        let differ = pixels_that_differ(play(7).picture(), play(8).picture());
        assert!(differ > 0);
    }
}

//! Duckling Parade: lead the lost ducklings home before the pike get them.
//!
//! Run it with:   cargo run --release --example duckling_parade
//! Controls:      Arrow keys or WASD swim, Enter or Space starts,
//!                P pauses, R restarts, F12 saves a screenshot, Escape quits.
//!
//! Swim into a lost duckling and it joins the line behind you. Bring the line
//! to the nest to save them. A longer line scores much more (1 + 2 + 3 + ...),
//! but every duckling slows you down a little, and a pike that bites the line
//! eats one duckling and scatters the rest. A dark shadow and an arrow show
//! where a pike is about to strike, and which way it will go.

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use duckforge::prelude::*;

// ---- Tuning knobs. Change these and see what happens! ----

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;

/// The water. The duck can't leave it.
const POND: Rect = Rect::new(10.0, 24.0, 300.0, 204.0);
/// The nest sits on the bottom shore, half in the water.
const NEST: Vec2 = vec2(160.0, 228.0);
const NEST_RADIUS: f32 = 15.0;

const DUCK_RADIUS: f32 = 6.0;
const DUCK_SPEED: f32 = 100.0;
/// How quickly the duck reaches the speed you steer it to (higher = snappier).
const DUCK_GRIP: f32 = 8.0;
/// Each duckling in the line slows the duck by this fraction...
const SLOWDOWN_PER_DUCKLING: f32 = 0.025;
/// ...but never below this fraction of full speed.
const MIN_SPEED_FACTOR: f32 = 0.72;
/// Seconds the duck can't be hurt after a bite (it blinks).
const HURT_TIME: f32 = 2.0;

const DUCKLING_RADIUS: f32 = 3.5;
const MAX_LINE: usize = 12;
/// The duck's path is remembered as points this far apart...
const TRAIL_STEP: f32 = 1.5;
/// ...and each duckling swims this many points behind the one in front.
const FOLLOW_GAP: usize = 6;
/// Seconds a scattered duckling panics before it can be picked up again.
const PANIC_TIME: f32 = 0.8;

const PIKE_LENGTH: f32 = 28.0;
const PIKE_RADIUS: f32 = 4.5;
/// Seconds of warning (shadow and arrow) before a pike strikes.
const PIKE_WARNING: f32 = 1.1;

const START_LIVES: u32 = 3;
const MAX_LIVES: u32 = 5;
const LEVEL_BANNER_TIME: f32 = 2.2;

// ---- Colors ----

const GRASS: Color = Color::from_hex(0x5d9c46);
const GRASS_DARK: Color = Color::from_hex(0x467f36);
const MUD: Color = Color::from_hex(0x6f5a38);
const WATER_TOP: Color = Color::from_hex(0x3a8cc0);
const WATER_BOTTOM: Color = Color::from_hex(0x23629a);
const WATER_MID: Color = Color::from_hex(0x2e77ad);
const SPARKLE: Color = Color::from_hex(0x8cc8ea);
const DUCK_WHITE: Color = Color::from_hex(0xf6f3ea);
const DUCK_SHADE: Color = Color::from_hex(0xd8d2c2);
const DUCK_OUTLINE: Color = Color::from_hex(0x6d7a86);
const BEAK: Color = Color::from_hex(0xf39a2b);
const CHICK: Color = Color::from_hex(0xffd94a);
const CHICK_OUTLINE: Color = Color::from_hex(0xa87916);
const PIKE_BODY: Color = Color::from_hex(0x46623c);
const PIKE_BELLY: Color = Color::from_hex(0x7f9a5a);
const PIKE_OUTLINE: Color = Color::from_hex(0x1f3324);
const PIKE_SHADOW: Color = Color::from_hex(0x1b4a75);
const WARNING: Color = Color::from_hex(0xff7a5c);
const NEST_OUTER: Color = Color::from_hex(0x8a5a2b);
const NEST_INNER: Color = Color::from_hex(0xb47d3e);
const NEST_HOLLOW: Color = Color::from_hex(0x65401d);
const STRAW: Color = Color::from_hex(0xe0b267);
const HEART: Color = Color::from_hex(0xf0506e);
const SHADOW: Color = Color::from_hex(0x1c2b3a);

/// The game is always in exactly one of these states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Title,
    Playing,
    Paused,
    GameOver,
}

struct Duck {
    pos: Vec2,
    vel: Vec2,
    /// The way the duck faces, in radians (0 = right, PI/2 = down).
    angle: f32,
    /// Seconds of blinking left after being bitten.
    hurt: f32,
}

/// A duckling swimming in the line behind the duck.
struct Follower {
    pos: Vec2,
    angle: f32,
}

/// A duckling alone on the pond, waiting to be found.
struct Lost {
    pos: Vec2,
    vel: Vec2,
    angle: f32,
    /// Seconds of panic left; it can't be picked up until this reaches 0.
    panic: f32,
    /// Counts down to the next "peep" ripple.
    call: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PikePhase {
    /// A shadow under the water. The number is seconds left until it strikes.
    Warning(f32),
    Swimming,
    /// Going back under. The number is seconds left until it's gone.
    Diving(f32),
}

struct Pike {
    head: Vec2,
    angle: f32,
    speed: f32,
    phase: PikePhase,
    travelled: f32,
    max_travel: f32,
    /// Radians per second it can turn toward its prey (0 = straight line).
    turn_rate: f32,
}

impl Pike {
    fn dir(&self) -> Vec2 {
        vec2(self.angle.cos(), self.angle.sin())
    }

    fn tail(&self) -> Vec2 {
        self.head - self.dir() * PIKE_LENGTH
    }

    /// Does a circle at `p` with radius `r` touch this pike's body?
    fn touches(&self, p: Vec2, r: f32) -> bool {
        self.phase == PikePhase::Swimming
            && distance_to_segment(p, self.tail(), self.head) < r + PIKE_RADIUS
    }
}

/// An expanding ring on the water. Purely for looks.
struct Ripple {
    pos: Vec2,
    radius: f32,
    grow: f32,
    life: f32,
    max_life: f32,
    color: Color,
}

/// A feather or a drop of water. Purely for looks.
struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: f32,
    max_life: f32,
    color: Color,
    size: f32,
}

/// Floating text, like "+60".
struct Popup {
    pos: Vec2,
    text: String,
    life: f32,
    color: Color,
}

/// A saved duckling hopping from the line into the nest.
struct Hop {
    from: Vec2,
    /// Seconds since the hop started; negative means it is still waiting its turn.
    t: f32,
}

const HOP_TIME: f32 = 0.35;

/// The whole state of the game.
struct Parade {
    state: State,
    time: f32,
    duck: Duck,
    /// Where the duck has been, newest first, points `TRAIL_STEP` apart.
    trail: VecDeque<Vec2>,
    line: Vec<Follower>,
    lost: Vec<Lost>,
    pikes: Vec<Pike>,
    ripples: Vec<Ripple>,
    particles: Vec<Particle>,
    popups: Vec<Popup>,
    hops: Vec<Hop>,

    score: u32,
    best: u32,
    lives: u32,
    level: u32,
    saved_this_level: u32,
    saved_total: u32,
    eaten: u32,
    longest_line: usize,
    /// Ducklings sitting in the nest right now (drawn there).
    in_nest: u32,

    pike_timer: f32,
    lost_timer: f32,
    wake_timer: f32,
    /// Seconds left of the "LEVEL N" banner. No pike strike while it shows.
    banner: f32,
    /// Seconds of screen shake left.
    shake: f32,

    lily_pads: Vec<(Vec2, f32, f32)>,
    sparkles: Vec<Vec2>,
}

impl Parade {
    fn new(best: u32) -> Self {
        // The scenery comes from its own fixed seed, so the pond always looks
        // the same and the game's random numbers aren't used up.
        let mut scenery = Rng::new(2024);
        let mut lily_pads: Vec<(Vec2, f32, f32)> = Vec::new();
        while lily_pads.len() < 6 {
            let pos = vec2(
                scenery.range(POND.x + 16.0, POND.right() - 16.0),
                scenery.range(POND.y + 16.0, POND.bottom() - 26.0),
            );
            let crowded = lily_pads.iter().any(|(p, _, _)| p.distance(pos) < 40.0);
            if !crowded && pos.distance(NEST) > 50.0 && pos.distance(start_spot()) > 30.0 {
                lily_pads.push((pos, scenery.range(6.0, 10.0), scenery.range(0.0, TAU)));
            }
        }
        let sparkles = (0..28)
            .map(|_| {
                vec2(
                    scenery.range(POND.x + 6.0, POND.right() - 8.0),
                    scenery.range(POND.y + 4.0, POND.bottom() - 4.0),
                )
            })
            .collect();

        Self {
            state: State::Title,
            time: 0.0,
            duck: Duck {
                pos: start_spot(),
                vel: Vec2::ZERO,
                angle: -PI / 2.0,
                hurt: 0.0,
            },
            trail: VecDeque::new(),
            line: Vec::new(),
            lost: Vec::new(),
            pikes: Vec::new(),
            ripples: Vec::new(),
            particles: Vec::new(),
            popups: Vec::new(),
            hops: Vec::new(),
            score: 0,
            best,
            lives: START_LIVES,
            level: 1,
            saved_this_level: 0,
            saved_total: 0,
            eaten: 0,
            longest_line: 0,
            in_nest: 0,
            pike_timer: 2.5,
            lost_timer: 0.0,
            wake_timer: 0.0,
            banner: 0.0,
            shake: 0.0,
            lily_pads,
            sparkles,
        }
    }

    /// Starts a new game from the title screen.
    fn start(&mut self) {
        let best = self.best;
        *self = Parade::new(best);
        self.state = State::Playing;
        self.banner = LEVEL_BANNER_TIME;
    }

    // ---- Rules of the level ----

    /// Ducklings to save to finish the current level.
    fn goal(&self) -> u32 {
        4 + 2 * self.level
    }

    fn max_pikes(&self) -> usize {
        (1 + (self.level as usize).div_ceil(2)).min(5)
    }

    fn pike_interval(&self) -> f32 {
        (3.2 - 0.35 * self.level as f32).max(1.0)
    }

    fn max_lost(&self) -> usize {
        (3 + self.level as usize / 2).min(6)
    }

    /// Points for bringing `n` ducklings home at once: 10 + 20 + 30 + ...
    fn points_for(n: usize) -> u32 {
        let n = n as u32;
        10 * n * (n + 1) / 2
    }

    // ---- The duck and its line ----

    fn swim(&mut self, ctx: &Context) {
        let dt = ctx.dt();
        let input = ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D);
        let input_y = ctx.input.axis(Key::Up, Key::Down) + ctx.input.axis(Key::W, Key::S);
        let steer = vec2(input.clamp(-1.0, 1.0), input_y.clamp(-1.0, 1.0)).normalized();
        let factor = (1.0 - SLOWDOWN_PER_DUCKLING * self.line.len() as f32).max(MIN_SPEED_FACTOR);
        self.steer_duck(steer * (DUCK_SPEED * factor), dt);
    }

    /// Moves the duck toward `target_vel`, keeps it in the pond, and records its path.
    fn steer_duck(&mut self, target_vel: Vec2, dt: f32) {
        let duck = &mut self.duck;
        duck.vel += (target_vel - duck.vel) * (DUCK_GRIP * dt).min(1.0);
        duck.pos += duck.vel * dt;

        let r = DUCK_RADIUS;
        if duck.pos.x < POND.x + r || duck.pos.x > POND.right() - r {
            duck.vel.x = 0.0;
        }
        if duck.pos.y < POND.y + r || duck.pos.y > POND.bottom() - r {
            duck.vel.y = 0.0;
        }
        duck.pos.x = duck.pos.x.clamp(POND.x + r, POND.right() - r);
        duck.pos.y = duck.pos.y.clamp(POND.y + r, POND.bottom() - r);

        if duck.vel.length() > 8.0 {
            duck.angle = turn_toward(duck.angle, duck.vel.y.atan2(duck.vel.x), 12.0 * dt);
        }
        duck.hurt = (duck.hurt - dt).max(0.0);

        // Remember the path so the ducklings can follow it exactly.
        let moved = self
            .trail
            .front()
            .is_none_or(|last| last.distance(duck.pos) >= TRAIL_STEP);
        if moved {
            self.trail.push_front(duck.pos);
            self.trail.truncate((MAX_LINE + 2) * FOLLOW_GAP + 4);
        }

        // A wake behind the duck while it swims.
        self.wake_timer -= dt;
        if duck.vel.length() > 30.0 && self.wake_timer <= 0.0 {
            self.wake_timer = 0.14;
            let back = duck.pos - vec2(duck.angle.cos(), duck.angle.sin()) * 8.0;
            self.ripples.push(Ripple {
                pos: back,
                radius: 2.0,
                grow: 14.0,
                life: 0.6,
                max_life: 0.6,
                color: SPARKLE,
            });
        }
    }

    /// Moves each duckling in the line toward its spot on the duck's path.
    fn follow(&mut self, dt: f32) {
        for (i, follower) in self.line.iter_mut().enumerate() {
            let index = (i + 1) * FOLLOW_GAP + 1;
            let Some(target) = self.trail.get(index).or_else(|| self.trail.back()).copied() else {
                continue;
            };
            let step = target - follower.pos;
            if step.length() > 0.3 {
                follower.angle = turn_toward(follower.angle, step.y.atan2(step.x), 14.0 * dt);
            }
            follower.pos += step * (14.0 * dt).min(1.0);
        }
    }

    /// Ducklings the duck swims into join the end of the line.
    fn pick_up(&mut self) {
        let duck = self.duck.pos;
        let mut i = 0;
        while i < self.lost.len() {
            let lost = &self.lost[i];
            let close = lost.pos.distance(duck) < DUCK_RADIUS + DUCKLING_RADIUS + 3.0;
            if close && lost.panic <= 0.0 && self.line.len() < MAX_LINE {
                let lost = self.lost.swap_remove(i);
                self.line.push(Follower {
                    pos: lost.pos,
                    angle: lost.angle,
                });
                self.longest_line = self.longest_line.max(self.line.len());
                self.ripples.push(Ripple {
                    pos: lost.pos,
                    radius: 3.0,
                    grow: 30.0,
                    life: 0.4,
                    max_life: 0.4,
                    color: CHICK,
                });
            } else {
                i += 1;
            }
        }
    }

    /// Swimming into the nest saves the whole line.
    fn bring_home(&mut self) {
        if self.line.is_empty() || self.duck.pos.distance(NEST) > NEST_RADIUS + DUCK_RADIUS {
            return;
        }
        let n = self.line.len();
        let points = Parade::points_for(n);
        self.score += points;
        self.best = self.best.max(self.score);
        self.saved_this_level += n as u32;
        self.saved_total += n as u32;
        for (i, follower) in self.line.drain(..).enumerate() {
            self.hops.push(Hop {
                from: follower.pos,
                t: -0.08 * i as f32,
            });
        }
        let text = if n >= 5 {
            format!("PARADE X{n}! +{points}")
        } else {
            format!("+{points}")
        };
        self.popups.push(Popup {
            pos: NEST - vec2(Canvas::text_width(&text, 1) / 2.0, 26.0),
            text,
            life: 1.4,
            color: Color::YELLOW,
        });

        if self.saved_this_level >= self.goal() {
            self.level_up();
        }
    }

    fn level_up(&mut self) {
        self.level += 1;
        self.saved_this_level = 0;
        // Every second level gives back a life.
        if self.level % 2 == 1 {
            self.lives = (self.lives + 1).min(MAX_LIVES);
        }
        self.banner = LEVEL_BANNER_TIME;
        for pike in &mut self.pikes {
            if !matches!(pike.phase, PikePhase::Diving(_)) {
                pike.phase = PikePhase::Diving(0.4);
            }
        }
    }

    /// Turns the line from `from` onward back into lost, panicking ducklings.
    fn scatter(&mut self, from: usize, away_from: Vec2, rng: &mut Rng) {
        for follower in self.line.drain(from..) {
            let away = (follower.pos - away_from).normalized();
            let jitter = vec2(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0));
            self.lost.push(Lost {
                pos: follower.pos,
                vel: (away + jitter * 0.6).normalized() * rng.range(60.0, 90.0),
                angle: follower.angle,
                panic: PANIC_TIME,
                call: rng.range(0.2, 1.0),
            });
        }
    }

    // ---- Lost ducklings ----

    fn update_lost(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        for lost in &mut self.lost {
            lost.panic = (lost.panic - dt).max(0.0);
            // Paddle about slowly, turning a little at random.
            lost.angle += ctx.rng.range(-3.0, 3.0) * dt;
            let paddle = vec2(lost.angle.cos(), lost.angle.sin()) * 10.0;
            lost.vel += (paddle - lost.vel) * (2.5 * dt).min(1.0);
            lost.pos += lost.vel * dt;
            // Bounce off the shore and keep clear of the nest.
            let r = DUCKLING_RADIUS + 4.0;
            if lost.pos.x < POND.x + r || lost.pos.x > POND.right() - r {
                lost.vel.x = -lost.vel.x;
                lost.angle = PI - lost.angle;
            }
            if lost.pos.y < POND.y + r || lost.pos.y > POND.bottom() - r - 8.0 {
                lost.vel.y = -lost.vel.y;
                lost.angle = -lost.angle;
            }
            lost.pos.x = lost.pos.x.clamp(POND.x + r, POND.right() - r);
            lost.pos.y = lost.pos.y.clamp(POND.y + r, POND.bottom() - r - 8.0);

            lost.call -= dt;
            if lost.call <= 0.0 {
                lost.call = 1.6;
                self.ripples.push(Ripple {
                    pos: lost.pos,
                    radius: 4.0,
                    grow: 16.0,
                    life: 0.7,
                    max_life: 0.7,
                    color: CHICK,
                });
            }
        }

        // Keep a few lost ducklings on the pond.
        self.lost_timer -= dt;
        if self.lost.len() < self.max_lost() && self.lost_timer <= 0.0 {
            self.lost_timer = 1.0;
            for _ in 0..30 {
                let pos = vec2(
                    ctx.rng.range(POND.x + 18.0, POND.right() - 18.0),
                    ctx.rng.range(POND.y + 18.0, POND.bottom() - 30.0),
                );
                let clear = pos.distance(self.duck.pos) > 45.0
                    && pos.distance(NEST) > 40.0
                    && self.lost.iter().all(|l| l.pos.distance(pos) > 20.0);
                if clear {
                    self.lost.push(Lost {
                        pos,
                        vel: Vec2::ZERO,
                        angle: ctx.rng.range(0.0, TAU),
                        panic: 0.0,
                        call: 0.0,
                    });
                    break;
                }
            }
        }
    }

    // ---- Pike ----

    fn spawn_pike(&mut self, rng: &mut Rng) {
        for _ in 0..40 {
            let start = vec2(
                rng.range(POND.x + 24.0, POND.right() - 24.0),
                rng.range(POND.y + 24.0, POND.bottom() - 24.0),
            );
            if start.distance(self.duck.pos) < 65.0 || start.distance(NEST) < 30.0 {
                continue;
            }
            // Aim roughly at the duck, so pike matter, but never exactly.
            let to_duck = self.duck.pos - start;
            let angle = to_duck.y.atan2(to_duck.x) + rng.range(-0.8, 0.8);
            let dir = vec2(angle.cos(), angle.sin());
            let room = distance_to_edge(start, dir, POND);
            if room < 110.0 {
                continue;
            }
            let level = self.level as f32;
            self.pikes.push(Pike {
                head: start,
                angle,
                speed: (65.0 + 10.0 * (level - 1.0)).min(135.0),
                phase: PikePhase::Warning(PIKE_WARNING),
                travelled: 0.0,
                max_travel: (room - 6.0).min(260.0),
                // From level 2 on, pike bend toward their prey.
                turn_rate: if self.level >= 2 {
                    (0.5 + 0.15 * (level - 2.0)).min(1.3)
                } else {
                    0.0
                },
            });
            return;
        }
    }

    fn update_pikes(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();

        self.pike_timer -= dt;
        let active = self
            .pikes
            .iter()
            .filter(|p| !matches!(p.phase, PikePhase::Diving(_)))
            .count();
        if self.banner <= 0.0 && self.pike_timer <= 0.0 && active < self.max_pikes() {
            self.pike_timer = self.pike_interval();
            self.spawn_pike(&mut ctx.rng);
        }

        // The nearest thing to bite, for pike that hunt.
        let mut prey: Vec<Vec2> = self.line.iter().map(|f| f.pos).collect();
        prey.push(self.duck.pos);

        for pike in &mut self.pikes {
            match pike.phase {
                PikePhase::Warning(left) => {
                    let left = left - dt;
                    if (left / 0.3).floor() != ((left + dt) / 0.3).floor() {
                        self.ripples.push(Ripple {
                            pos: pike.head - pike.dir() * (PIKE_LENGTH / 2.0),
                            radius: 6.0,
                            grow: 22.0,
                            life: 0.5,
                            max_life: 0.5,
                            color: WARNING,
                        });
                    }
                    pike.phase = if left <= 0.0 {
                        splash(&mut self.particles, pike.head, 10, &mut ctx.rng);
                        PikePhase::Swimming
                    } else {
                        PikePhase::Warning(left)
                    };
                }
                PikePhase::Swimming => {
                    if pike.turn_rate > 0.0 {
                        let nearest = prey
                            .iter()
                            .min_by(|a, b| a.distance(pike.head).total_cmp(&b.distance(pike.head)));
                        if let Some(&target) = nearest {
                            let to = target - pike.head;
                            // Only chase what's in front: a pike doesn't turn around.
                            if to.normalized().dot(pike.dir()) > 0.0 {
                                pike.angle =
                                    turn_toward(pike.angle, to.y.atan2(to.x), pike.turn_rate * dt);
                            }
                        }
                    }
                    let step = pike.speed * dt;
                    pike.head += pike.dir() * step;
                    pike.travelled += step;
                    let inside = POND.x + 4.0 < pike.head.x
                        && pike.head.x < POND.right() - 4.0
                        && POND.y + 4.0 < pike.head.y
                        && pike.head.y < POND.bottom() - 4.0;
                    if pike.travelled >= pike.max_travel || !inside {
                        pike.phase = PikePhase::Diving(0.4);
                        splash(&mut self.particles, pike.head, 6, &mut ctx.rng);
                    }
                }
                PikePhase::Diving(left) => pike.phase = PikePhase::Diving(left - dt),
            }
        }
        self.pikes
            .retain(|p| !matches!(p.phase, PikePhase::Diving(left) if left <= 0.0));

        self.check_bites(&mut ctx.rng);
    }

    fn check_bites(&mut self, rng: &mut Rng) {
        for p in 0..self.pikes.len() {
            if self.pikes[p].phase != PikePhase::Swimming {
                continue;
            }

            // A bite on the line: one duckling is eaten and the rest scatter.
            let hit = (0..self.line.len())
                .find(|&i| self.pikes[p].touches(self.line[i].pos, DUCKLING_RADIUS));
            if let Some(i) = hit {
                let at = self.line[i].pos;
                self.line.remove(i);
                self.eaten += 1;
                self.scatter(i, at, rng);
                feathers(&mut self.particles, at, CHICK, 14, rng);
                splash(&mut self.particles, at, 8, rng);
                self.popups.push(Popup {
                    pos: at - vec2(10.0, 12.0),
                    text: String::from("GULP!"),
                    life: 1.0,
                    color: WARNING,
                });
                self.pikes[p].phase = PikePhase::Diving(0.4);
                self.shake = 0.25;
                continue;
            }

            // A bite on the duck: a life is lost and the whole line scatters.
            if self.duck.hurt <= 0.0 && self.pikes[p].touches(self.duck.pos, DUCK_RADIUS) {
                let at = self.duck.pos;
                self.lives = self.lives.saturating_sub(1);
                self.duck.hurt = HURT_TIME;
                self.duck.vel = (at - self.pikes[p].head).normalized() * 160.0;
                self.scatter(0, at, rng);
                feathers(&mut self.particles, at, DUCK_WHITE, 18, rng);
                self.pikes[p].phase = PikePhase::Diving(0.4);
                self.shake = 0.4;
                if self.lives == 0 {
                    self.state = State::GameOver;
                    self.best = self.best.max(self.score);
                }
            }
        }
    }

    // ---- Effects ----

    fn update_effects(&mut self, dt: f32) {
        for r in &mut self.ripples {
            r.life -= dt;
            r.radius += r.grow * dt;
        }
        self.ripples.retain(|r| r.life > 0.0);

        for p in &mut self.particles {
            p.life -= dt;
            p.pos += p.vel * dt;
            p.vel = p.vel * (1.0 - 3.0 * dt).max(0.0);
        }
        self.particles.retain(|p| p.life > 0.0);

        for popup in &mut self.popups {
            popup.life -= dt;
            popup.pos.y -= 14.0 * dt;
        }
        self.popups.retain(|p| p.life > 0.0);

        let mut landed = 0;
        for hop in &mut self.hops {
            hop.t += dt;
            if hop.t >= HOP_TIME {
                landed += 1;
            }
        }
        self.hops.retain(|h| h.t < HOP_TIME);
        for _ in 0..landed {
            self.in_nest += 1;
            self.ripples.push(Ripple {
                pos: NEST,
                radius: NEST_RADIUS - 4.0,
                grow: 20.0,
                life: 0.35,
                max_life: 0.35,
                color: STRAW,
            });
        }

        let banner_was_up = self.banner > 0.0;
        self.banner = (self.banner - dt).max(0.0);
        if banner_was_up && self.banner <= 0.0 && self.saved_this_level == 0 {
            // The nest's ducklings go off to bed once the new level begins.
            self.in_nest = 0;
        }
        self.shake = (self.shake - dt).max(0.0);
    }

    /// On the title screen, the duck leads a little parade around the pond by itself.
    fn title_parade(&mut self, dt: f32) {
        if self.line.is_empty() {
            for _ in 0..5 {
                self.line.push(Follower {
                    pos: self.duck.pos,
                    angle: 0.0,
                });
            }
        }
        let t = self.time * 0.45;
        let target = vec2(160.0 + 110.0 * t.cos(), 150.0 + 45.0 * (2.0 * t).sin());
        let vel = (target - self.duck.pos) * 3.0;
        self.steer_duck(vel, dt);
        self.follow(dt);
    }

    fn shake_offset(&self) -> Vec2 {
        if self.shake <= 0.0 {
            return Vec2::ZERO;
        }
        let amount = self.shake * 8.0;
        vec2(
            ((self.time * 91.0).sin() * amount).round(),
            ((self.time * 73.0).cos() * amount).round(),
        )
    }
}

impl Game for Parade {
    fn update(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        self.time += dt;
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        let confirm = ctx.input.was_pressed(Key::Enter) || ctx.input.was_pressed(Key::Space);

        match self.state {
            State::Title => {
                self.title_parade(dt);
                if confirm {
                    self.start();
                }
            }
            State::Playing => {
                if ctx.input.was_pressed(Key::P) {
                    self.state = State::Paused;
                    return;
                }
                if ctx.input.was_pressed(Key::R) {
                    self.start();
                    return;
                }
                self.swim(ctx);
                self.follow(dt);
                self.pick_up();
                self.bring_home();
                self.update_lost(ctx);
                self.update_pikes(ctx);
            }
            State::Paused => {
                if ctx.input.was_pressed(Key::P) || confirm {
                    self.state = State::Playing;
                }
                return;
            }
            State::GameOver => {
                // The pond carries on quietly behind the score.
                self.follow(dt);
                if confirm || ctx.input.was_pressed(Key::R) {
                    self.start();
                }
            }
        }
        self.update_effects(dt);
    }

    fn draw(&self, canvas: &mut Canvas) {
        let o = self.shake_offset();
        self.draw_pond(canvas, o);
        self.draw_nest(canvas, o);

        for r in &self.ripples {
            let color = WATER_MID.lerp(r.color, r.life / r.max_life * 0.8);
            ring(canvas, r.pos + o, r.radius, color);
        }
        for pike in &self.pikes {
            draw_pike(canvas, pike, o, self.time);
        }
        for lost in &self.lost {
            // Panicking ducklings flash, so you know you can't grab them yet.
            let bob = (self.time * 5.0 + lost.pos.x).sin() * 0.4;
            if lost.panic <= 0.0 || (self.time * 12.0).sin() > 0.0 {
                draw_bird(canvas, lost.pos + o, lost.angle, 0.55 + bob * 0.05, true);
            }
        }
        for follower in &self.line {
            draw_bird(canvas, follower.pos + o, follower.angle, 0.55, true);
        }
        for hop in &self.hops {
            let k = (hop.t / HOP_TIME).clamp(0.0, 1.0);
            let pos = hop.from.lerp_to(NEST, k) - vec2(0.0, (k * PI).sin() * 12.0);
            draw_bird(canvas, pos + o, -PI / 2.0, 0.55, true);
        }
        let blink = self.duck.hurt > 0.0 && (self.time * 20.0).sin() < 0.0;
        if !blink {
            draw_bird(canvas, self.duck.pos + o, self.duck.angle, 1.0, false);
        }
        for p in &self.particles {
            let color = WATER_MID.lerp(p.color, (p.life / p.max_life).sqrt());
            canvas.fill_rect(Rect::from_center(p.pos + o, p.size, p.size), color);
        }
        for popup in &self.popups {
            let color = WATER_MID.lerp(popup.color, (popup.life * 2.0).min(1.0));
            shadow_text(canvas, &popup.text, popup.pos + o, 1, color);
        }

        match self.state {
            State::Title => self.draw_title(canvas),
            State::Playing | State::Paused => {
                self.draw_hud(canvas);
                if self.banner > 0.0 {
                    panel(canvas, Rect::from_center(vec2(160.0, 104.0), 130.0, 46.0));
                    let text = format!("LEVEL {}", self.level);
                    shadow_text_centered(canvas, &text, 88.0, 3, Color::WHITE);
                    let hint = format!("SAVE {} DUCKLINGS", self.goal());
                    shadow_text_centered(canvas, &hint, 114.0, 1, Color::YELLOW);
                }
                if self.state == State::Paused {
                    panel(canvas, Rect::from_center(vec2(160.0, 120.0), 120.0, 44.0));
                    canvas.draw_text_centered("PAUSED", 106.0, 2, Color::WHITE);
                    canvas.draw_text_centered("P TO CARRY ON", 124.0, 1, Color::GRAY);
                }
            }
            State::GameOver => {
                self.draw_hud(canvas);
                self.draw_game_over(canvas);
            }
        }
    }
}

// ---- Drawing the scene ----

impl Parade {
    fn draw_pond(&self, canvas: &mut Canvas, o: Vec2) {
        canvas.clear(GRASS);
        // Tufts of darker grass, always in the same places.
        let mut tufts = Rng::new(7);
        for _ in 0..90 {
            let p = vec2(tufts.range(0.0, WIDTH), tufts.range(0.0, HEIGHT));
            canvas.draw_line(p + o, p + o + vec2(1.0, -3.0), GRASS_DARK);
            canvas.draw_line(p + o + vec2(2.0, 0.0), p + o + vec2(2.0, -2.0), GRASS_DARK);
        }

        // A muddy bank, then the water, darker toward the bottom.
        rounded_rect(canvas, grow(POND, 3.0), 14.0, o, |_| MUD);
        rounded_rect(canvas, POND, 12.0, o, |y| {
            WATER_TOP.lerp(WATER_BOTTOM, (y - POND.y) / POND.h)
        });

        for (i, &p) in self.sparkles.iter().enumerate() {
            let shimmer = (self.time * 1.7 + i as f32 * 2.3).sin();
            if shimmer > 0.55 {
                let len = if shimmer > 0.85 { 4.0 } else { 2.0 };
                canvas.draw_line(p + o, p + o + vec2(len, 0.0), SPARKLE);
            }
        }

        for &(pos, radius, notch) in &self.lily_pads {
            let p = pos + o;
            canvas.fill_circle(p, radius + 1.0, Color::from_hex(0x2f6a2c));
            canvas.fill_circle(p, radius, Color::from_hex(0x4f9a3f));
            let water = WATER_TOP.lerp(WATER_BOTTOM, (pos.y - POND.y) / POND.h);
            let a = p + vec2((notch - 0.35).cos(), (notch - 0.35).sin()) * (radius + 1.5);
            let b = p + vec2((notch + 0.35).cos(), (notch + 0.35).sin()) * (radius + 1.5);
            canvas.fill_triangle(p, a, b, water);
            canvas.draw_line(
                p,
                p - vec2(notch.cos(), notch.sin()) * (radius - 2.0),
                Color::from_hex(0x3f8434),
            );
        }
        // One pad has a flower.
        if let Some(&(pos, _, _)) = self.lily_pads.first() {
            for k in 0..5 {
                let a = k as f32 * TAU / 5.0;
                canvas.fill_circle(
                    pos + o + vec2(a.cos(), a.sin()) * 2.5,
                    2.0,
                    Color::from_hex(0xf3a6c8),
                );
            }
            canvas.fill_circle(pos + o, 1.5, Color::YELLOW);
        }

        // Reeds on the side banks.
        for (i, x) in [4.0, 7.0, 312.0, 315.0, 309.0].into_iter().enumerate() {
            let base = 200.0 - i as f32 * 37.0 % 60.0;
            for k in 0..3 {
                let y = base + k as f32 * 9.0;
                let sway = ((self.time * 1.3 + y).sin() * 1.5).round();
                canvas.draw_line(vec2(x, y + 10.0) + o, vec2(x + sway, y) + o, GRASS_DARK);
                canvas.fill_rect(
                    Rect::new(x + sway - 1.0, y - 3.0, 2.0, 4.0).moved(o),
                    Color::from_hex(0x7a4e2a),
                );
            }
        }
    }

    fn draw_nest(&self, canvas: &mut Canvas, o: Vec2) {
        let c = NEST + o;
        // When you have a line to bring home, the nest calls you with a glow.
        if !self.line.is_empty() && self.state == State::Playing {
            let pulse = (self.time * 5.0).sin() * 0.5 + 0.5;
            ring(
                canvas,
                c,
                NEST_RADIUS + 3.0 + pulse * 3.0,
                STRAW.lerp(Color::WHITE, pulse),
            );
        }
        canvas.fill_circle(c, NEST_RADIUS + 1.0, Color::from_hex(0x4d3016));
        canvas.fill_circle(c, NEST_RADIUS, NEST_OUTER);
        canvas.fill_circle(c, NEST_RADIUS - 3.0, NEST_INNER);
        for k in 0..14 {
            let a = k as f32 * TAU / 14.0;
            let from = c + vec2(a.cos(), a.sin()) * (NEST_RADIUS - 5.0);
            let to = c + vec2((a + 0.6).cos(), (a + 0.6).sin()) * (NEST_RADIUS - 0.5);
            canvas.draw_line(from, to, STRAW);
        }
        canvas.fill_circle(c, NEST_RADIUS - 7.0, NEST_HOLLOW);

        // The ducklings already home, snuggled in a ring.
        let shown = self.in_nest.min(10) as usize;
        for i in 0..shown {
            let a = -PI / 2.0 + i as f32 * TAU / shown.max(1) as f32;
            let r = if shown == 1 { 0.0 } else { 4.0 };
            let p = c + vec2(a.cos(), a.sin()) * r;
            canvas.fill_circle(p, 3.0, CHICK_OUTLINE);
            canvas.fill_circle(p, 2.3, CHICK);
        }
    }

    fn draw_hud(&self, canvas: &mut Canvas) {
        canvas.fill_rect(Rect::new(0.0, 0.0, WIDTH, 15.0), GRASS_DARK);
        canvas.fill_rect(Rect::new(0.0, 15.0, WIDTH, 1.0), Color::from_hex(0x35652a));
        shadow_text(
            canvas,
            &format!("SCORE {}", self.score),
            vec2(5.0, 5.0),
            1,
            Color::WHITE,
        );

        let progress = format!(
            "LEVEL {}  {}/{}",
            self.level,
            self.saved_this_level,
            self.goal()
        );
        let x = 160.0 - Canvas::text_width(&progress, 1) / 2.0;
        shadow_text(canvas, &progress, vec2(x, 5.0), 1, Color::WHITE);
        // A bar fills up as you save ducklings.
        let bar = Rect::new(x, 12.0, Canvas::text_width(&progress, 1), 2.0);
        canvas.fill_rect(bar, Color::from_hex(0x2d4d24));
        let filled = bar.w * self.saved_this_level as f32 / self.goal() as f32;
        canvas.fill_rect(Rect::new(bar.x, bar.y, filled.min(bar.w), 2.0), CHICK);

        for i in 0..self.lives {
            heart(canvas, vec2(WIDTH - 9.0 - i as f32 * 10.0, 7.0));
        }

        // What the line behind you is worth right now.
        if !self.line.is_empty() {
            let text = format!(
                "LINE {}  +{}",
                self.line.len(),
                Parade::points_for(self.line.len())
            );
            let x = NEST.x - NEST_RADIUS - 10.0 - Canvas::text_width(&text, 1);
            shadow_text(canvas, &text, vec2(x, 218.0), 1, CHICK);
        }
    }

    fn draw_title(&self, canvas: &mut Canvas) {
        shadow_text_centered(canvas, "DUCKLING", 40.0, 4, Color::WHITE);
        shadow_text_centered(canvas, "PARADE", 66.0, 4, CHICK);
        shadow_text_centered(
            canvas,
            "LEAD THE LOST DUCKLINGS HOME",
            96.0,
            1,
            Color::WHITE,
        );
        shadow_text_centered(canvas, "BEFORE THE PIKE GET THEM", 104.0, 1, Color::WHITE);
        if (self.time * 2.5).sin() > -0.3 {
            shadow_text_centered(canvas, "PRESS ENTER", 196.0, 2, Color::YELLOW);
        }
        shadow_text_centered(
            canvas,
            "ARROWS OR WASD TO SWIM",
            214.0,
            1,
            Color::from_hex(0xd8e8f0),
        );
        if self.best > 0 {
            shadow_text_centered(canvas, &format!("BEST {}", self.best), 5.0, 1, Color::WHITE);
        }
    }

    fn draw_game_over(&self, canvas: &mut Canvas) {
        panel(canvas, Rect::from_center(vec2(160.0, 118.0), 200.0, 96.0));
        canvas.draw_text_centered("GAME OVER", 80.0, 3, Color::WHITE);
        let new_best = self.score > 0 && self.score >= self.best;
        let score = format!("SCORE {}", self.score);
        canvas.draw_text_centered(&score, 104.0, 2, Color::YELLOW);
        let best = if new_best {
            String::from("NEW BEST!")
        } else {
            format!("BEST {}", self.best)
        };
        canvas.draw_text_centered(&best, 120.0, 1, if new_best { CHICK } else { Color::GRAY });
        let stats = format!(
            "SAVED {}   LONGEST LINE {}   EATEN {}",
            self.saved_total, self.longest_line, self.eaten
        );
        canvas.draw_text_centered(&stats, 132.0, 1, Color::from_hex(0xd8e8f0));
        if (self.time * 2.5).sin() > -0.3 {
            canvas.draw_text_centered("ENTER TO SWIM AGAIN", 150.0, 1, Color::WHITE);
        }
    }
}

/// Draws the duck (`chick = false`) or a duckling, facing `angle`, `scale` times full size.
fn draw_bird(canvas: &mut Canvas, pos: Vec2, angle: f32, scale: f32, chick: bool) {
    let dir = vec2(angle.cos(), angle.sin());
    let side = vec2(-dir.y, dir.x);
    let (body, shade, outline) = if chick {
        (CHICK, CHICK.lerp(CHICK_OUTLINE, 0.3), CHICK_OUTLINE)
    } else {
        (DUCK_WHITE, DUCK_SHADE, DUCK_OUTLINE)
    };
    let s = scale;
    let back = pos - dir * (3.0 * s);
    let head = pos + dir * (6.0 * s);

    // A soft shadow on the water first.
    canvas.fill_circle(pos + vec2(1.0, 2.0), 6.5 * s, SHADOW.lerp(WATER_MID, 0.55));

    // Outline: everything drawn one pixel bigger in a darker color.
    let tail_tip = pos - dir * (11.0 * s);
    canvas.fill_triangle(
        back + side * (4.5 * s),
        back - side * (4.5 * s),
        tail_tip - dir,
        outline,
    );
    canvas.fill_circle(pos, 6.5 * s + 1.0, outline);
    canvas.fill_circle(back, 5.5 * s + 1.0, outline);
    canvas.fill_circle(head, 4.0 * s + 1.0, outline);

    canvas.fill_triangle(
        back + side * (3.5 * s),
        back - side * (3.5 * s),
        tail_tip,
        body,
    );
    canvas.fill_circle(back, 5.5 * s, body);
    canvas.fill_circle(pos, 6.5 * s, body);
    // Folded wings.
    canvas.fill_circle(pos - dir * (1.5 * s) + side * (3.0 * s), 3.0 * s, shade);
    canvas.fill_circle(pos - dir * (1.5 * s) - side * (3.0 * s), 3.0 * s, shade);
    canvas.fill_circle(head, 4.0 * s, body);

    let beak_base = head + dir * (3.0 * s);
    canvas.fill_triangle(
        beak_base + side * (2.2 * s),
        beak_base - side * (2.2 * s),
        head + dir * (7.5 * s),
        BEAK,
    );
    for eye in [
        head + dir * (1.2 * s) + side * (2.2 * s),
        head + dir * (1.2 * s) - side * (2.2 * s),
    ] {
        canvas.fill_rect(
            Rect::from_center(eye, 1.0 + s * 0.6, 1.0 + s * 0.6),
            Color::from_hex(0x1a1a1a),
        );
    }
}

fn draw_pike(canvas: &mut Canvas, pike: &Pike, o: Vec2, time: f32) {
    let dir = pike.dir();
    let side = vec2(-dir.y, dir.x);
    match pike.phase {
        PikePhase::Warning(left) => {
            // A dark shape rising, and an arrow showing where it will go.
            let k = 1.0 - left / PIKE_WARNING;
            let middle = pike.head - dir * (PIKE_LENGTH / 2.0) + o;
            let shade = WATER_MID.lerp(PIKE_SHADOW, 0.4 + 0.6 * k);
            for i in 0..5 {
                let t = i as f32 / 4.0 - 0.5;
                canvas.fill_circle(
                    middle + dir * (t * PIKE_LENGTH * k),
                    (2.0 + 3.0 * k) * (1.0 - t.abs()),
                    shade,
                );
            }
            let reach = 22.0 + 30.0 * k;
            let start = pike.head + o + dir * 4.0;
            let end = start + dir * reach;
            let blink = (time * 14.0).sin() > -0.2;
            if blink {
                for i in 0..4 {
                    let p = start + dir * (reach * i as f32 / 4.0);
                    canvas.fill_rect(Rect::from_center(p, 2.0, 2.0), WARNING);
                }
                canvas.fill_triangle(end + dir * 5.0, end + side * 4.0, end - side * 4.0, WARNING);
            }
        }
        PikePhase::Swimming | PikePhase::Diving(_) => {
            let sink = match pike.phase {
                PikePhase::Diving(left) => 1.0 - left / 0.4,
                _ => 0.0,
            };
            let body = PIKE_BODY.lerp(PIKE_SHADOW, sink);
            let belly = PIKE_BELLY.lerp(PIKE_SHADOW, sink);
            let outline = PIKE_OUTLINE.lerp(PIKE_SHADOW, sink);
            let size = 1.0 - sink * 0.4;
            // A body of overlapping circles that wiggles like a swimming fish.
            let radii = [3.4, 4.4, 4.8, 4.8, 4.4, 3.8, 3.0, 2.2];
            let points: Vec<Vec2> = (0..radii.len())
                .map(|i| {
                    let along = i as f32 * PIKE_LENGTH / (radii.len() as f32 + 0.5);
                    let wiggle = (time * 14.0 - i as f32 * 0.9).sin() * (i as f32 * 0.35);
                    pike.head - dir * along + side * wiggle + o
                })
                .collect();
            let tail = *points.last().unwrap_or(&pike.head);
            let fin_wave = (time * 14.0 - 7.0).sin() * 2.5;
            let fin_end = tail - dir * 7.0 + side * fin_wave;
            canvas.fill_triangle(tail, fin_end + side * 5.0, fin_end - side * 5.0, outline);
            for (p, r) in points.iter().zip(radii) {
                canvas.fill_circle(*p, r * size + 1.0, outline);
            }
            for (p, r) in points.iter().zip(radii) {
                canvas.fill_circle(*p, r * size, body);
            }
            for (p, r) in points.iter().zip(radii).skip(1).take(5) {
                canvas.fill_circle(*p + side * 0.0, r * size * 0.45, belly);
            }
            // Spots, eyes and a long toothy mouth.
            for p in points.iter().skip(2).step_by(2) {
                canvas.fill_rect(Rect::from_center(*p + side * 2.0, 1.0, 1.0), belly);
                canvas.fill_rect(Rect::from_center(*p - side * 2.0, 1.0, 1.0), belly);
            }
            let head = points[0];
            canvas.fill_triangle(
                head + side * 3.0,
                head - side * 3.0,
                head + dir * 6.0 * size,
                body,
            );
            for eye in [head + side * 2.6 - dir, head - side * 2.6 - dir] {
                canvas.fill_rect(Rect::from_center(eye, 2.0, 2.0), Color::from_hex(0xf4e27a));
                canvas.fill_rect(Rect::from_center(eye, 1.0, 1.0), Color::BLACK);
            }
            canvas.draw_line(head + dir * 5.0, head - dir * 1.0, outline);
        }
    }
}

// ---- Small helpers ----

/// The middle of the pond, a little low: where the duck starts.
fn start_spot() -> Vec2 {
    vec2(160.0, 170.0)
}

/// Turns `angle` toward `target` by at most `max_step` radians, the short way round.
fn turn_toward(angle: f32, target: f32, max_step: f32) -> f32 {
    let diff = (target - angle + PI).rem_euclid(TAU) - PI;
    angle + diff.clamp(-max_step, max_step)
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let length_squared = ab.dot(ab);
    let t = if length_squared > 0.0 {
        ((p - a).dot(ab) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// How far from `start` along `dir` (a unit vector) until the edge of `area`.
fn distance_to_edge(start: Vec2, dir: Vec2, area: Rect) -> f32 {
    let along = |pos: f32, d: f32, low: f32, high: f32| {
        if d > 1e-4 {
            (high - pos) / d
        } else if d < -1e-4 {
            (low - pos) / d
        } else {
            f32::INFINITY
        }
    };
    along(start.x, dir.x, area.x, area.right()).min(along(start.y, dir.y, area.y, area.bottom()))
}

fn grow(rect: Rect, by: f32) -> Rect {
    Rect::new(
        rect.x - by,
        rect.y - by,
        rect.w + 2.0 * by,
        rect.h + 2.0 * by,
    )
}

/// A rectangle with round corners, filled one row at a time with `color(y)`.
fn rounded_rect(
    canvas: &mut Canvas,
    rect: Rect,
    radius: f32,
    o: Vec2,
    color: impl Fn(f32) -> Color,
) {
    let mut y = rect.y;
    while y < rect.bottom() {
        // How far in from each side the corner curve is on this row.
        let from_top = y + 0.5 - rect.y;
        let from_bottom = rect.bottom() - (y + 0.5);
        let d = from_top.min(from_bottom);
        let inset = if d < radius {
            radius
                - (radius * radius - (radius - d) * (radius - d))
                    .max(0.0)
                    .sqrt()
        } else {
            0.0
        };
        let row = Rect::new(rect.x + inset, y, rect.w - 2.0 * inset, 1.0);
        canvas.fill_rect(row.moved(o), color(y));
        y += 1.0;
    }
}

/// An outline of a circle, drawn with short lines.
fn ring(canvas: &mut Canvas, center: Vec2, radius: f32, color: Color) {
    let segments = ((radius * 1.2) as usize).clamp(8, 40);
    let mut previous = center + vec2(radius, 0.0);
    for i in 1..=segments {
        let a = i as f32 * TAU / segments as f32;
        let next = center + vec2(a.cos(), a.sin()) * radius;
        canvas.draw_line(previous, next, color);
        previous = next;
    }
}

fn heart(canvas: &mut Canvas, center: Vec2) {
    canvas.fill_circle(center + vec2(-2.0, -1.0), 2.3, HEART);
    canvas.fill_circle(center + vec2(2.0, -1.0), 2.3, HEART);
    canvas.fill_triangle(
        center + vec2(-4.3, -0.5),
        center + vec2(4.3, -0.5),
        center + vec2(0.0, 4.0),
        HEART,
    );
    canvas.fill_rect(
        Rect::new(center.x - 2.5, center.y - 2.5, 1.0, 1.0),
        Color::from_hex(0xffb3c2),
    );
}

fn panel(canvas: &mut Canvas, rect: Rect) {
    canvas.fill_rect(grow(rect, 2.0), Color::from_hex(0x0e1a26));
    canvas.fill_rect(rect, Color::from_hex(0x1d3448));
    canvas.draw_rect(rect, Color::from_hex(0x3f6c8c));
}

fn shadow_text(canvas: &mut Canvas, text: &str, pos: Vec2, scale: u32, color: Color) {
    canvas.draw_text(text, pos + vec2(scale as f32, scale as f32), scale, SHADOW);
    canvas.draw_text(text, pos, scale, color);
}

fn shadow_text_centered(canvas: &mut Canvas, text: &str, y: f32, scale: u32, color: Color) {
    let x = (WIDTH - Canvas::text_width(text, scale)) / 2.0;
    shadow_text(canvas, text, vec2(x.round(), y), scale, color);
}

fn splash(particles: &mut Vec<Particle>, at: Vec2, count: usize, rng: &mut Rng) {
    for _ in 0..count {
        let a = rng.range(0.0, TAU);
        let speed = rng.range(30.0, 80.0);
        particles.push(Particle {
            pos: at,
            vel: vec2(a.cos(), a.sin()) * speed,
            life: 0.45,
            max_life: 0.45,
            color: SPARKLE.lerp(Color::WHITE, 0.5),
            size: 2.0,
        });
    }
}

fn feathers(particles: &mut Vec<Particle>, at: Vec2, color: Color, count: usize, rng: &mut Rng) {
    for _ in 0..count {
        let a = rng.range(0.0, TAU);
        let speed = rng.range(20.0, 70.0);
        particles.push(Particle {
            pos: at,
            vel: vec2(a.cos(), a.sin()) * speed,
            life: rng.range(0.6, 1.1),
            max_life: 1.1,
            color,
            size: if rng.chance(0.3) { 2.0 } else { 1.0 },
        });
    }
}

trait Moved {
    fn moved(self, by: Vec2) -> Self;
}

impl Moved for Rect {
    fn moved(self, by: Vec2) -> Self {
        Rect::new(self.x + by.x, self.y + by.y, self.w, self.h)
    }
}

trait LerpTo {
    fn lerp_to(self, other: Self, t: f32) -> Self;
}

impl LerpTo for Vec2 {
    fn lerp_to(self, other: Vec2, t: f32) -> Vec2 {
        self + (other - self) * t
    }
}

fn config() -> Config {
    Config {
        title: String::from("Duckling Parade - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Parade::new(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARROWS: [Key; 4] = [Key::Left, Key::Right, Key::Up, Key::Down];

    /// A simple autopilot: fetch ducklings, bring them home, dodge pike.
    /// It plays only by pressing keys, exactly like a person would.
    fn bot_direction(game: &Parade, bank_at: usize) -> Vec2 {
        let duck = game.duck.pos;
        let need = (game.goal() - game.saved_this_level) as usize;
        let free: Vec<&Lost> = game.lost.iter().filter(|l| l.panic <= 0.0).collect();
        let go_home =
            !game.line.is_empty() && (game.line.len() >= bank_at.min(need) || free.is_empty());
        let target = if go_home {
            NEST
        } else {
            free.iter()
                .map(|l| l.pos)
                .min_by(|a, b| a.distance(duck).total_cmp(&b.distance(duck)))
                .unwrap_or(start_spot())
        };
        let mut want = (target - duck).normalized();

        // Swim away from pike that are near, or about to strike near us.
        for pike in &game.pikes {
            let (from, to) = match pike.phase {
                PikePhase::Diving(_) => continue,
                PikePhase::Warning(_) => (pike.tail(), pike.head + pike.dir() * 60.0),
                PikePhase::Swimming => (pike.tail(), pike.head + pike.dir() * 45.0),
            };
            let d = distance_to_segment(duck, from, to);
            if d < 40.0 {
                let ab = to - from;
                let t = ((duck - from).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
                let away = (duck - (from + ab * t)).normalized();
                want += away * (3.0 * (1.0 - d / 40.0));
            }
        }
        want.normalized()
    }

    fn hold(sim: &mut Simulation<Parade>, dir: Vec2) {
        let wanted = [dir.x < -0.35, dir.x > 0.35, dir.y < -0.35, dir.y > 0.35];
        for (key, on) in ARROWS.into_iter().zip(wanted) {
            if on {
                sim.press(key);
            } else {
                sim.release(key);
            }
        }
    }

    fn tap(sim: &mut Simulation<Parade>, key: Key) {
        sim.press(key);
        sim.step();
        sim.release(key);
    }

    /// Plays `frames` updates with the bot; saves a PNG every `every` frames into `shots`.
    fn play(
        seed: u32,
        frames: u64,
        bank_at: usize,
        shots: Option<(&str, u64)>,
    ) -> Simulation<Parade> {
        let config = Config {
            seed: Some(seed),
            ..config()
        };
        let mut sim = Simulation::new(config, Parade::new(0));
        if let Some((dir, _)) = shots {
            std::fs::create_dir_all(dir).unwrap();
        }
        let save = |sim: &mut Simulation<Parade>, frame: u64| {
            if let Some((dir, every)) = shots
                && frame.is_multiple_of(every)
            {
                let path = format!("{dir}/{frame:05}.png");
                sim.picture().save_png(path).unwrap();
            }
        };
        sim.run(90); // the title screen
        save(&mut sim, 0);
        tap(&mut sim, Key::Enter);
        for frame in 1..frames {
            if sim.game().state == State::GameOver {
                break;
            }
            let dir = bot_direction(sim.game(), bank_at);
            hold(&mut sim, dir);
            sim.step();
            save(&mut sim, frame);
        }
        if let Some((dir, _)) = shots {
            // One more picture a second later, to see how it ended.
            release_all(&mut sim);
            sim.run(60);
            sim.picture().save_png(format!("{dir}/end.png")).unwrap();
        }
        sim
    }

    fn release_all(sim: &mut Simulation<Parade>) {
        for key in ARROWS {
            sim.release(key);
        }
    }

    #[test]
    fn the_bot_saves_ducklings_and_climbs_levels() {
        let sim = play(3, 60 * 60, 4, None);
        let game = sim.game();
        println!(
            "score {} level {} saved {} eaten {} lives {} longest {}",
            game.score, game.level, game.saved_total, game.eaten, game.lives, game.longest_line
        );
        assert!(
            game.saved_total >= 10,
            "the bot saved only {}",
            game.saved_total
        );
        assert!(game.level >= 2);
    }

    #[test]
    fn the_same_seed_gives_the_same_game() {
        let mut a = play(11, 1200, 4, None);
        let mut b = play(11, 1200, 4, None);
        assert_eq!(a.game().score, b.game().score);
        assert_eq!(a.picture().pixels(), b.picture().pixels());
    }

    #[test]
    fn a_long_line_is_worth_much_more() {
        assert_eq!(Parade::points_for(1), 10);
        assert_eq!(Parade::points_for(3), 60);
        assert_eq!(Parade::points_for(6), 210);
    }

    #[test]
    fn a_pike_biting_the_line_eats_one_and_scatters_the_rest() {
        let mut game = Parade::new(0);
        game.start();
        for i in 0..4 {
            game.line.push(Follower {
                pos: vec2(100.0 + 10.0 * i as f32, 100.0),
                angle: 0.0,
            });
        }
        // A pike lying across the second duckling.
        game.pikes.push(Pike {
            head: vec2(110.0, 110.0),
            angle: PI / 2.0, // pointing down, so its body runs from y 82 to 110
            speed: 0.0,
            phase: PikePhase::Swimming,
            travelled: 0.0,
            max_travel: 100.0,
            turn_rate: 0.0,
        });
        game.check_bites(&mut Rng::new(1));
        assert_eq!(game.line.len(), 1);
        assert_eq!(game.eaten, 1);
        assert_eq!(game.lost.len(), 2);
        assert_eq!(game.lives, START_LIVES);
    }

    #[test]
    fn three_bites_on_the_duck_end_the_game() {
        let mut game = Parade::new(0);
        game.start();
        for _ in 0..3 {
            game.duck.hurt = 0.0;
            game.pikes.push(Pike {
                head: game.duck.pos + vec2(10.0, 0.0),
                angle: 0.0,
                speed: 0.0,
                phase: PikePhase::Swimming,
                travelled: 0.0,
                max_travel: 100.0,
                turn_rate: 0.0,
            });
            game.check_bites(&mut Rng::new(1));
            game.pikes.clear();
        }
        assert_eq!(game.lives, 0);
        assert_eq!(game.state, State::GameOver);
    }

    /// Not a real test: saves pictures of a bot playthrough to look at.
    /// `SHOTS=folder cargo test --example duckling_parade -- --ignored shots`
    #[test]
    #[ignore]
    fn shots() {
        let dir = std::env::var("SHOTS").unwrap_or_else(|_| String::from("target/shots"));
        let seed = std::env::var("SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        let every = std::env::var("EVERY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30);
        let frames = std::env::var("FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1800);
        let sim = play(seed, frames, 4, Some((&dir, every)));
        let game = sim.game();
        println!(
            "frames {} score {} level {} saved {} eaten {} lives {}",
            sim.context().frame(),
            game.score,
            game.level,
            game.saved_total,
            game.eaten,
            game.lives
        );
    }
}

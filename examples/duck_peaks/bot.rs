//! A bot that plays Duck Peaks and records playthroughs/duck_peaks.json:
//! `cargo test --example duck_peaks -- --ignored`.

use super::*;

/// Makes playthroughs/duck_peaks.json: the bot collects each coin from left
/// to right, then touches the flag, finding its way with `map.route`.
/// `cargo test --example duck_peaks -- --ignored`
#[test]
#[ignore]
fn record_playthrough() {
    let mut sim = Simulation::new(config(), DuckPeaks::new(Art::load().unwrap()));
    play(&mut sim).unwrap_or_else(|why| panic!("{why}"));
    sim.recording()
        .save("playthroughs/duck_peaks.json")
        .unwrap();
}

/// The bot plays the level as it is now, with the Feel values as they are
/// now, until it reaches the flag (or three minutes pass), then a second
/// more. The play tests use it to check the level can still be won. If it
/// can't win, it says why.
pub(super) fn play(sim: &mut Simulation<DuckPeaks>) -> Result<(), String> {
    sim.step(); // reports exist after the first update
    let mut bot = Bot::default();
    let mut jump_held = false;
    let mut gave_up = None;
    while sim.text("state") == "Playing" && sim.context().frame() < 60 * 180 {
        let (go, jump) = match bot.decide(sim.game(), feel(sim.context())) {
            Ok(keys) => keys,
            Err(why) => {
                gave_up = Some(why);
                break;
            }
        };
        for (key, held) in [(Key::Left, go < 0.0), (Key::Right, go > 0.0)] {
            if held {
                sim.press(key);
            } else {
                sim.release(key);
            }
        }
        // A jump is a press, then a release on the next frame.
        if jump_held {
            sim.release(Key::Space);
            jump_held = false;
        } else if jump {
            sim.press(Key::Space);
            jump_held = true;
        }
        sim.step();
    }
    for key in [Key::Left, Key::Right, Key::Space] {
        sim.release(key);
    }
    sim.run(60); // a second of the ending
    if sim.text("state") == "Won" {
        return Ok(());
    }
    let (column, row) = cell_of(sim.game().feet());
    Err(gave_up
        .unwrap_or_else(|| format!("the bot ran out of time around column {column}, row {row}")))
}

/// How the duck runs and jumps right now: the game's tunable values (the
/// Studio's Feel), so the bot plays the game as it feels today.
pub(super) fn feel(ctx: &Context) -> Jump {
    let gravity = ctx.tune("gravity", GRAVITY);
    Jump {
        // The game moves in steps of a sixtieth of a second, which makes
        // a jump a touch lower than the formula says.
        speed: ctx.tune("jump_speed", JUMP_SPEED) - gravity / 120.0,
        gravity,
        // Nine tenths of its running speed: the duck takes a moment to
        // get going, and a jump that needs every last pixel isn't one to
        // rely on. (`map.route` leaves room in the jump's height the
        // same way.)
        run: ctx.tune("run_speed", RUN_SPEED) * 0.9,
        width: PLAYER_SIZE.x,
    }
}

/// The tile the feet at `feet` stand in.
fn cell_of(feet: Vec2) -> (i32, i32) {
    (
        (feet.x / TILE).floor() as i32,
        ((feet.y - 0.5) / TILE).floor() as i32,
    )
}

/// Where the duck stands: its feet, or (when its middle hangs over an
/// edge) the side of it that's on the ground.
fn standing(game: &DuckPeaks) -> Vec2 {
    let feet = game.feet();
    [feet.x, game.player.left(), game.player.right() - 0.0001]
        .into_iter()
        .map(|x| vec2(x, feet.y))
        .find(|at| game.map.kind_at(vec2(at.x, at.y + 1.0)) != TileKind::Empty)
        .unwrap_or(feet)
}

/// What the bot remembers from frame to frame.
#[derive(Default)]
struct Bot {
    /// The hops still to make to get to `target`: `map.route` works them
    /// out from the level and the jump as they are now.
    route: Vec<Hop>,
    /// The coin (or the flag) it's going for.
    target: Option<Vec2>,
    /// Frames it has been in the air without landing.
    in_air: u32,
    /// Times it has worked out the way to this target: missing a jump
    /// over and over means it can't play this level.
    tries: u32,
}

impl Bot {
    /// Which way to run (-1, 0 or 1), and whether to jump now. An error
    /// says why the level can't be won.
    fn decide(&mut self, game: &DuckPeaks, feel: Jump) -> Result<(f32, bool), String> {
        if !game.on_ground {
            // In the air: steer along the hop it's making.
            self.in_air += 1;
            if self.in_air > 60 * 20 {
                let (column, row) = cell_of(game.feet());
                return Err(format!(
                    "the bot never comes down to stand anywhere (it's around column {column}, \
                     row {row}): is the Start over a gap?"
                ));
            }
            // On the last hop, the one that passes through the coin, it
            // keeps to the arc exactly until it has it.
            let after = self.target.is_some_and(|target| {
                self.route.len() == 1 && (game.coins.contains(&target) || target == game.flag)
            });
            let go = self
                .route
                .first()
                .map_or(0.0, |hop| steer(hop, game, feel, after));
            return Ok((go, false));
        }
        let feet = standing(game);
        let cell = cell_of(feet);
        // The next coin from the left, or the flag when they're all collected.
        let nearest = game
            .coins
            .iter()
            .copied()
            .min_by(|a, b| a.x.total_cmp(&b.x));
        let (target, what) = match nearest {
            Some(coin) => (coin, "coin"),
            None => (game.flag, "flag"),
        };
        // A hop is done when the duck stands where it leads: after
        // landing, or (walking) in the next tile. Landed anywhere else, or
        // back at the start after a splash: think again.
        if let Some(hop) = self.route.first().copied() {
            let arrived = cell == cell_of(hop.to);
            if self.in_air > 0 {
                if arrived {
                    self.route.remove(0);
                } else {
                    self.route.clear();
                }
            } else if arrived && !hop.jump {
                self.route.remove(0);
            }
        }
        self.in_air = 0;
        if self.target != Some(target) {
            self.target = Some(target);
            self.tries = 0;
            self.route.clear();
        }
        let on_the_way = self
            .route
            .first()
            .is_some_and(|hop| cell_of(hop.from) == cell);
        if !on_the_way {
            let (column, row) = cell_of(target);
            self.tries += 1;
            if self.tries > 40 {
                return Err(format!(
                    "the bot keeps missing its way to the {what} at column {column}, row {row} \
                     (it's around column {}, row {})",
                    cell.0, cell.1
                ));
            }
            self.route = game.map.route(feet, target, feel).ok_or_else(|| {
                format!(
                    "the bot can't get from column {}, row {} to the {what} at column {column}, \
                     row {row} (its jump is {:.1} tiles high and {:.1} tiles long)",
                    cell.0,
                    cell.1,
                    feel.speed * feel.speed / (2.0 * feel.gravity) / TILE,
                    feel.run * 2.0 * feel.speed / feel.gravity / TILE
                )
            })?;
        }
        let Some(hop) = self.route.first().copied() else {
            // In the target's tile already: step onto it.
            return Ok(((target.x - feet.x).signum(), false));
        };
        // Which way the hop goes.
        let side = if hop.speed != 0.0 {
            hop.speed.signum()
        } else if hop.jump {
            0.0 // straight up
        } else {
            (hop.to.x - hop.from.x).signum()
        };
        let moving = game.vel.x * side;
        if !hop.jump {
            // Walk to the next tile, or off the edge: no faster than the
            // hop wants, so a drop straight down stays close to the edge.
            let fast_enough = moving >= hop.speed.abs().max(feel.run * 0.25);
            return Ok((if fast_enough { 0.0 } else { side }, false));
        }
        // A jump starts from the middle of the tile, or just past it (the
        // air steering can slow down, but can't catch up), not moving the
        // wrong way, and not much faster than the hop wants.
        let past = (game.feet().x - hop.from.x) * if side == 0.0 { 1.0 } else { side };
        let in_place = if side == 0.0 {
            past.abs() <= 2.0
        } else {
            (0.0..=2.0).contains(&past)
        };
        let slow = feel.run * 0.1;
        let steady = if side == 0.0 {
            game.vel.x.abs() <= slow
        } else {
            (-slow..=hop.speed.abs() + feel.run * 0.3).contains(&moving)
        };
        if in_place && steady {
            return Ok((side, true));
        }
        // Get into place, braking in time: the duck takes a moment to stop.
        let dx = hop.from.x + side - game.feet().x;
        let stopping = game.vel.x * game.vel.x / (16.0 * feel.run);
        let go = if dx.abs() <= stopping + 0.5 {
            0.0
        } else {
            dx.signum()
        };
        Ok((go, false))
    }
}

/// In the air on `hop`: which way to hold, to follow the arc the route
/// was worked out with, and come down still, well inside the tile it
/// lands in. `after_a_coin`: it's on its way through a coin, so it keeps
/// to the arc all the way down.
fn steer(hop: &Hop, game: &DuckPeaks, feel: Jump, after_a_coin: bool) -> f32 {
    let feet = game.feet();
    // Where the arc is when the feet are at height `y`, going up or down.
    let arc_at = |y: f32, rising: bool| {
        if !hop.jump {
            // Walked off an edge: falling once all of it is past it.
            let side = (hop.to.x - hop.from.x).signum();
            let time = (2.0 * (y - hop.from.y).max(0.0) / feel.gravity).sqrt();
            return hop.from.x + side * (TILE / 2.0 + feel.width / 2.0 + 0.5) + hop.speed * time;
        }
        let root = (feel.speed * feel.speed - 2.0 * feel.gravity * (hop.from.y - y))
            .max(0.0)
            .sqrt();
        let time = if rising {
            feel.speed - root
        } else {
            feel.speed + root
        };
        hop.from.x + hop.speed * time / feel.gravity
    };
    let inside = |x: f32| {
        if after_a_coin {
            x
        } else {
            x.clamp(hop.to.x - TILE / 4.0, hop.to.x + TILE / 4.0)
        }
    };
    let landing = inside(arc_at(hop.to.y, false));
    let mut aim = arc_at(feet.y, game.vel.y < 0.0);
    if game.vel.y > 0.0 && feet.y > hop.to.y - 2.0 * TILE {
        aim = inside(aim); // nearly down
    }
    // Brake in time to stop where it lands: the duck slows down at eight
    // times its running speed a second.
    let to_landing = landing - feet.x;
    let stopping = game.vel.x * game.vel.x / (16.0 * feel.run);
    if game.vel.x * to_landing > 0.0 && to_landing.abs() <= stopping {
        return 0.0;
    }
    let dx = aim - feet.x;
    if dx.abs() < 1.0 { 0.0 } else { dx.signum() }
}

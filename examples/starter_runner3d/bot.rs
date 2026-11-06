//! A bot that plays the 3D runner and records
//! playthroughs/starter_runner3d.json:
//! `cargo test --example starter_runner3d -- --ignored`.

use super::*;

/// Makes playthroughs/starter_runner3d.json: a minute of running. The bot
/// moves to the lane whose next crate is furthest away (preferring coins
/// when it's safe), and jumps when every lane is blocked close ahead.
#[test]
#[ignore]
fn record_playthrough() {
    let mut sim = Simulation::new(config(), Runner::new(Art::load().unwrap()));
    sim.step(); // reports exist after the first update
    let mut held: Option<Key> = None;
    while sim.text("state") == "Playing" && sim.context().frame() < 60 * 60 {
        // A key press lasts one frame: let go of the last one first.
        if let Some(key) = held.take() {
            sim.release(key);
            sim.step();
            continue;
        }
        if let Some(key) = decide(sim.game()) {
            sim.press(key);
            held = Some(key);
        }
        sim.step();
    }
    if let Some(key) = held {
        sim.release(key);
    }
    sim.run(60);
    assert_eq!(
        sim.text("state"),
        "Playing",
        "the bot should survive the minute"
    );
    sim.recording()
        .save("playthroughs/starter_runner3d.json")
        .unwrap();
}

/// The key to press now, if any.
fn decide(game: &Runner) -> Option<Key> {
    // How far ahead the next crate in a lane is.
    let ahead = |lane: i32| {
        game.crates
            .iter()
            .filter(|c| c.lane == lane && c.z > game.z - 0.6)
            .map(|c| c.z - game.z)
            .fold(f32::INFINITY, f32::min)
    };
    let coin_soon = |lane: i32| {
        game.coins
            .iter()
            .any(|c| c.lane == lane && c.z > game.z && c.z - game.z < 10.0)
    };
    // Can it get to lane `to`? Every lane on the way must be clear for as
    // long as it takes to slide there (one lane takes LANE_WIDTH /
    // LANE_SPEED seconds), with a crate's length to spare.
    let slide = LANE_WIDTH / LANE_SPEED;
    let reachable = |to: i32| {
        let steps = (to - game.lane).abs();
        (1..=steps).all(|n| {
            let lane = game.lane + n * (to - game.lane).signum();
            ahead(lane) > game.speed * slide * n as f32 + 1.5
        })
    };
    // Far crates are good, coins a bonus, and moving less breaks ties.
    let score = |lane: i32| {
        let clear = ahead(lane).min(40.0);
        let coin = if coin_soon(lane) { 4.0 } else { 0.0 };
        clear + coin - (lane - game.lane).abs() as f32
    };
    let best = (0..3)
        .filter(|&lane| lane == game.lane || (game.on_ground() && reachable(lane)))
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))
        .unwrap_or(game.lane);
    // Staying: jump when the crate ahead will be under the top of the
    // jump, which comes JUMP_SPEED / GRAVITY = 1/3 s after taking off (and
    // the key works on the next frame).
    let close = ahead(game.lane) < game.speed * (JUMP_SPEED / GRAVITY + 1.0 / 60.0);
    if best == game.lane && close && game.on_ground() {
        return Some(Key::Space);
    }
    match best.cmp(&game.lane) {
        std::cmp::Ordering::Less => Some(Key::Left),
        std::cmp::Ordering::Greater => Some(Key::Right),
        std::cmp::Ordering::Equal => None,
    }
}

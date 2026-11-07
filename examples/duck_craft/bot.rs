//! A bot that plays Duck Craft and records playthroughs/duck_craft.json:
//! `cargo test --example duck_craft -- --ignored record_playthrough`.
//!
//! It looks round, digs a hole, fills it with planks, puts a lamp on it,
//! builds a pillar under itself by jumping and placing, then steps back to
//! look at the duck from behind and walks off the pillar.

use super::*;

/// Holds `key` until `done` says so (at most `limit` frames).
fn hold_until(
    sim: &mut Simulation<DuckCraft>,
    key: Key,
    limit: u64,
    done: impl Fn(&DuckCraft) -> bool,
) {
    sim.press(key);
    for _ in 0..limit {
        if done(sim.game()) {
            break;
        }
        sim.step();
    }
    sim.release(key);
    sim.step();
}

fn tap(sim: &mut Simulation<DuckCraft>, key: Key) {
    sim.press(key);
    sim.step();
    sim.release(key);
    sim.run(6);
}

#[test]
#[ignore]
fn record_playthrough() {
    let mut sim = Simulation::new(config(), new_game().unwrap());
    sim.run(40);

    // Look round: right a little, then down at the ground in front.
    hold_until(&mut sim, Key::Right, 50, |_| false);
    sim.run(10);
    hold_until(&mut sim, Key::Down, 60, |g| g.pitch < -0.85);
    sim.run(10);

    // Dig two blocks: a hole.
    hold_until(&mut sim, Key::X, 200, |g| g.dug >= 2);
    assert_eq!(sim.game().dug, 2, "the bot should dig two blocks");
    sim.run(20);

    // Planks (slot 3), and fill the hole back in.
    tap(&mut sim, Key::Digit3);
    for _ in 0..2 {
        tap(&mut sim, Key::C);
        sim.run(10);
    }
    sim.run(20);

    // A lamp on top of the filled hole, then planks again.
    tap(&mut sim, Key::Digit9);
    tap(&mut sim, Key::C);
    assert_eq!(
        sim.game().carried[Block::Lamp as usize],
        7,
        "the lamp is placed"
    );
    tap(&mut sim, Key::Digit3);
    sim.run(20);

    // A pillar: look straight down, jump, and place a block under the duck
    // at the top of each jump.
    hold_until(&mut sim, Key::Down, 60, |g| g.pitch <= -1.5);
    for _ in 0..4 {
        let floor = sim.game().position.y;
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        for _ in 0..30 {
            if sim.game().position.y > floor + 1.05 {
                break;
            }
            sim.step();
        }
        tap(&mut sim, Key::C);
        sim.run(25);
    }

    // Look from behind the duck, turn round and walk off the pillar.
    tap(&mut sim, Key::V);
    hold_until(&mut sim, Key::Up, 60, |g| g.pitch > -0.35);
    hold_until(&mut sim, Key::Left, 80, |_| false);
    hold_until(&mut sim, Key::W, 90, |_| false);
    sim.run(60);

    let game = sim.game();
    assert_eq!(game.placed, 7, "two planks, a lamp and a pillar of four");
    assert!(game.behind_view);
    sim.recording()
        .save("playthroughs/duck_craft.json")
        .unwrap();
}

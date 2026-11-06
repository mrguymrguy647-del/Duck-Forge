//! A bot that plays Mini Craft and records playthroughs/mini_craft.json:
//! `cargo test --example mini_craft -- --ignored`.

use super::*;
use std::collections::{HashMap, VecDeque};

/// Makes playthroughs/mini_craft.json: the bot walks the shortest safe path
/// (no lava, no climbs of more than one block) to the nearest gem, again
/// and again until it has them all.
#[test]
#[ignore]
fn record_playthrough() {
    let mut sim = Simulation::new(config(), new_game().unwrap());
    sim.step();
    while sim.text("state") == "Playing" && sim.context().frame() < 60 * 90 {
        let game = sim.game();
        let (tx, tz) = next_step(game);
        let keys = [
            (Key::Left, tx < game.x - 0.05),
            (Key::Right, tx > game.x + 0.05),
            (Key::Down, tz < game.z - 0.05),
            (Key::Up, tz > game.z + 0.05),
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
    for key in [Key::Left, Key::Right, Key::Up, Key::Down] {
        sim.release(key);
    }
    sim.run(60);
    assert_eq!(sim.text("state"), "Won", "the bot should collect every gem");
    sim.recording()
        .save("playthroughs/mini_craft.json")
        .unwrap();
}

/// The middle of the next column on the shortest safe path to a gem (a
/// breadth-first search over columns), or where the duck is if there's none.
fn next_step(game: &MiniCraft) -> (f32, f32) {
    let world = &game.world;
    let start = (game.x.round() as i32, game.z.round() as i32);
    let goals: Vec<(i32, i32)> = game
        .gems
        .iter()
        .map(|&(x, z)| (x as i32, z as i32))
        .collect();
    let height = |(x, z): (i32, i32)| world.column(x, z).map(Column::height);
    let mut came_from = HashMap::from([(start, start)]);
    let mut queue = VecDeque::from([start]);
    while let Some(cell) = queue.pop_front() {
        if goals.contains(&cell) {
            let mut step = cell;
            while came_from[&step] != start && step != start {
                step = came_from[&step];
            }
            return (step.0 as f32, step.1 as f32);
        }
        let (x, z) = cell;
        for next in [(x + 1, z), (x - 1, z), (x, z + 1), (x, z - 1)] {
            let safe = world
                .column(next.0, next.1)
                .is_some_and(|c| c != Column::Lava);
            let climbable = height(next).is_some_and(|h| h <= height(cell).unwrap() + 1.0);
            if safe && climbable && !came_from.contains_key(&next) {
                came_from.insert(next, cell);
                queue.push_back(next);
            }
        }
    }
    (game.x, game.z)
}

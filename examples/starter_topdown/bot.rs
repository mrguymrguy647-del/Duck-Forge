//! A bot that plays starter_topdown and records playthroughs/starter_topdown.json:
//! `cargo test --example starter_topdown -- --ignored`.

use super::*;

/// Makes playthroughs/starter_topdown.json: a bot walks to the nearest
/// coin by the shortest path on the map that keeps away from enemies.
/// `cargo test --example starter_topdown -- --ignored`
#[test]
#[ignore]
fn record_playthrough() {
    let mut sim = Simulation::new(config(), TopDown::new());
    play(&mut sim).unwrap_or_else(|why| panic!("{why}"));
    sim.recording()
        .save("playthroughs/starter_topdown.json")
        .unwrap();
}

/// The bot plays the level as it is now, with the Feel values as they are
/// now (how fast everyone moves, how far enemies see), until it has every
/// coin, or is caught, or four minutes pass; then a second more. The play
/// tests use it to check the level can still be won. If it can't win, it
/// says why.
pub(super) fn play(sim: &mut Simulation<TopDown>) -> Result<(), String> {
    sim.step(); // reports exist after the first update
    let mut next = sim.game().player.center();
    while sim.text("state") == "Playing" && sim.context().frame() < 60 * 240 {
        let game = sim.game();
        // Think again six times a second (and at once when a coin is got).
        let ctx = sim.context();
        if ctx.frame().is_multiple_of(10) || game.player.center().distance(next) < 2.0 {
            // The faster the enemies are next to the player, the wider
            // the berth the bot gives them.
            let pace =
                ctx.tune("enemy_speed", ENEMY_SPEED) / ctx.tune("player_speed", PLAYER_SPEED);
            next = next_step_to_a_coin(game, TILE * (1.5 + 3.0 * pace));
        }
        let me = game.player.center();
        let keys = [
            (Key::Left, next.x < me.x - 1.0),
            (Key::Right, next.x > me.x + 1.0),
            (Key::Up, next.y < me.y - 1.0),
            (Key::Down, next.y > me.y + 1.0),
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
    sim.run(60); // a second of the ending
    let game = sim.game();
    let at = game.player.center();
    let (column, row) = ((at.x / TILE) as i32, (at.y / TILE) as i32);
    match sim.text("state").as_str() {
        "Won" => Ok(()),
        "Lost" => Err(format!(
            "the bot was caught three times (the last time around column {column}, row {row}) \
             with {} coins still to get",
            game.coins.len()
        )),
        _ => Err(format!(
            "the bot ran out of time around column {column}, row {row} with {} coins still to get",
            game.coins.len()
        )),
    }
}

/// The center of the next tile on the best path to a coin: the fewest
/// steps, where a step near an enemy counts as many (the nearer, the
/// more), so the bot goes round them when it can, and keeps moving when
/// it can't. `berth` is how far from an enemy counts as near.
fn next_step_to_a_coin(game: &TopDown, berth: f32) -> Vec2 {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};
    let tile_of = |p: Vec2| ((p.x / TILE) as i32, (p.y / TILE) as i32);
    let center = |(x, y): (i32, i32)| vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
    // What a step onto a tile costs: 10, and up to 400 more next to an
    // enemy.
    let cost = |tile: (i32, i32)| {
        let nearest = game
            .enemies
            .iter()
            .map(|e| e.body.center().distance(center(tile)))
            .fold(f32::MAX, f32::min);
        let danger = ((berth - nearest) / berth).max(0.0);
        10 + (400.0 * danger * danger) as u32
    };
    let start = tile_of(game.player.center());
    let goals: Vec<(i32, i32)> = game.coins.iter().map(|&c| tile_of(c)).collect();
    // Cheapest first (Dijkstra's search).
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::from([(start, start)]);
    let mut best: HashMap<(i32, i32), u32> = HashMap::from([(start, 0)]);
    let mut queue = BinaryHeap::from([Reverse((0, start))]);
    while let Some(Reverse((so_far, tile))) = queue.pop() {
        if so_far > best[&tile] {
            continue; // an older, dearer way here
        }
        if goals.contains(&tile) {
            // Walk back to the tile after the start.
            let mut step = tile;
            while came_from[&step] != start && step != start {
                step = came_from[&step];
            }
            return center(step);
        }
        let (x, y) = tile;
        for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            let wall = matches!(game.map.tile_at(next.0, next.1), Some((_, TileKind::Solid)));
            let inside = next.0 >= 0
                && next.1 >= 0
                && next.0 < game.map.columns() as i32
                && next.1 < game.map.rows() as i32;
            let total = so_far + cost(next);
            if inside && !wall && best.get(&next).is_none_or(|&known| total < known) {
                best.insert(next, total);
                came_from.insert(next, tile);
                queue.push(Reverse((total, next)));
            }
        }
    }
    game.player.center()
}

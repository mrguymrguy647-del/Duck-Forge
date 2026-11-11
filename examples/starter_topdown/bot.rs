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
    sim.step(); // reports exist after the first update
    while sim.text("state") == "Playing" && sim.context().frame() < 60 * 60 {
        let game = sim.game();
        let next = next_step_to_a_coin(game);
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
    sim.recording()
        .save("playthroughs/starter_topdown.json")
        .unwrap();
}

/// The center of the next tile on the shortest path to the nearest coin
/// (a breadth-first search over the floor tiles), not passing next to
/// an enemy.
fn next_step_to_a_coin(game: &TopDown) -> Vec2 {
    let tile_of = |p: Vec2| ((p.x / TILE) as usize, (p.y / TILE) as usize);
    let center = |(x, y): (usize, usize)| vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
    let start = tile_of(game.player.center());
    let goals: Vec<(usize, usize)> = game.coins.iter().map(|&c| tile_of(c)).collect();
    let mut came_from = std::collections::HashMap::new();
    let mut queue = std::collections::VecDeque::from([start]);
    came_from.insert(start, start);
    while let Some(tile) = queue.pop_front() {
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
            let near_enemy = game
                .enemies
                .iter()
                .any(|e| e.body.center().distance(center(next)) < 2.0 * TILE);
            let wall = matches!(
                game.map.tile_at(next.0 as i32, next.1 as i32),
                Some((_, TileKind::Solid))
            );
            if !wall && !near_enemy && !came_from.contains_key(&next) {
                came_from.insert(next, tile);
                queue.push_back(next);
            }
        }
    }
    game.player.center()
}

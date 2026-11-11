//! A bot that plays Duck Peaks and records playthroughs/duck_peaks.json:
//! `cargo test --example duck_peaks -- --ignored`.

use super::*;

/// Makes playthroughs/duck_peaks.json: the bot runs right, collecting each
/// coin from left to right, then touches the flag. It jumps for coins
/// above it, over walls and over water.
/// `cargo test --example duck_peaks -- --ignored`
#[test]
#[ignore]
fn record_playthrough() {
    let mut sim = Simulation::new(config(), DuckPeaks::new(Art::load().unwrap()));
    play(&mut sim);
    assert_eq!(sim.text("state"), "Won", "the bot should reach the flag");
    sim.recording()
        .save("playthroughs/duck_peaks.json")
        .unwrap();
}

/// The bot plays the level as it is now, until it reaches the flag (or 90
/// seconds pass), then a second more. The play tests use it to check the
/// level can still be won.
pub(super) fn play(sim: &mut Simulation<DuckPeaks>) {
    sim.step(); // reports exist after the first update
    let mut jump_held = false;
    let mut last_x = 0.0;
    let mut stuck_frames = 0;
    while sim.text("state") == "Playing" && sim.context().frame() < 60 * 90 {
        let game = sim.game();
        let (go, jump) = decide(game);
        // Stuck against something for half a second: jump.
        stuck_frames = if (game.player.x - last_x).abs() < 0.1 && go != 0.0 {
            stuck_frames + 1
        } else {
            0
        };
        last_x = game.player.x;
        let jump = jump || stuck_frames > 30;
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
}

/// Which way to run (-1, 0 or 1), and whether to jump now.
fn decide(game: &DuckPeaks) -> (f32, bool) {
    // The next coin from the left, or the flag when they're all collected.
    let target = game
        .coins
        .iter()
        .copied()
        .min_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap_or(game.flag);
    let feet = game.feet();
    let dx = target.x - feet.x;
    let go = if dx.abs() < 3.0 { 0.0 } else { dx.signum() };
    if !game.on_ground {
        return (go, false);
    }
    let map = &game.map;
    // The column just ahead, and the rows the duck's body is in.
    let ahead = if go < 0.0 {
        game.player.left() - 6.0
    } else {
        game.player.right() + 6.0
    };
    let (column, feet_row) = map.cell_at(vec2(ahead, feet.y - 1.0));
    let wall_ahead = go != 0.0
        && (feet_row - 1..=feet_row)
            .any(|row| matches!(map.tile_at(column, row), Some((_, TileKind::Solid))));
    // Water ahead: nothing to stand on in that column, all the way down.
    let floor_ahead = (feet_row + 1..map.rows() as i32).any(|row| {
        matches!(
            map.tile_at(column, row),
            Some((_, TileKind::Solid | TileKind::OneWay))
        )
    });
    let gap_ahead = go != 0.0 && !floor_ahead;
    // A coin higher up, close enough to jump for.
    let coin_above = target.y < feet.y - TILE && dx.abs() < 3.0 * TILE;
    (go, wall_ahead || gap_ahead || coin_above)
}

//! Play tests for starter_topdown: `cargo test --example starter_topdown`.

use super::*;

fn hold(keys: &[&str], frames: u64) -> String {
    let presses: Vec<String> = keys
        .iter()
        .map(|k| format!(r#"{{"frame": 1, "press": "{k}"}}"#))
        .collect();
    format!(
        r#"{{"events": [{}], "frames": {frames}}}"#,
        presses.join(", ")
    )
}

#[test]
fn the_map_is_a_closed_rectangle() {
    for row in MAP {
        assert_eq!(row.len(), MAP[0].len(), "every row as long as the first");
        assert!(row.starts_with('#') && row.ends_with('#'));
    }
    assert!(
        MAP[0]
            .chars()
            .chain(MAP[MAP.len() - 1].chars())
            .all(|c| c == '#')
    );
    assert_eq!(MAP.concat().matches('p').count(), 1, "one start");
}

#[test]
fn walking_right_stops_at_the_wall() {
    let played = PlayTest::new(config(), TopDown::new())
        .script(&hold(&["Right"], 240))
        .frames(240)
        .run();
    // The wall starts at tile 12, so the player stops just before it.
    assert_eq!(played.game().player.right(), 12.0 * TILE);
    assert_eq!(played.number("x"), f64::from(12.0 * TILE - BODY));
    assert_eq!(
        played.number("y"),
        f64::from(2.0 * TILE + 2.0),
        "didn't drift"
    );
}

#[test]
fn diagonals_are_not_faster() {
    let walk = |keys: &[&str]| {
        let played = PlayTest::new(config(), TopDown::new())
            .script(&hold(keys, 20))
            .frames(20)
            .run();
        played
            .game()
            .player
            .center()
            .distance(TopDown::new().player.center())
    };
    let straight = walk(&["Right"]);
    let diagonal = walk(&["Right", "Down"]);
    assert!(straight > 25.0, "{straight}");
    assert!(
        (straight - diagonal).abs() < 0.5,
        "{straight} vs {diagonal}"
    );
}

#[test]
fn a_hit_costs_a_life_then_gives_a_moment_of_safety() {
    let mut game = TopDown::new();
    // Put an enemy right on top of the player, and keep it there.
    game.enemies[0].body = game.player;
    let mut sim = Simulation::new(config(), game);
    sim.step();
    assert_eq!(sim.number("lives"), 2.0);
    sim.assert_sound("hit", 1, 0);
    for _ in 0..60 {
        let player = sim.game().player;
        sim.game_mut().enemies[0].body = player; // still touching
        sim.step();
    }
    assert_eq!(sim.number("lives"), 2.0, "safe for {SAFE_TIME} s");
    assert!(sim.failed_checks().is_empty(), "{:?}", sim.failed_checks());
}

#[test]
fn the_last_coin_wins() {
    let mut game = TopDown::new();
    let next_to_player = game.player.center() + vec2(20.0, 0.0);
    game.coins = vec![next_to_player];
    game.collected = game.total_coins - 1;
    let played = PlayTest::new(config(), game)
        .script(&hold(&["Right"], 60))
        .frames(60)
        .run();
    assert_eq!(played.text("state"), "Won");
    assert_eq!(played.number("coins"), played.game().total_coins as f64);
    assert_eq!(played.sound_frames("coin").len(), 1);
}

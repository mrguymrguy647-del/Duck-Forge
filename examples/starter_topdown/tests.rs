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

/// The game on a tiny level of its own (same letters as `level.txt`).
fn game_on(level: &str) -> TopDown {
    let legend = TileMap::load_level(LEVEL).unwrap().legend().clone();
    TopDown::on_map(TileMap::parse(level, TILE, &legend).unwrap())
}

/// A room to walk about in: the wall is at column 12.
const ROOM: &str = "
#####################
#...........#.......#
#.p......c..#....c..#
#...........#.......#
#...........#....e..#
#...................#
#####################";

// ---- The real level, as it is now: what must stay true ----

#[test]
fn the_level_keeps_its_rules_and_is_closed() {
    let map = TileMap::load_level(LEVEL).unwrap();
    assert_eq!(map.broken_rules(), Vec::<String>::new());
    let (columns, rows) = (map.columns() as i32, map.rows() as i32);
    let open: Vec<(i32, i32)> = (0..columns)
        .flat_map(|c| [(c, 0), (c, rows - 1)])
        .chain((0..rows).flat_map(|r| [(0, r), (columns - 1, r)]))
        .filter(|&(c, r)| !matches!(map.tile_at(c, r), Some((_, TileKind::Solid))))
        .collect();
    assert!(
        open.is_empty(),
        "walls all round, but these edge tiles are open: {open:?}"
    );
}

#[test]
fn every_coin_can_be_walked_to() {
    let map = TileMap::load_level(LEVEL).unwrap();
    let reach = map.reach_on_foot(map.spawn("player"));
    let missing: Vec<(i32, i32)> = reach
        .missing(&map.spawns("coin"))
        .iter()
        .map(|at| map.cell_at(vec2(at.x, at.y - 1.0)))
        .collect();
    assert!(
        missing.is_empty(),
        "walled off (column, row, counted from 0): {missing:?}"
    );
}

// ---- How it plays, on tiny levels ----

#[test]
fn walking_right_stops_at_the_wall() {
    let played = PlayTest::new(config(), game_on(ROOM))
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
        let played = PlayTest::new(config(), game_on(ROOM))
            .script(&hold(keys, 20))
            .frames(20)
            .run();
        played
            .game()
            .player
            .center()
            .distance(game_on(ROOM).player.center())
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
    let mut game = game_on(ROOM);
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
    let mut game = game_on(ROOM);
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

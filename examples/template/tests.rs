//! Play tests for the template: `cargo test --example template`.

use super::*;

/// The game, with its art loaded.
fn game() -> Template {
    Template::new(Art::load().unwrap())
}

/// The game with the duck standing at column `column` of the ground.
fn game_at(column: f32) -> Template {
    let mut game = game();
    game.put_player_at(vec2((column + 0.5) * TILE, 13.0 * TILE));
    game
}

#[test]
fn the_level_has_a_start_coins_and_a_flag() {
    let game = game();
    assert_eq!(game.map.spawns("player").len(), 1, "one start");
    assert_eq!(game.coins.len(), 8);
    assert_eq!(
        game.map.bounds(),
        Rect::new(0.0, 0.0, 80.0 * TILE, 15.0 * TILE)
    );
    // The duck starts standing on the ground.
    assert_eq!(game.player.bottom(), 13.0 * TILE);
}

#[test]
fn running_right_moves_the_duck_and_keeps_it_on_the_ground() {
    let played = PlayTest::new(config(), game())
        .script(
            r#"{"events": [{"frame": 1, "press": "Right"}, {"frame": 61, "release": "Right"}]}"#,
        )
        .frames(90)
        .run();
    assert!(played.number("x") > 120.0, "about a second of running");
    assert!(played.flag("on_ground"));
    assert_eq!(played.number("coins"), 1.0, "the coin on the way");
    assert_eq!(played.text("state"), "Playing");
}

#[test]
fn jumping_plays_a_sound_and_lands_again() {
    let played = PlayTest::new(config(), game())
        .script(
            r#"{"events": [{"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"}]}"#,
        )
        .frames(120)
        .run();
    played.assert_sound("jump", 10, 0);
    let ground: Vec<bool> = played
        .history("on_ground")
        .iter()
        .filter_map(|(_, value)| value.as_bool())
        .collect();
    assert_eq!(ground, [true, false, true], "took off, then landed");
}

#[test]
fn a_plank_is_jumped_through_from_below_and_stood_on() {
    // Under the plank at columns 8 to 11, row 10 (its top is at y = 160).
    let played = PlayTest::new(config(), game_at(9.0))
        .script(r#"{"events": [{"frame": 5, "press": "Space"}, {"frame": 6, "release": "Space"}]}"#)
        .frames(90)
        .run();
    assert!(played.flag("on_ground"));
    assert_eq!(
        played.game().player.bottom(),
        10.0 * TILE,
        "standing on the plank"
    );
    assert_eq!(played.number("coins"), 1.0, "and the coin on it");
}

#[test]
fn the_brick_wall_stops_the_duck() {
    // The bricks at columns 16 and 17 are two tiles high.
    let played = PlayTest::new(config(), game_at(12.0))
        .script(r#"{"events": [{"frame": 1, "press": "Right"}]}"#)
        .frames(90)
        .run();
    assert_eq!(played.game().player.right(), 16.0 * TILE);
}

#[test]
fn falling_in_the_water_goes_back_to_the_start() {
    // Over the first pool, columns 22 to 24.
    let mut game = game_at(23.0);
    game.player.y -= 1.0;
    let played = PlayTest::new(config(), game).frames(60).run();
    assert_eq!(played.number("splashes"), 1.0);
    played.assert_sound("hit", 20, 20);
    assert_eq!(played.game().feet(), played.game().map.spawn("player"));
}

#[test]
fn pausing_freezes_the_game() {
    let played = PlayTest::new(config(), game())
        .script(
            r#"{"events": [{"frame": 1, "press": "Right"}, {"frame": 30, "press": "P"},
                           {"frame": 31, "release": "P"}]}"#,
        )
        .frames(120)
        .run();
    assert_eq!(played.text("state"), "Paused");
    let x_at_pause = played
        .reported_at(30, "x")
        .and_then(|v| v.as_number())
        .unwrap();
    assert_eq!(played.number("x"), x_at_pause, "nothing moves while paused");
}

#[test]
fn the_playthrough_reaches_the_flag_with_every_coin() {
    let played = PlayTest::new(config(), game())
        .inputs("playthroughs/template.json")
        .run();
    assert_eq!(played.text("state"), "Won");
    assert_eq!(played.number("coins"), 8.0);
}

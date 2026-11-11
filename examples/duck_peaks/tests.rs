//! Play tests for Duck Peaks: `cargo test --example duck_peaks`.
//!
//! Two kinds, so painting the level never breaks a test by accident:
//!
//! - How the game plays (running, jumping, planks, walls, water, pausing)
//!   is tested on tiny levels written right here, with the same letters.
//!   Editing `level.txt` can't change them.
//! - The real level is tested for what must stay true: its legend's rules
//!   hold, and the bot can still win on it as it is now. (Duck Forge
//!   Studio's Check also says when a coin looks out of reach, with
//!   `map.reach`: a cautious reckoning, so a warning rather than a test.)

use super::*;

/// The game on its real level, with its art loaded.
fn game() -> DuckPeaks {
    DuckPeaks::new(Art::load().unwrap())
}

/// The game on a tiny level of its own (same letters as `level.txt`).
fn game_on(level: &str) -> DuckPeaks {
    let legend = TileMap::load_level(LEVEL).unwrap().legend().clone();
    let map = TileMap::parse(level, TILE, &legend).unwrap();
    DuckPeaks::on_map(Art::load().unwrap(), map)
}

#[test]
fn running_right_moves_the_duck_and_keeps_it_on_the_ground() {
    let level = "
..p....c....................
############################";
    let played = PlayTest::new(config(), game_on(level))
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
    let level = "
....................
..p.................
####################";
    let played = PlayTest::new(config(), game_on(level))
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
    // A plank three tiles up (its top at y = 48), with a coin on it.
    let level = "
....................
....................
....c...............
...====.............
....................
....p...............
####################";
    let played = PlayTest::new(config(), game_on(level))
        .script(r#"{"events": [{"frame": 5, "press": "Space"}, {"frame": 6, "release": "Space"}]}"#)
        .frames(90)
        .run();
    assert!(played.flag("on_ground"));
    assert_eq!(
        played.game().player.bottom(),
        3.0 * TILE,
        "standing on the plank"
    );
    assert_eq!(played.number("coins"), 1.0, "and the coin on it");
}

#[test]
fn a_rock_wall_stops_the_duck() {
    // Rock two tiles high at columns 8 and 9.
    let level = "
....................
........RR..........
..p.....RR..........
####################";
    let played = PlayTest::new(config(), game_on(level))
        .script(r#"{"events": [{"frame": 1, "press": "Right"}]}"#)
        .frames(90)
        .run();
    assert_eq!(played.game().player.right(), 8.0 * TILE);
}

#[test]
fn falling_in_the_water_goes_back_to_the_start() {
    let level = "
....................
p...................
#####~~~############
ddddd~~~dddddddddddd";
    let mut game = game_on(level);
    game.put_player_at(vec2(6.5 * TILE, 2.0 * TILE - 1.0)); // over the water
    let played = PlayTest::new(config(), game).frames(60).run();
    assert_eq!(played.number("splashes"), 1.0);
    assert_eq!(played.sound_frames("hit").len(), 1);
    assert_eq!(played.game().feet(), start_of(&played.game().map));
}

#[test]
fn pausing_freezes_the_game() {
    let level = "
..p.................................
####################################";
    let played = PlayTest::new(config(), game_on(level))
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

// ---- The real level, as it is now ----

#[test]
fn the_level_keeps_its_rules() {
    let map = TileMap::load_level(LEVEL).unwrap();
    assert_eq!(map.broken_rules(), Vec::<String>::new());
}

#[test]
fn the_bot_can_still_win_on_the_level() {
    let mut sim = Simulation::new(config(), game());
    let coins = sim.game().coins.len() as f64;
    super::bot::play(&mut sim);
    let feet = sim.game().feet();
    assert_eq!(
        sim.text("state"),
        "Won",
        "the bot got stuck around column {} row {}",
        (feet.x / TILE).floor(),
        (feet.y / TILE).floor() - 1.0
    );
    assert_eq!(sim.number("coins"), coins, "with every coin");
}

#[test]
fn the_playthrough_reaches_the_flag_with_every_coin() {
    // It replays on the level it was recorded on (it keeps a copy), so
    // painting the level doesn't break it. Check says when it's from an
    // older level, and the bot can record it again.
    let played = PlayTest::new(config(), game())
        .inputs("playthroughs/duck_peaks.json")
        .run();
    assert_eq!(played.text("state"), "Won");
    assert_eq!(played.number("coins"), 10.0);
    // The music plays all the way through, looping.
    let music = &played.loops()[0];
    assert_eq!((music.name.as_str(), music.start), ("music", 1));
    assert!(music.is_playing());
}

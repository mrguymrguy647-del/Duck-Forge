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

/// The Feel values written in the code (the Studio's Feel sliders, a
/// tune file or `--set` can change them).
fn feel_in_the_code() -> std::collections::BTreeMap<String, f32> {
    [
        ("run_speed", RUN_SPEED),
        ("jump_speed", JUMP_SPEED),
        ("gravity", GRAVITY),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect()
}

#[test]
fn the_bot_can_still_win_on_the_level() {
    // With the Feel values as they are now (the tune file, if there is one).
    let mut sim = Simulation::new(config(), game());
    let coins = sim.game().coins.len() as f64;
    if let Err(why) = super::bot::play(&mut sim) {
        // Is it the level, or the Feel? Try the values written in the code.
        let mut plain = Simulation::new(config(), game());
        plain.change_tunes(feel_in_the_code());
        if super::bot::play(&mut plain).is_ok() {
            panic!(
                "With the Feel values as they are now, {why}. With the values in the code the \
                 bot wins: change Feel back (the preset \"As it was made\"), or change the level \
                 to suit the new Feel."
            );
        }
        panic!("On the level as it is now, {why}.");
    }
    assert_eq!(sim.number("coins"), coins, "with every coin");
}

#[test]
fn the_bot_can_win_with_every_feel_preset() {
    // The presets in the Studio's Feel panel: each must leave a game that
    // can be won. (A preset changes some values; the rest are the code's.)
    for (preset, values) in feel_presets("examples/duck_peaks.feel.json").unwrap() {
        let mut sim = Simulation::new(config(), game());
        let mut feel = feel_in_the_code();
        feel.extend(values);
        sim.change_tunes(feel);
        if let Err(why) = super::bot::play(&mut sim) {
            panic!("With the Feel preset \"{preset}\", {why}.");
        }
    }
}

#[test]
fn the_playthrough_reaches_the_flag_with_every_coin() {
    // It replays on the level it was recorded on (it keeps a copy), so
    // painting the level doesn't break it. Check says when it's from an
    // older level, and the bot can record it again.
    let recording = "playthroughs/duck_peaks.json";
    if !std::path::Path::new(recording).exists() {
        return; // not recorded yet: Check says so, and offers to record it
    }
    let played = PlayTest::new(config(), game()).inputs(recording).run();
    assert_eq!(played.text("state"), "Won");
    assert!(played.game().coins.is_empty(), "with every coin");
}

#[test]
fn the_bot_wins_whenever_it_says_there_is_a_way() {
    // The level with random things painted on it, and random Feel values
    // (the same ones every time). The bot may say there's no way to a
    // coin; it must never set off and then fail to get there. This is
    // what keeps "the bot can still win" honest when levels and Feel
    // change. (More rounds: ROUNDS=2000 cargo test --release ...)
    let art = Art::load().unwrap();
    let legend = TileMap::load_level(LEVEL).unwrap().legend().clone();
    let paint: Vec<char> = legend
        .letters()
        .iter()
        .filter(|l| matches!(l.name.as_str(), "coin") || l.kind != LetterKind::Thing)
        .map(|l| l.letter)
        .chain(['.'])
        .collect();
    let text = std::fs::read_to_string(LEVEL).unwrap();
    let rounds = std::env::var("ROUNDS").map_or(40, |n| n.parse().unwrap());
    let mut rng = Rng::new(11);
    let (mut won, mut failed) = (0, Vec::new());
    for round in 0..rounds {
        let mut rows: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
        let (columns, last_row) = (rows[0].len() as i32, rows.len() as i32 - 3);
        for _ in 0..rng.range_i32(1, 14) {
            let column = rng.range_i32(5, columns - 2) as usize;
            let row = rng.range_i32(2, last_row) as usize;
            let letter = paint[rng.range_i32(0, paint.len() as i32) as usize];
            let length = if letter == 'c' {
                1
            } else {
                rng.range_i32(1, 5) as usize
            };
            for cell in &mut rows[row][column..(column + length).min(columns as usize - 1)] {
                if !matches!(*cell, 'p' | 'g') {
                    *cell = letter;
                }
            }
        }
        let level: String = rows
            .iter()
            .map(|row| row.iter().collect::<String>() + "\n")
            .collect();
        let map = TileMap::parse(&level, TILE, &legend).unwrap();
        let mut sim = Simulation::new(config(), DuckPeaks::on_map(art.clone(), map));
        let feel = [
            ("run_speed", rng.range(40.0, 250.0)),
            ("jump_speed", rng.range(200.0, 500.0)),
            ("gravity", rng.range(300.0, 1500.0)),
        ];
        sim.change_tunes(feel.into_iter().map(|(n, v)| (n.to_string(), v)).collect());
        match super::bot::play(&mut sim) {
            Ok(()) => won += 1,
            Err(why) if why.contains("can't get") => {}
            Err(why) => failed.push(format!("round {round}, {feel:?}: {why}")),
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
    assert!(
        won * 5 >= rounds,
        "it should win a fair share: {won} of {rounds}"
    );
}

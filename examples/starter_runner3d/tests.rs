//! Play tests for the 3D runner: `cargo test --example starter_runner3d`.

use super::*;

/// The game, with its art and sounds loaded.
fn game() -> Runner {
    Runner::new(Art::load().unwrap())
}

/// Presses `key` on `frame` and lets go on the next.
fn tap(key: &str, frame: u64) -> String {
    format!(
        r#"{{"frame": {frame}, "press": "{key}"}}, {{"frame": {}, "release": "{key}"}}"#,
        frame + 1
    )
}

#[test]
fn the_duck_runs_faster_and_faster_down_the_middle_lane() {
    let played = PlayTest::new(config(), game()).frames(60).run();
    assert_eq!(played.text("state"), "Playing");
    assert_eq!(played.number("lane"), 1.0);
    // A second at 8 units a second, speeding up a little.
    let distance = played.number("distance");
    assert!((8.0..=9.0).contains(&distance), "{distance}");
    // The hum starts on the first frame and keeps going.
    let hum = &played.loops()[0];
    assert_eq!((hum.name.as_str(), hum.start), ("hum", 1));
    assert!(hum.is_playing());
}

#[test]
fn left_and_right_change_lane_one_at_a_time() {
    let script = format!(
        r#"{{"events": [{}, {}, {}]}}"#,
        tap("Left", 5),
        tap("Left", 10),
        tap("Right", 30)
    );
    let played = PlayTest::new(config(), game())
        .script(&script)
        .frames(60)
        .run();
    let lanes: Vec<f64> = played
        .history("lane")
        .iter()
        .filter_map(|(_, value)| value.as_number())
        .collect();
    assert_eq!(
        lanes,
        [1.0, 0.0, 1.0],
        "left once (it can't go further), then back"
    );
    assert!(
        played.game().x.abs() < 0.001,
        "slid all the way back to the middle"
    );
}

#[test]
fn jumping_goes_up_and_comes_down_with_a_sound() {
    let script = format!(r#"{{"events": [{}]}}"#, tap("Space", 10));
    let played = PlayTest::new(config(), game())
        .script(&script)
        .frames(60)
        .run();
    played.assert_sound("jump", 10, 0);
    let air: Vec<bool> = played
        .history("in_air")
        .iter()
        .filter_map(|(_, value)| value.as_bool())
        .collect();
    assert_eq!(air, [false, true, false]);
}

#[test]
fn a_crate_costs_a_life_and_three_end_the_game() {
    let mut runner = game();
    // Crates right ahead in the middle lane, far enough apart that the
    // second comes after the blinking (one second) is over.
    runner.next_row = 1000.0; // no random rows in this test
    runner.crates = (1..=3)
        .map(|i| Thing {
            lane: 1,
            z: i as f32 * 12.0,
        })
        .collect();
    let played = PlayTest::new(config(), runner).frames(5 * 60).run();
    assert_eq!(played.number("lives"), 0.0);
    assert_eq!(played.text("state"), "GameOver");
    played.assert_sound("hit", 80, 20);
    // The hum stops at the game over.
    assert!(!played.loops()[0].is_playing());
}

#[test]
fn jumping_clears_a_crate() {
    let mut runner = game();
    runner.next_row = 1000.0;
    runner.crates = vec![Thing { lane: 1, z: 6.0 }];
    // The top of a jump comes 1/3 s after taking off, 2.7 units on at 8
    // units a second: jump when the crate is that far, on frame 25.
    let script = format!(r#"{{"events": [{}]}}"#, tap("Space", 25));
    let played = PlayTest::new(config(), runner)
        .script(&script)
        .frames(90)
        .run();
    assert_eq!(played.number("lives"), 3.0);
}

#[test]
fn coins_in_a_lane_are_collected() {
    let mut runner = game();
    runner.next_row = 1000.0;
    runner.coins = (0..3)
        .map(|i| Thing {
            lane: 1,
            z: 4.0 + i as f32,
        })
        .collect();
    let played = PlayTest::new(config(), runner).frames(90).run();
    assert_eq!(played.number("coins"), 3.0);
    assert_eq!(played.sound_frames("coin").len(), 3);
}

#[test]
fn every_row_leaves_a_way_through() {
    let played = PlayTest::new(config(), game()).frames(60).run();
    let runner = played.game();
    for row in runner.crates.iter().map(|c| c.z) {
        let blocked = runner.crates.iter().filter(|c| c.z == row).count();
        assert!(blocked <= 2, "row at {row} has {blocked} crates");
    }
}

#[test]
fn the_playthrough_runs_a_minute_without_a_game_over() {
    let played = PlayTest::new(config(), game())
        .inputs("playthroughs/starter_runner3d.json")
        .run();
    assert_eq!(played.text("state"), "Playing");
    assert!(
        played.number("coins") >= 20.0,
        "{} coins",
        played.number("coins")
    );
}

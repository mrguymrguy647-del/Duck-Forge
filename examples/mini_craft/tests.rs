//! Play tests for Mini Craft: `cargo test --example mini_craft`.

use super::*;

fn game() -> MiniCraft {
    new_game().unwrap()
}

/// The game with the duck standing on column (x, z).
fn game_at(x: usize, z: usize) -> MiniCraft {
    let mut game = game();
    game.x = x as f32;
    game.z = z as f32;
    game.y = game.ground();
    game
}

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
fn the_world_has_six_gems_lava_and_four_chunks() {
    let game = game();
    assert_eq!(game.world.size, 20);
    assert_eq!(game.gems.len(), 6);
    assert_eq!(game.world.start, (1, 1));
    assert_eq!(game.art.chunks.len(), 4);
    assert!(game.art.lava.glowing && !game.art.chunks[0].glowing);
    assert_eq!(
        game.art.lava.triangles.len(),
        12 * 12,
        "12 lava blocks in one mesh"
    );
}

#[test]
fn walking_right_goes_along_x_and_the_wind_loops() {
    let played = PlayTest::new(config(), game())
        .script(&hold(&["Right"], 60))
        .frames(60)
        .run();
    let x = played.number("x");
    assert!(
        (4.3..=4.6).contains(&x),
        "a second at 3.5 blocks a second, from 1: {x}"
    );
    assert_eq!(played.number("z"), 1.0);
    assert_eq!(played.loops()[0].name, "hum");
}

#[test]
fn a_pillar_stops_the_duck_and_a_step_of_one_is_climbed() {
    // The stone pillar at (14, 6): walking right from (12, 6) stops before it.
    let played = PlayTest::new(config(), game_at(12, 6))
        .script(&hold(&["Right"], 90))
        .frames(90)
        .run();
    let x = played.number("x");
    assert!((13.1..13.3).contains(&x), "stopped at the pillar: {x}");
    // From (11, 14) walking right: up the step at (12, 14) and onto the
    // plateau (height 2).
    let played = PlayTest::new(config(), game_at(11, 14))
        .script(&hold(&["Right"], 90))
        .frames(90)
        .run();
    assert!(played.number("x") > 13.0);
    assert_eq!(played.game().y, 2.0, "standing on the plateau");
}

#[test]
fn lava_sends_the_duck_back_to_the_start() {
    // At (6, 9), walking right into the lava at (7, 9) (half a block
    // away: about 9 frames), letting go just after.
    let script =
        r#"{"events": [{"frame": 1, "press": "Right"}, {"frame": 12, "release": "Right"}]}"#;
    let played = PlayTest::new(config(), game_at(6, 9))
        .script(script)
        .frames(30)
        .run();
    assert_eq!(played.number("burns"), 1.0);
    played.assert_sound("hit", 10, 10);
    // Back at the start, (1, 1) (give or take the last step before letting go).
    let (x, z) = (played.number("x"), played.number("z"));
    assert!((1.0..1.2).contains(&x) && z == 1.0, "({x}, {z})");
}

#[test]
fn a_gem_is_collected_by_walking_into_it() {
    // The gem at (7, 5), from (5, 5).
    let played = PlayTest::new(config(), game_at(5, 5))
        .script(&hold(&["Right"], 60))
        .frames(60)
        .run();
    assert_eq!(played.number("gems"), 1.0);
    assert_eq!(played.sound_frames("coin").len(), 1);
}

#[test]
fn the_duck_hides_behind_the_pillar() {
    // Standing at the gem (10, 14), the pillar at (9, 13) is between the
    // duck and the camera: the duck's middle on screen shows stone, not duck.
    let game = game_at(10, 14);
    let mut sim = Simulation::new(config(), game);
    sim.step();
    let camera = sim.game().camera();
    let middle = vec3(10.0, 1.5, 14.0);
    let (point, _) = camera.project(middle, sim.picture()).unwrap();
    let picture = sim.picture();
    let pixel = picture.get_pixel(point.x as i32, point.y as i32).unwrap();
    let yellow = pixel.r > 200 && pixel.g > 150 && pixel.b < 100;
    assert!(!yellow, "the duck should be hidden: {pixel:?}");
    // Two blocks to the right, nothing is in front: the duck shows.
    let mut sim = Simulation::new(config(), game_at(12, 12));
    sim.step();
    let camera = sim.game().camera();
    let (point, _) = camera
        .project(vec3(12.0, 1.4, 12.0), sim.picture())
        .unwrap();
    let pixel = sim
        .picture()
        .get_pixel(point.x as i32, point.y as i32)
        .unwrap();
    assert!(pixel.r > 200 && pixel.g > 150, "the duck shows: {pixel:?}");
}

#[test]
fn the_playthrough_collects_every_gem() {
    let played = PlayTest::new(config(), game())
        .inputs("playthroughs/mini_craft.json")
        .run();
    assert_eq!(played.text("state"), "Won");
    assert_eq!(played.number("gems"), 6.0);
    assert_eq!(played.number("burns"), 0.0);
}

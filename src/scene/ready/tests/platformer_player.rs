//! The Platformer player, played: standing, running, jumping as high as
//! its setting says, through a one-way plank and onto it, back from
//! falling out, and reading actions rather than keys.

use std::path::Path;

use super::super::platformer_player::PlatformerPlayer;
use crate::prelude::*;
use crate::simulation::Simulation;
use crate::test_folder::{TestFolder, read_shared};

/// A game with no code and no scripts of its own: ground with a gap at
/// the right, a one-way plank, and a duck that is a Platformer player.
/// `duck` is the rest of the duck's line (its settings).
fn game(name: &str, duck: &str) -> Simulation<SceneGame> {
    let folder = TestFolder::new(name);
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    std::fs::create_dir_all(folder.join("scenes")).unwrap();
    for file in ["tiles.json", "tiles.png", "hero.json", "hero.png"] {
        let from = Path::new("examples/template/assets").join(file);
        std::fs::write(folder.join("assets").join(file), read_shared(&from)).unwrap();
    }
    // 10 by 6 tiles of 16 pixels: the plank's top is at 32, the ground's
    // at 80, and the last two columns are a hole.
    folder.file(
        "level.txt",
        "..........\n..........\n....===...\n..........\n..........\n########..\n",
    );
    folder.file(
        "level.legend.json",
        r##"{"tile_size": 16, "tiles": "assets/tiles.json", "letters": {
            "#": {"kind": "solid", "name": "grass"},
            "=": {"kind": "one_way", "name": "plank"}}}"##,
    );
    folder.file(
        "scenes/level1.json",
        &format!(
            r##"{{"duckforge_scene": 1, "size": [160, 96], "objects": [
            {{"id": 1, "name": "ground", "kind": "tilemap", "level": "level.txt"}},
            {{"id": 2, "name": "duck", "kind": "platformer_player", "at": [88, 80], "picture": "assets/hero.json", "box": [3, 2, 10, 14]{duck}}}]}}"##
        ),
    );
    let game = SceneGame::new(&*folder);
    let problems = game.scene().unwrap().problems().to_vec();
    assert!(problems.is_empty(), "{problems:?}");
    let config = Config {
        width: 160,
        height: 96,
        ..Config::default()
    };
    Simulation::new(config, game)
}

fn duck(sim: &Simulation<SceneGame>) -> &Object {
    let scene = sim.game().scene().unwrap();
    &scene[scene.find("duck").unwrap()]
}

fn player(sim: &Simulation<SceneGame>) -> &PlatformerPlayer {
    let scene = sim.game().scene().unwrap();
    scene.script(scene.find("duck").unwrap()).unwrap()
}

/// Taps a key: held for one update.
fn tap(sim: &mut Simulation<SceneGame>, key: Key) {
    sim.press(key);
    sim.step();
    sim.release(key);
}

/// Jumps, holding the key, and gives how high its feet went.
fn jump_from_the_ground(sim: &mut Simulation<SceneGame>, key: Key) -> f32 {
    let ground = duck(sim).at.y;
    let mut highest = ground;
    sim.press(key);
    for _ in 0..90 {
        sim.step();
        highest = highest.min(duck(sim).at.y);
    }
    sim.release(key);
    ground - highest
}

#[test]
fn it_stands_on_the_ground_with_no_code_and_no_scripts() {
    let mut sim = game("ready-player-stands", "");
    sim.run(30);
    assert_eq!(duck(&sim).at.y, 80.0, "it stands on the ground");
    assert!(player(&sim).on_ground());
    assert_eq!(duck(&sim).animation.as_deref(), Some("idle"));
}

#[test]
fn it_runs_at_its_speed_and_faces_the_way_it_runs() {
    let mut sim = game("ready-player-runs", r#", "at": [24, 80], "run_speed": 60"#);
    sim.run(5);
    sim.press(Key::Right);
    sim.run(30); // past the speed-up (0.12 s)
    assert_eq!(player(&sim).velocity().x, 60.0);
    let x = duck(&sim).at.x;
    sim.run(30);
    assert!(
        (duck(&sim).at.x - x - 30.0).abs() < 0.01,
        "half a second at 60"
    );
    assert_eq!(duck(&sim).animation.as_deref(), Some("run"));
    assert!(!duck(&sim).flip);
    sim.release(Key::Right);
    sim.press(Key::A); // A is "left" too
    sim.run(2);
    assert!(duck(&sim).flip, "it faces left");
}

#[test]
fn it_jumps_as_high_as_its_setting_says() {
    // The usual 60 pixels; 40 when the scene file says so.
    for (says, high) in [("", 60.0), (r#", "jump_height": 40"#, 40.0)] {
        let mut sim = game("ready-player-jumps", says);
        sim.run(10);
        let rose = jump_from_the_ground(&mut sim, Key::Space);
        assert!((rose - high).abs() <= 2.0, "{says}: rose {rose}");
    }
    // Let go at once: a short hop.
    let mut sim = game("ready-player-hops", "");
    sim.run(10);
    let ground = duck(&sim).at.y;
    tap(&mut sim, Key::Space);
    let mut highest = ground;
    for _ in 0..60 {
        sim.step();
        highest = highest.min(duck(&sim).at.y);
    }
    assert!(ground - highest < 40.0, "rose {}", ground - highest);
}

#[test]
fn it_jumps_up_through_a_one_way_plank_and_lands_on_it() {
    // Under the plank: high enough to go through it from below...
    let mut sim = game("ready-player-plank", r#", "jump_height": 64"#);
    sim.run(10);
    sim.press(Key::Up);
    sim.run(90);
    sim.release(Key::Up);
    // ...and it lands on its top, and stands there.
    assert_eq!(duck(&sim).at.y, 32.0);
    assert!(player(&sim).on_ground());
    sim.run(30);
    assert_eq!(duck(&sim).at.y, 32.0);
}

#[test]
fn falling_out_of_the_scene_puts_it_back_where_it_started() {
    let mut sim = game("ready-player-falls", r#", "at": [100, 80]"#);
    sim.run(5);
    sim.press(Key::Right);
    let mut fell = false;
    for _ in 0..240 {
        sim.step();
        fell |= duck(&sim).at.y > 96.0;
        if fell && duck(&sim).at.y <= 80.0 {
            break;
        }
    }
    sim.release(Key::Right);
    assert!(fell, "it ran into the hole");
    assert_eq!(duck(&sim).at, vec2(100.0, 80.0));
    // It never leaves by the scene's right end: that's a wall.
    let mut sim = game(
        "ready-player-wall",
        r#", "at": [100, 80], "falls_back": false"#,
    );
    sim.press(Key::Right);
    sim.run(120);
    assert!(duck(&sim).at.x <= 160.0 - 5.0, "{}", duck(&sim).at.x);
}

#[test]
fn it_reads_actions_so_a_game_can_give_them_other_keys() {
    let mut sim = game("ready-player-actions", "");
    sim.run(10);
    sim.with_context(|_, ctx| ctx.input.bind("jump", &[Key::J]));
    assert!(
        jump_from_the_ground(&mut sim, Key::Space) < 1.0,
        "Space isn't jump now"
    );
    assert!(jump_from_the_ground(&mut sim, Key::J) > 50.0);
    // An object can name another action for its jump.
    let mut sim = game("ready-player-hop", r#", "jump": "hop""#);
    sim.run(10);
    sim.with_context(|_, ctx| ctx.input.bind("hop", &[Key::K]));
    assert!(jump_from_the_ground(&mut sim, Key::K) > 50.0);
}

#[test]
fn its_settings_are_listed_for_the_inspector_with_the_studios_words() {
    let scripts = Scripts::new();
    let names: Vec<String> = scripts
        .settings_of("platformer_player")
        .unwrap()
        .iter()
        .map(|setting| setting.name.clone())
        .collect();
    assert_eq!(&names[..3], ["run_speed", "jump_height", "gravity"]);
    let json = scripts.to_json();
    let ready = json
        .as_array()
        .unwrap()
        .iter()
        .find(|script| script["name"] == "platformer_player")
        .unwrap();
    assert_eq!(ready["ready_made"], true);
    assert_eq!(ready["label"], "Platformer player");
    assert_eq!(ready["type"], "PlatformerPlayer");
}

#[test]
fn it_plays_the_same_every_time() {
    let run = || {
        let mut sim = game("ready-player-same", "");
        for frame in 0..300u32 {
            for (key, from, to) in [
                (Key::Right, 20, 140),
                (Key::Space, 60, 90),
                (Key::Left, 150, 260),
            ] {
                if frame == from {
                    sim.press(key);
                }
                if frame == to {
                    sim.release(key);
                }
            }
            sim.step();
        }
        (duck(&sim).at, player(&sim).velocity())
    };
    assert_eq!(run(), run());
}

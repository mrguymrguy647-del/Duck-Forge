//! Play tests for the ready-made game: `cargo test --example ready_made`.

use super::*;
use duckforge::{InputScript, Simulation};

const SCENE: &str = "examples/ready_made/scenes/level1.json";

fn game() -> SceneGame {
    let game = SceneGame::new("examples/ready_made");
    let problems = game.scene().expect("the scene loads").problems().to_vec();
    assert!(problems.is_empty(), "{problems:?}");
    game
}

fn duck(sim: &Simulation<SceneGame>) -> Object {
    let scene = sim.game().scene().unwrap();
    scene[scene.find("duck").unwrap()].clone()
}

#[test]
fn the_scene_file_reads_without_mistakes_and_is_written_the_duck_forge_way() {
    let text = std::fs::read_to_string(SCENE)
        .unwrap()
        .replace("\r\n", "\n");
    let scene = duckforge::scene::SceneFile::parse(&text).unwrap();
    assert_eq!(scene.problems, [], "what the file says is wrong");
    assert_eq!(
        scene.to_text(),
        text,
        "one object per line, as Duck Forge writes it"
    );
    let game = game();
    let scene = game.scene().unwrap();
    assert_eq!(scene[scene.find("duck").unwrap()].kind, "platformer_player");
}

/// Plays the level as a person would, watching where the duck is: under
/// the plank, a jump straight up through it onto its top; then off its
/// end, and up the bricks with a jump each time it's stopped.
fn play_it_through() -> Simulation<SceneGame> {
    let mut sim = Simulation::new(Config::default(), game());
    sim.run(10);
    assert_eq!(duck(&sim).at.y, 208.0, "it stands on the ground");
    sim.press(Key::Right);
    while duck(&sim).at.x < 136.0 {
        sim.step();
    }
    sim.release(Key::Right);
    sim.press(Key::Space);
    sim.run(60);
    sim.release(Key::Space);
    sim.run(20);
    assert_eq!(duck(&sim).at.y, 160.0, "on the plank");
    sim.press(Key::Right);
    while duck(&sim).at.x < 270.0 {
        let x = duck(&sim).at.x;
        sim.step();
        if duck(&sim).at.x == x {
            // Stopped by a brick: a full jump, holding Up.
            sim.press(Key::Up);
            sim.run(20);
            sim.release(Key::Up);
        }
    }
    sim.release(Key::Right);
    sim.run(30);
    sim
}

#[test]
fn the_duck_jumps_onto_the_plank_and_climbs_the_bricks() {
    let at = duck(&play_it_through()).at;
    assert!(
        at.x > 260.0 && at.y == 208.0,
        "over the bricks and down: {at:?}"
    );
}

#[test]
fn the_playthrough_plays_the_same_every_time() {
    let script = InputScript::load("playthroughs/ready_made.json").unwrap();
    let play = || {
        let mut sim = Simulation::new(Config::default(), game());
        sim.follow(script.clone());
        sim.run(script.frames.unwrap_or(600));
        duck(&sim).at
    };
    let end = play();
    assert_eq!(end, play());
    assert_eq!(
        end,
        duck(&play_it_through()).at,
        "the same as playing it now"
    );
}

/// Writes `playthroughs/ready_made.json` from `play_it_through`:
/// `cargo test --example ready_made -- --ignored`.
#[test]
#[ignore]
fn record_the_playthrough() {
    let sim = play_it_through();
    sim.recording()
        .save("playthroughs/ready_made.json")
        .unwrap();
}

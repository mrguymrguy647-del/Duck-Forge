//! A bot that plays the scene as it is now, and records the playthrough:
//! `cargo test --example scene_demo -- --ignored`.
//!
//! It looks at the game, not at a list of keys, so it still wins after
//! the scene is changed: it runs right, and jumps when something is in
//! its way or a coin is overhead.

use super::*;

/// Plays the scene and keeps the run as `playthroughs/scene_demo.json`:
/// `cargo test --example scene_demo -- --ignored`
#[test]
#[ignore]
fn record_the_playthrough() {
    let sim = play(SceneDemo::new().unwrap()).unwrap_or_else(|why| panic!("{why}"));
    sim.recording()
        .save("playthroughs/scene_demo.json")
        .unwrap();
}

/// Plays `game` to the flag. `Err` says, in plain words, why it couldn't.
pub fn play(game: SceneDemo) -> Result<Simulation<SceneDemo>, String> {
    let mut sim = Simulation::new(config(), game);
    sim.press(Key::Right);
    let mut stuck_for = 0;
    for _ in 0..60 * 60 {
        let game = sim.game();
        if game.won {
            sim.run(60); // a second to look at the last picture
            return Ok(sim);
        }
        let Some(duck) = game.scene.find("duck") else {
            return Err(String::from(
                "the scene has no object called \"duck\" to play",
            ));
        };
        let body = game.scene.rect(duck);
        // Something in the way: it's standing and not getting anywhere.
        let blocked = game.on_ground && game.velocity.x.abs() < 1.0;
        stuck_for = if blocked { stuck_for + 1 } else { 0 };
        // A coin overhead, close enough to jump for.
        let coin_overhead = game.scene.all("coin").into_iter().any(|coin| {
            let coin = game.scene.rect(coin);
            coin.bottom() < body.top()
                && body.top() - coin.bottom() < 56.0
                && (coin.center().x - body.center().x).abs() < 24.0
        });
        if game.on_ground && (stuck_for > 3 || coin_overhead) {
            sim.press(Key::Space);
        } else {
            sim.release(Key::Space);
        }
        sim.step();
    }
    let game = sim.game();
    let x = game
        .scene
        .find("duck")
        .map_or(0.0, |duck| game.scene[duck].at.x.round());
    Err(format!(
        "the bot ran right for a minute and didn't reach the flag: it's at x = {x}. Is there \
         a flag, and is anything in the way too tall to jump (about 60 pixels)?"
    ))
}

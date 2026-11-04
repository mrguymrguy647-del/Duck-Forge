//! A bot that plays starter_shooter and records playthroughs/starter_shooter.json:
//! `cargo test --example starter_shooter -- --ignored`.

use super::*;

/// Makes playthroughs/starter_shooter.json: a bot holds fire, flies
/// under the nearest enemy, and steps aside from shots about to hit.
/// `cargo test --example starter_shooter -- --ignored`
#[test]
#[ignore]
fn record_playthrough() {
    let mut sim = Simulation::new(config(), Shooter::new());
    sim.press(Key::Space);
    sim.step();
    sim.release(Key::Space);
    sim.step();
    sim.press(Key::Space); // and hold it: auto-fire
    while sim.text("state") == "Playing" && sim.context().frame() < 90 * 60 {
        let game = sim.game();
        let me = game.ship;
        let danger = game
            .shots
            .iter()
            .find(|s| s.y < me.y && s.y > me.y - 70.0 && (s.x - me.x).abs() < 12.0);
        let target = match danger {
            Some(shot) if shot.x > me.x || me.x > WIDTH - 30.0 => me.x - 30.0,
            Some(_) => me.x + 30.0,
            None => game
                .enemies
                .iter()
                .filter(|e| e.flight == Flight::InPlace)
                .map(|e| e.body().center().x)
                .min_by(|a, b| (a - me.x).abs().total_cmp(&(b - me.x).abs()))
                .unwrap_or(WIDTH / 2.0),
        };
        let keys = [
            (Key::Left, target < me.x - 2.0),
            (Key::Right, target > me.x + 2.0),
        ];
        for (key, held) in keys {
            if held {
                sim.press(key);
            } else {
                sim.release(key);
            }
        }
        sim.step();
    }
    for key in [Key::Left, Key::Right, Key::Space] {
        sim.release(key);
    }
    sim.run(90); // the explosion and GAME OVER
    sim.recording()
        .save("playthroughs/starter_shooter.json")
        .unwrap();
}

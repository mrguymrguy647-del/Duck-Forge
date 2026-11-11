//! A bot that plays Duck Craft and records playthroughs/duck_craft.json:
//! `cargo test --example duck_craft -- --ignored record_playthrough`.
//!
//! It looks round, digs a hole, fills it with planks, puts a lamp on it,
//! builds a pillar under itself by jumping and placing, then steps back to
//! look at the duck from behind and walks off the pillar.

use super::*;
use std::f32::consts::PI;

/// Holds `key` until `done` says so (at most `limit` frames).
fn hold_until(
    sim: &mut Simulation<DuckCraft>,
    key: Key,
    limit: u64,
    done: impl Fn(&DuckCraft) -> bool,
) {
    sim.press(key);
    for _ in 0..limit {
        if done(sim.game()) {
            break;
        }
        sim.step();
    }
    sim.release(key);
    sim.step();
}

fn tap(sim: &mut Simulation<DuckCraft>, key: Key) {
    sim.press(key);
    sim.step();
    sim.release(key);
    sim.run(6);
}

#[test]
#[ignore]
fn record_playthrough() {
    // From the title, as the game starts: Enter for a survival island.
    let mut sim = Simulation::new(config(), title_game(None).unwrap());
    sim.run(30);
    tap(&mut sim, Key::Enter);
    sim.run(40);

    // Look round: right a little, then down at the ground in front.
    hold_until(&mut sim, Key::Right, 50, |_| false);
    sim.run(10);
    hold_until(&mut sim, Key::Down, 60, |g| g.pitch < -0.85);
    sim.run(10);

    // Dig two blocks: a hole.
    hold_until(&mut sim, Key::X, 200, |g| g.dug >= 2);
    assert_eq!(sim.game().dug, 2, "the bot should dig two blocks");
    sim.run(20);

    // Planks (slot 1), and fill the hole back in.
    tap(&mut sim, Key::Digit1);
    for _ in 0..2 {
        tap(&mut sim, Key::C);
        sim.run(10);
    }
    sim.run(20);

    // A lamp on top of the filled hole, then planks again.
    tap(&mut sim, Key::Digit4);
    tap(&mut sim, Key::C);
    assert_eq!(
        sim.game().count(Item::Block(Block::Lamp)),
        7,
        "the lamp is placed"
    );
    tap(&mut sim, Key::Digit1);
    sim.run(20);

    // A pillar: look straight down, jump, and place a block under the duck
    // at the top of each jump.
    hold_until(&mut sim, Key::Down, 60, |g| g.pitch <= -1.5);
    for _ in 0..4 {
        let floor = sim.game().position.y;
        sim.press(Key::Space);
        sim.step();
        sim.release(Key::Space);
        for _ in 0..30 {
            if sim.game().position.y > floor + 1.05 {
                break;
            }
            sim.step();
        }
        tap(&mut sim, Key::C);
        sim.run(25);
    }

    // Look from behind the duck, turn round and walk off the pillar.
    tap(&mut sim, Key::V);
    hold_until(&mut sim, Key::Up, 60, |g| g.pitch > -0.35);
    hold_until(&mut sim, Key::Left, 80, |_| false);
    hold_until(&mut sim, Key::W, 90, |_| false);
    sim.run(60);

    let game = sim.game();
    assert_eq!(game.placed, 7, "two planks, a lamp and a pillar of four");
    assert!(game.behind_view);
    sim.recording()
        .save("playthroughs/duck_craft.json")
        .unwrap();
}

/// The bot that records the night showcase: `cargo test --example
/// duck_craft -- --ignored record_night_showcase`, then render it with the
/// same start time:
///
///     cargo run --release --no-default-features --example duck_craft -- --headless \
///         --set start_time_of_day=0.47 --inputs playthroughs/duck_craft_night.json \
///         --video out/night.mp4 --out out/night
///
/// From the title, a survival island just before sunset. It sets three
/// lamps round itself, waits for the dark, looks back at the Watcher when
/// it shows itself, then goes out to meet a Hollow with its wings.
#[test]
#[ignore]
fn record_night_showcase() {
    std::fs::create_dir_all("out").unwrap();
    std::fs::write(
        "out/night_showcase.tune.json",
        r#"{"start_time_of_day": 0.47}"#,
    )
    .unwrap();
    let mut config = config();
    config.tune_file = Some("out/night_showcase.tune.json".into());
    let mut sim = Simulation::new(config, title_game(None).unwrap());
    sim.run(200); // the title, turning
    tap(&mut sim, Key::Enter);
    sim.run(90);
    let home = sim.game().position;

    // Turn the view by mouse, a little each frame, towards (yaw, pitch).
    fn steer(sim: &mut Simulation<DuckCraft>, yaw: f32, pitch: f32, rate: f32) -> bool {
        let g = sim.game();
        let dyaw = (yaw - g.yaw + PI).rem_euclid(TAU) - PI;
        let dpitch = pitch - g.pitch;
        let step = |d: f32| d.clamp(-rate, rate);
        sim.move_mouse_by(vec2(step(dyaw) / MOUSE_LOOK, -step(dpitch) / MOUSE_LOOK));
        sim.step();
        dyaw.abs() < 0.03 && dpitch.abs() < 0.03
    }
    fn look_at(sim: &mut Simulation<DuckCraft>, at: Vec3, rate: f32) -> bool {
        let to = at - sim.game().eye();
        let yaw = to.x.atan2(to.z);
        let pitch = (to.y / vec2(to.x, to.z).length().max(0.01)).atan();
        steer(sim, yaw, pitch, rate)
    }

    // Three lamps round where it stands.
    tap(&mut sim, Key::Digit4);
    let start_yaw = sim.game().yaw;
    for k in 0..3 {
        let yaw = start_yaw + k as f32 * TAU / 3.0 + 0.4;
        for _ in 0..90 {
            if steer(&mut sim, yaw, -0.75, 0.05) {
                break;
            }
        }
        sim.run(10);
        tap(&mut sim, Key::C);
        sim.run(30);
    }
    // Waiting for the dark, looking out over the island.
    while sim.game().time_of_day() < 0.505 {
        steer(&mut sim, start_yaw + 0.6, -0.05, 0.004);
    }

    // The night: watch the Hollows come; turn to the Watcher a few seconds
    // after it shows itself.
    let mut seen_for = 0;
    let mut frames = 0;
    let mut watcher_done = false;
    while frames < 60 * 40 {
        frames += 1;
        let g = sim.game().clone();
        if let Some(w) = &g.watcher {
            seen_for += 1;
            if seen_for > 150 {
                let head = w.position + vec3(0.0, 1.6 * night::WATCHER_SIZE, 0.0);
                look_at(&mut sim, head, 0.03);
                continue;
            }
        } else if seen_for > 0 {
            watcher_done = true;
        }
        let nearest = g
            .hollows
            .iter()
            .filter(|h| h.fading.is_none())
            .min_by(|a, b| {
                a.position
                    .distance(g.position)
                    .total_cmp(&b.position.distance(g.position))
            });
        // Only one it can see (not behind a lamp or a hill).
        let visible = |at: Vec3| {
            let to = at - g.eye();
            g.world
                .raycast(g.eye(), to.normalized(), to.length())
                .is_none()
        };
        match nearest {
            Some(h)
                if h.position.distance(g.position) < 20.0
                    && visible(h.position + vec3(0.0, 1.4, 0.0)) =>
            {
                look_at(&mut sim, h.position + vec3(0.0, 1.4, 0.0), 0.02);
            }
            _ => {
                let yaw = g.yaw + 0.004;
                steer(&mut sim, yaw, -0.05, 0.01);
            }
        }
        if watcher_done && frames > 60 * 25 {
            break;
        }
    }

    // Out of the light, to meet one. Seen from behind, then from the eyes.
    tap(&mut sim, Key::V);
    let mut fight = 0;
    while fight < 60 * 30 && sim.game().kills == 0 && !sim.game().vitals.fainted {
        fight += 1;
        if fight == 60 * 12 {
            sim.press(Key::V);
            sim.step();
            sim.release(Key::V);
        }
        let g = sim.game().clone();
        let target = g
            .hollows
            .iter()
            .filter(|h| h.fading.is_none())
            .min_by(|a, b| {
                a.position
                    .distance(g.position)
                    .total_cmp(&b.position.distance(g.position))
            })
            .map(|h| h.position);
        let hurt = g.vitals.health < 8;
        match target {
            Some(at) if !hurt => {
                let close = vec2(at.x - g.position.x, at.z - g.position.z).length();
                if close > 2.2 {
                    sim.press(Key::W);
                } else {
                    sim.release(Key::W);
                }
                if g.aimed_hollow_now.is_some() {
                    sim.press(Key::X);
                } else {
                    sim.release(Key::X);
                }
                look_at(&mut sim, at + vec3(0.0, 1.3, 0.0), 0.06);
            }
            _ => {
                // Back to the light.
                sim.release(Key::X);
                let to = home - g.position;
                if vec2(to.x, to.z).length() > 1.0 {
                    sim.press(Key::W);
                    steer(&mut sim, to.x.atan2(to.z), -0.1, 0.08);
                } else {
                    sim.release(Key::W);
                    sim.step();
                }
            }
        }
    }
    sim.release(Key::W);
    sim.release(Key::X);
    sim.run(240);
    let g = sim.game();
    eprintln!(
        "NIGHT SHOWCASE: {} frames, watcher seen {}, kills {}, health {}, hollows {}",
        sim.context().frame(),
        watcher_done,
        g.kills,
        g.vitals.health,
        g.hollows.len()
    );
    sim.recording()
        .save("playthroughs/duck_craft_night.json")
        .unwrap();
}

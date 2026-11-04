//! Play tests for starter_shooter: `cargo test --example starter_shooter`.

use super::*;

/// Press Space on frame 1 to start, then hold `held` from frame 10.
fn start_then_hold(held: &[&str]) -> String {
    let mut events = vec![
        String::from(r#"{"frame": 1, "press": "Space"}"#),
        String::from(r#"{"frame": 2, "release": "Space"}"#),
    ];
    for key in held {
        events.push(format!(r#"{{"frame": 10, "press": "{key}"}}"#));
    }
    format!(r#"{{"events": [{}]}}"#, events.join(", "))
}

/// A game already playing, with one enemy in place straight above the ship.
fn one_enemy_above() -> Shooter {
    let mut game = Shooter::new();
    game.state = State::Playing;
    game.wave = 1;
    let pos = vec2(game.ship.x - ENEMY_SIZE.x / 2.0, 60.0);
    game.enemies = vec![Enemy {
        pos,
        home: pos,
        hp: 1,
        flight: Flight::Diving(Vec2::ZERO), // stays put
    }];
    game
}

#[test]
fn it_waits_for_space() {
    let played = PlayTest::new(config(), Shooter::new()).frames(300).run();
    assert_eq!(played.text("state"), "Ready");
    assert_eq!(played.number("wave"), 0.0);
}

#[test]
fn holding_fire_shoots_at_the_fire_rate() {
    let played = PlayTest::new(config(), Shooter::new())
        .script(&start_then_hold(&["Space"]))
        .tune("enemy_fire", 0.0)
        .frames(70)
        .run();
    // Frames 10 to 70 is one second: one shot every 0.12 s.
    let shots = played.sound_frames("laser").len();
    assert!((8..=9).contains(&shots), "{shots} shots");
}

#[test]
fn even_very_fast_bullets_hit() {
    // At 3000 pixels per second a bullet moves 50 pixels a frame, five
    // times the enemy's height. A plain overlap test would miss.
    let played = PlayTest::new(config(), one_enemy_above())
        .script(r#"{"events": [{"frame": 1, "press": "Space"}]}"#)
        .tune("bullet_speed", 3000.0)
        .tune("enemy_fire", 0.0)
        .frames(10)
        .run();
    assert_eq!(played.number("score"), 10.0);
    played.assert_sound("pop", 1, 5);
}

#[test]
fn clearing_a_wave_starts_the_next() {
    let played = PlayTest::new(config(), one_enemy_above())
        .script(r#"{"events": [{"frame": 1, "press": "Space"}]}"#)
        .tune("enemy_fire", 0.0)
        .frames(150)
        .run();
    assert_eq!(played.number("wave"), 2.0);
    // Wave 2 arrives with 8 enemies (and the ship keeps shooting them).
    let counts: Vec<f64> = played
        .history("enemies")
        .iter()
        .filter_map(|(_, n)| n.as_number())
        .collect();
    assert_eq!(counts[..3], [1.0, 0.0, 8.0], "1, shot it, then 8");
}

#[test]
fn a_hit_costs_a_life_explodes_and_gives_a_moment_of_safety() {
    let mut game = one_enemy_above();
    game.shots = vec![game.ship]; // an enemy shot right on the ship
    let mut sim = Simulation::new(config(), game);
    sim.step();
    assert_eq!(sim.number("lives"), 2.0);
    sim.assert_sound("hit", 1, 0);
    assert!(!sim.game().particles.is_empty(), "an explosion");
    for _ in 0..60 {
        let ship = sim.game().ship;
        sim.game_mut().shots.push(ship); // keep shooting it
        sim.step();
    }
    assert_eq!(sim.number("lives"), 2.0, "safe for {SAFE_TIME} s");
}

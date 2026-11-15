//! Events: the moments a Duck Mold animation's keys mark (`footstep`),
//! as `AnimatedModel::events` lists them after each update.
//!
//! The rule a game counts on: **each event is heard exactly once each
//! time the animation passes its key**, however the time is cut into
//! updates. These tests play every example animation with updates of
//! many sizes, forwards and backwards, looping and once, and check what
//! is heard against a list worked out here from the events alone.

use std::path::PathBuf;

mod common;
use common::TestFolder;

use duckforge::LoadError;
use duckforge::glb::{AnimatedModel, GlbEvent, GlbModel};

fn fixtures(test: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(test);
    let models = folder.fixture("duck_mold");
    (folder, models)
}

/// Every animation of the example models.
const ANIMATED: [(&str, &str); 8] = [
    ("robot", "idle"),
    ("robot", "walk"),
    ("robot", "wave"),
    ("robot", "jump"),
    ("door", "open"),
    ("cart", "roll"),
    ("gnome", "walk"),
    ("gnome", "wave"),
];

/// Updates that add up to `total` seconds, of sizes that repeat in a
/// pattern: some tiny, some a frame, some long.
fn steps(total: f32, pattern: &[f32]) -> Vec<f32> {
    let mut steps = Vec::new();
    let mut sum = 0.0;
    for &step in pattern.iter().cycle() {
        if sum + step >= total {
            steps.push(total - sum);
            break;
        }
        steps.push(step);
        sum += step;
    }
    steps
}

/// The patterns of update sizes tried.
const PATTERNS: [&[f32]; 5] = [
    &[1.0 / 30.0],
    &[1.0 / 60.0, 1.0 / 144.0],
    &[0.013, 0.071, 0.2],
    &[0.37],
    &[0.05, 0.6, 0.001],
];

/// What should be heard, in order, playing for `total` seconds from the
/// start (or, backwards, from the end): every event each time its key is
/// passed, worked out from the list of events alone.
fn expected(
    events: &[GlbEvent],
    length: f32,
    looping: bool,
    backwards: bool,
    total: f32,
) -> Vec<(String, f32)> {
    // Each pass as a distance from where playing started, with the key.
    let mut passes: Vec<(f32, usize, usize, String, f32)> = Vec::new();
    let rounds = if looping {
        (total / length).ceil() as usize + 1
    } else {
        1
    };
    for round in 0..rounds {
        for (order, event) in events.iter().enumerate() {
            let along = if backwards {
                // From the end: the last key's events are passed first.
                length - event.at
            } else {
                event.at
            };
            let distance = round as f32 * length + along;
            // Ties (the end of one round and the start of the next) go
            // in the order they are passed: the earlier round's first;
            // within a key, in the order written (backwards, reversed).
            let order = if backwards {
                events.len() - order
            } else {
                order
            };
            passes.push((distance, round, order, event.name.clone(), event.at));
        }
    }
    passes.retain(|(distance, ..)| *distance <= total + 1e-4);
    passes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    passes
        .into_iter()
        .map(|(_, _, _, name, at)| (name, at))
        .collect()
}

/// Plays an animation from its start (or end), update by update, and
/// gives everything heard, in order.
fn heard(
    model: &GlbModel,
    animation: &str,
    looping: bool,
    speed: f32,
    updates: &[f32],
) -> Vec<(String, f32)> {
    let mut playing = AnimatedModel::new(model.clone());
    playing.set_blend(0.0);
    playing.set_speed(speed);
    if looping {
        playing.play_looping(animation);
    } else {
        playing.play_once(animation);
    }
    let mut all = Vec::new();
    for &dt in updates {
        playing.update(dt);
        for event in playing.events() {
            assert_eq!(event.animation, animation);
            all.push((event.name.clone(), event.at));
        }
    }
    all
}

#[test]
fn the_examples_have_the_events_their_recipes_give() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("events-in-the-files");
    let robot = GlbModel::load(models.join("robot.glb"))?;
    let events = |model: &GlbModel, name: &str| -> Vec<(String, f32)> {
        model
            .animations
            .iter()
            .find(|animation| animation.name == name)
            .unwrap()
            .events
            .iter()
            .map(|event| (event.name.clone(), event.at))
            .collect()
    };
    let named = |list: &[(&str, f32)]| -> Vec<(String, f32)> {
        list.iter().map(|(n, at)| (n.to_string(), *at)).collect()
    };
    assert_eq!(
        events(&robot, "walk"),
        named(&[("footstep", 0.0), ("footstep", 0.4)])
    );
    assert_eq!(
        events(&robot, "jump"),
        named(&[("takeoff", 0.3), ("land", 1.0)])
    );
    assert!(events(&robot, "wave").is_empty());
    let door = GlbModel::load(models.join("door.glb"))?;
    assert_eq!(events(&door, "open"), named(&[("opened", 1.0)]));
    let gnome = GlbModel::load(models.join("gnome.glb"))?;
    assert_eq!(events(&gnome, "walk"), events(&robot, "walk"));
    // Duck Mold's report on each animation lists the same.
    for (name, animation) in ANIMATED {
        let report: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(models.join(format!("{name}.{animation}.inspect.json")))
                .unwrap(),
        )
        .unwrap();
        let reported: Vec<(String, f32)> = report["events"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|event| {
                (
                    event["name"].as_str().unwrap().to_owned(),
                    event["at"].as_f64().unwrap() as f32,
                )
            })
            .collect();
        let model = GlbModel::load(models.join(format!("{name}.glb")))?;
        assert_eq!(events(&model, animation), reported, "{name} {animation}");
    }
    Ok(())
}

#[test]
fn every_event_is_heard_exactly_once_each_time_its_key_is_passed() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("events-once");
    let mut checked = 0;
    for (name, animation) in ANIMATED {
        let mut model = GlbModel::load(models.join(format!("{name}.glb")))?;
        let at = model
            .animations
            .iter()
            .position(|a| a.name == animation)
            .unwrap();
        // So that every animation is tested with events: one that has
        // none is given some, on its first key, a middle one and its
        // last.
        if model.animations[at].events.is_empty() {
            let length = model.animations[at].length;
            model.animations[at].events = vec![
                GlbEvent {
                    name: "start".into(),
                    at: 0.0,
                },
                GlbEvent {
                    name: "middle".into(),
                    at: (length * 0.5 * 1000.0).round() / 1000.0,
                },
                GlbEvent {
                    name: "end".into(),
                    at: length,
                },
            ];
        }
        let played = model.animations[at].clone();
        for looping in [false, true] {
            for backwards in [false, true] {
                for pattern in PATTERNS {
                    // A little over three rounds: a loop comes round
                    // three times; once, it stops at its end and stays
                    // there. (Not ending on a key's time, where adding
                    // up the updates' sizes could fall either side.)
                    let total = played.length * 3.37;
                    let updates = steps(total, pattern);
                    let speed = if backwards { -1.0 } else { 1.0 };
                    let found = heard(&model, animation, looping, speed, &updates);
                    let wanted = expected(
                        &played.events,
                        played.length,
                        looping,
                        backwards,
                        if looping { total } else { played.length },
                    );
                    assert_eq!(
                        found,
                        wanted,
                        "{name} {animation}, {} {}, updates {pattern:?}",
                        if looping { "looping" } else { "once" },
                        if backwards { "backwards" } else { "forwards" },
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, ANIMATED.len() * 2 * 2 * PATTERNS.len());
    Ok(())
}

#[test]
fn a_walk_says_footstep_twice_a_round() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("events-walk");
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    robot.play("walk");
    // The first update hears the step the walk starts on.
    robot.update(1.0 / 30.0);
    assert_eq!(robot.events().len(), 1);
    assert_eq!(robot.events()[0].name, "footstep");
    assert_eq!(robot.events()[0].animation, "walk");
    assert_eq!(robot.events()[0].at, 0.0);
    // The next hears nothing: each list is only that update's.
    robot.update(1.0 / 30.0);
    assert!(robot.events().is_empty());
    // Ten seconds at thirty frames a second: 25 steps a ... 12.5 rounds,
    // two steps each, and the first.
    let mut steps = 1;
    for _ in 0..300 {
        robot.update(1.0 / 30.0);
        steps += robot.events().len();
    }
    // (2/30 + 10 s: rounds start at 0, 0.8, ... 9.6, and 0.4, 1.2, ...
    // 10.0 fall within.)
    assert_eq!(steps, 26);
    Ok(())
}

#[test]
fn nothing_is_heard_while_stopped_finished_still_or_posed() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("events-quiet");
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    robot.set_blend(0.0);
    // Stopped: the last update's events are gone, and none come.
    robot.play("walk");
    robot.update(0.01);
    assert_eq!(robot.events().len(), 1);
    robot.stop();
    robot.update(1.0);
    assert!(robot.events().is_empty());
    // Going on, it hears the next step once, not the first again.
    robot.resume();
    robot.update(0.39);
    assert_eq!(robot.events().len(), 0);
    robot.update(0.01);
    assert_eq!(robot.events()[0].at, 0.4);

    // Speed 0: no time passes. Set going again, the start's event is
    // still heard: time hadn't moved.
    robot.set_speed(0.0);
    robot.play("jump");
    robot.update(1.0);
    assert!(robot.events().is_empty());
    robot.set_speed(1.0);
    robot.update(2.0);
    let names: Vec<&str> = robot.events().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["takeoff", "land"]);
    // Finished: it stands on its last pose, and says nothing more.
    assert!(robot.finished());
    robot.update(5.0);
    assert!(robot.events().is_empty());
    // restart plays it again, with its events.
    robot.restart();
    robot.update(0.31);
    assert_eq!(robot.events()[0].name, "takeoff");

    // Holding a pose, or at rest: nothing.
    robot.set_pose("wave");
    robot.update(1.0);
    assert!(robot.events().is_empty());
    robot.rest();
    robot.update(1.0);
    assert!(robot.events().is_empty());

    // A model without events, or without animations, never hears any.
    let mut crate_model = AnimatedModel::load(models.join("crate.glb"))?;
    crate_model.update(1.0);
    assert!(crate_model.events().is_empty());
    Ok(())
}

#[test]
fn changing_animation_stops_the_old_ones_events_at_once() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("events-change");
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    // (With the usual change-over: it only eases the parts.)
    robot.play("walk");
    robot.update(0.3);
    assert_eq!(robot.events().len(), 1);
    robot.play("jump");
    // 0.3 s more would have been the walk's 0.4 step: the jump's
    // takeoff is at 0.3, and that is what is heard.
    robot.update(0.3);
    let heard: Vec<(&str, &str)> = robot
        .events()
        .iter()
        .map(|e| (e.animation.as_str(), e.name.as_str()))
        .collect();
    assert_eq!(heard, [("jump", "takeoff")]);
    // Asking for what is already playing changes nothing: the takeoff
    // isn't heard again.
    robot.play("jump");
    robot.update(0.1);
    assert!(robot.events().is_empty());
    Ok(())
}

#[test]
fn a_door_closing_hears_opened_first() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("events-door");
    let mut door = AnimatedModel::load(models.join("door.glb"))?;
    door.set_speed(-1.0);
    door.play("open");
    door.update(0.05);
    assert_eq!(door.events().len(), 1);
    assert_eq!(door.events()[0].name, "opened");
    door.update(2.0);
    assert!(door.events().is_empty());
    assert!(door.finished());
    Ok(())
}

#[test]
fn an_event_on_both_ends_of_a_loop_is_heard_twice_where_it_joins() -> Result<(), LoadError> {
    // What duck-mold inspect warns of: the end and the start of a loop
    // are one moment.
    let (_folder, models) = fixtures("events-join");
    let mut model = GlbModel::load(models.join("robot.glb"))?;
    let walk = model
        .animations
        .iter_mut()
        .find(|a| a.name == "walk")
        .unwrap();
    walk.events.push(GlbEvent {
        name: "footstep".into(),
        at: walk.length,
    });
    let mut robot = AnimatedModel::new(model);
    robot.play("walk");
    robot.update(0.01);
    robot.update(0.785);
    assert!(robot.events().is_empty() || robot.events()[0].at == 0.4);
    robot.update(0.01);
    let ats: Vec<f32> = robot.events().iter().map(|e| e.at).collect();
    assert_eq!(ats, [0.8, 0.0], "the last key's, then the first's");
    Ok(())
}

#[test]
fn a_long_update_hears_at_most_ten_rounds() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("events-long");
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    robot.play("walk");
    robot.update(0.01);
    // A minute away: 75 rounds, of which the last ten are heard.
    robot.update(60.0);
    let count = robot.events().len();
    assert!((20..=21).contains(&count), "{count}");
    // In order: the two steps taking turns.
    let ats: Vec<f32> = robot.events().iter().map(|e| e.at).collect();
    assert!(ats.windows(2).all(|two| two[0] != two[1]), "{ats:?}");
    Ok(())
}

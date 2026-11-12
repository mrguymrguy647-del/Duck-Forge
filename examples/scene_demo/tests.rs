//! Play tests for the scene demo: `cargo test --example scene_demo`.
//!
//! The scene is a file people edit, so these test what must stay true of
//! it (it reads without mistakes, the bot can still win on it as it is
//! now), and test how the game plays on small scenes written right here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// A copy of the game's folder (its pictures and its legend) in a
/// temporary folder of the test's own, with `scene` as its scene file's
/// text and a flat level with nothing painted on it but the ground. So
/// painting the real level, or editing the real scene, can't change what
/// these tests see. Deleted when the test is done.
struct Copy(PathBuf);

impl Copy {
    fn of_the_game(scene: &str) -> Copy {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let folder = std::env::temp_dir().join(format!(
            "duckforge-scene-demo-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        let own = PathBuf::from("examples/scene_demo");
        std::fs::create_dir_all(folder.join("assets")).unwrap();
        std::fs::create_dir_all(folder.join("scenes")).unwrap();
        for entry in std::fs::read_dir(own.join("assets")).unwrap().flatten() {
            std::fs::copy(entry.path(), folder.join("assets").join(entry.file_name())).unwrap();
        }
        std::fs::copy(
            own.join("level.legend.json"),
            folder.join("level.legend.json"),
        )
        .unwrap();
        let (sky, grass, dirt) = (".".repeat(40), "#".repeat(40), "d".repeat(40));
        let level = format!("{}{grass}\n{dirt}\n", format!("{sky}\n").repeat(13));
        std::fs::write(folder.join("level.txt"), level).unwrap();
        let copy = Copy(folder);
        std::fs::write(copy.scene(), scene).unwrap();
        copy
    }

    fn scene(&self) -> PathBuf {
        self.0.join("scenes/pond.json")
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A small scene: the ground, a duck, and what a test adds.
fn scene_with(objects: &str) -> String {
    format!(
        r#"{{"duckforge_scene": 1, "objects": [
            {{"id": 1, "name": "ground", "kind": "tilemap", "level": "level.txt"}},
            {{"id": 2, "name": "duck", "at": [40, 208], "picture": "assets/hero.json", "box": [3, 2, 10, 14]}}
            {objects}]}}"#
    )
}

#[test]
fn the_scene_file_reads_without_mistakes_and_is_written_the_duck_forge_way() {
    let text = std::fs::read_to_string(SCENE)
        .unwrap()
        .replace("\r\n", "\n");
    let scene = duckforge::scene::SceneFile::parse(&text).unwrap();
    assert_eq!(scene.problems, [], "what the file says is wrong");
    assert_eq!(scene.ids_missing, 0, "every object has its id in the file");
    assert_eq!(
        scene.to_text(),
        text,
        "one object per line, as Duck Forge writes it"
    );
    let game = SceneDemo::new().unwrap();
    assert_eq!(
        game.scene.problems(),
        [] as [&str; 0],
        "what couldn't be loaded"
    );
    assert!(game.scene.find("duck").is_some());
    assert!(game.scene.find("flag").is_some(), "painted on the tile map");
}

#[test]
fn the_bot_wins_on_the_scene_as_it_is_now() {
    let played = bot::play(SceneDemo::new().unwrap()).unwrap_or_else(|why| panic!("{why}"));
    assert!(played.game().won);
    assert_eq!(played.game().coins, 6, "every coin, painted or placed");
    assert!(played.failed_checks().is_empty());
}

#[test]
fn the_duck_lands_runs_and_takes_a_coin_placed_by_hand() {
    let copy = Copy::of_the_game(&scene_with(
        r#", {"id": 3, "name": "coin", "at": [70, 208], "picture": "assets/tiles.json", "animation": "coin"}"#,
    ));
    let played = PlayTest::new(config(), SceneDemo::with_scene(copy.scene()).unwrap())
        .script(
            r#"{"events": [{"frame": 1, "press": "Right"}, {"frame": 40, "release": "Right"}]}"#,
        )
        .frames(60)
        .run();
    assert!(played.flag("on_ground"));
    assert!(played.number("x") > 90.0, "ran right for 40 frames");
    assert_eq!(played.number("coins"), 1.0);
    played.assert_sound("coin", 12, 12);
    // The duck faces the way it ran, and stands still again.
    let scene = &played.game().scene;
    let duck = scene.find("duck").unwrap();
    assert!(!scene[duck].flip);
    assert_eq!(scene[duck].animation.as_deref(), Some("idle"));
}

#[test]
fn a_solid_object_stops_the_duck_and_can_be_stood_on() {
    let copy = Copy::of_the_game(&scene_with(
        r#", {"id": 3, "name": "crate", "at": [100, 208], "picture": "assets/crate.png", "solid": true}"#,
    ));
    // Running into it: stopped at its side.
    let played = PlayTest::new(config(), SceneDemo::with_scene(copy.scene()).unwrap())
        .script(r#"{"events": [{"frame": 1, "press": "Right"}]}"#)
        .frames(90)
        .run();
    assert_eq!(
        played.number("x"),
        87.0,
        "its 10 pixels end where the crate starts, at 92"
    );
    // Jumping at it: up and over.
    let played = PlayTest::new(config(), SceneDemo::with_scene(copy.scene()).unwrap())
        .script(r#"{"events": [{"frame": 1, "press": "Right"}, {"frame": 25, "press": "Space"}]}"#)
        .frames(90)
        .run();
    assert!(played.number("x") > 110.0, "past the crate");
    // Dropped onto it: it's something to stand on.
    let above = scene_with(
        r#", {"id": 3, "name": "crate", "at": [100, 208], "picture": "assets/crate.png", "solid": true}"#,
    )
    .replace("\"at\": [40, 208]", "\"at\": [100, 150]");
    let copy = Copy::of_the_game(&above);
    let played = PlayTest::new(config(), SceneDemo::with_scene(copy.scene()).unwrap())
        .frames(60)
        .run();
    let scene = &played.game().scene;
    let duck = scene.find("duck").unwrap();
    assert!(played.flag("on_ground"));
    assert_eq!(
        scene[duck].at.y, 192.0,
        "standing on the crate, 16 pixels up"
    );
}

#[test]
fn a_change_to_the_scene_file_is_taken_in_mid_play() {
    let copy = Copy::of_the_game(&scene_with(
        r#", {"id": 3, "name": "sign", "at": [150, 208], "picture": "assets/sign.png"}"#,
    ));
    let mut sim = Simulation::new(config(), SceneDemo::with_scene(copy.scene()).unwrap());
    sim.press(Key::Right);
    sim.run(30);
    let duck = sim.game().scene.find("duck").unwrap();
    let walked_to = sim.game().scene[duck].at;
    assert!(walked_to.x > 60.0);

    // The file is saved with the duck starting somewhere else, the sign
    // moved and turned, and a coin just ahead of where the duck is now.
    std::fs::write(
        copy.scene(),
        scene_with(
            r#", {"id": 3, "name": "sign", "at": [200, 208], "turn": 20, "picture": "assets/sign.png"},
                 {"id": 4, "name": "coin", "at": [110, 208], "picture": "assets/tiles.json", "animation": "coin"}"#,
        )
        .replace("\"at\": [40, 208]", "\"at\": [300, 208]"),
    )
    .unwrap();
    let took_it = sim.with_context(|game, ctx| game.reload(ctx, &copy.scene()));
    assert!(took_it);
    let scene = &sim.game().scene;
    assert_eq!(
        scene[duck].at, walked_to,
        "the duck stays where it walked to"
    );
    assert_eq!(
        scene.starts_as(duck).unwrap().at.x,
        300.0,
        "and starts there next time"
    );
    let sign = scene.find("sign").unwrap();
    assert_eq!((scene[sign].at.x, scene[sign].turn), (200.0, 20.0));
    // The game plays on from the same frame, and takes the new coin.
    sim.run(40);
    assert_eq!(sim.game().coins, 1);
    assert_eq!(sim.context().frame(), 70);
}

#[test]
fn a_scene_with_no_duck_or_a_mistake_doesnt_stop_the_game() {
    let copy = Copy::of_the_game(
        r#"{"duckforge_scene": 1, "objects": [
            {"id": 1, "name": "ground", "kind": "tilemap", "level": "level.txt"},
            {"id": 2, "name": "ghost", "at": [100, 208], "picture": "assets/nobody.png"}]}"#,
    );
    let played = PlayTest::new(config(), SceneDemo::with_scene(copy.scene()).unwrap())
        .ignore_checks()
        .frames(30)
        .run();
    assert_eq!(played.failed_checks().len(), 1);
    assert_eq!(
        played.failed_checks()[0].name,
        "the scene file has no mistakes"
    );
    let problems = played.game().scene.problems();
    assert!(
        problems[0].contains("\"ghost\" has the picture \"assets/nobody.png\""),
        "{problems:?}"
    );
}

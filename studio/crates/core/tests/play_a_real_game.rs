//! The whole path, with a real game and no window: the Studio builds
//! Dodge, starts it, plays it through the link, freezes it, carries on,
//! and stops it. Then the saved session is replayed headless, and must end
//! on exactly the same picture.
//!
//! It needs cargo and takes a minute or two the first time (it builds the
//! game). The release workflow runs it on Windows too, which proves the
//! link there before anyone downloads the Studio.

use std::path::{Path, PathBuf};
use std::time::Instant;

use studio_core::play::PlayState;
use studio_core::script::{Step, Wait, frame, run_script, run_step};
use studio_core::settings::Settings;
use studio_core::test_folder::TestFolder;
use studio_core::{Command, Studio};

fn duck_forge() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

fn studio() -> Studio {
    // Default settings are never saved: the person's own are never touched.
    let studio = Studio::new(Settings {
        games_folder: Some(duck_forge()),
        ..Settings::default()
    });
    assert!(studio.folder.is_some(), "{:?}", studio.folder_problem);
    studio
}

/// The pixels of a PNG file, each `0x00RRGGBB`, with its size.
fn png_pixels(path: &Path) -> (u32, u32, Vec<u32>) {
    let mut decoder =
        png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    let each = info.color_type.samples();
    let pixels = bytes[..info.buffer_size()]
        .chunks_exact(each)
        .map(|p| u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
        .collect();
    (info.width, info.height, pixels)
}

fn press(key: &str) -> Step {
    Step::Do(serde_json::from_value(serde_json::json!({"do": "game.input", "press": key})).unwrap())
}

fn release(key: &str) -> Step {
    Step::Do(
        serde_json::from_value(serde_json::json!({"do": "game.input", "release": key})).unwrap(),
    )
}

#[test]
fn a_game_is_built_played_frozen_resumed_and_its_session_replays_exactly() {
    let mut studio = studio();
    let say = |studio: &mut Studio, step: Step| {
        run_step(studio, &step).unwrap_or_else(|problem| {
            panic!("{problem}\n{}", problem.details);
        })
    };
    say(
        &mut studio,
        Step::Do(Command::GameOpen {
            name: String::from("dodge"),
        }),
    );
    say(&mut studio, Step::Wait(Wait::Playing));
    let hello = studio.play().unwrap().hello.clone().unwrap();
    assert_eq!((hello.width, hello.height), (320, 240));

    // Play for a second and a half, moving about.
    say(&mut studio, press("Space"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 10 }));
    say(&mut studio, release("Space"));
    say(&mut studio, press("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 60 }));

    // Tab: frozen on that frame. Nothing moves, however long it stays.
    say(&mut studio, Step::Do(Command::GameToggle));
    assert_eq!(studio.play().unwrap().state, PlayState::Frozen);
    let frozen = studio.play().unwrap().picture.clone().unwrap();
    say(&mut studio, Step::Wait(Wait::Seconds { seconds: 0.5 }));
    let still = studio.play().unwrap().picture.clone().unwrap();
    assert_eq!(
        still.frame, frozen.frame,
        "no frame was played while frozen"
    );
    assert_eq!(still.pixels, frozen.pixels);
    assert!(
        studio.open.as_ref().unwrap().last_session.is_some(),
        "freezing saves the session"
    );

    // F1 while frozen: the game draws its debug overlay on the same
    // frame, and takes it away again. The game itself doesn't move.
    say(
        &mut studio,
        Step::Do(Command::GameDebugOverlay { on: Some(true) }),
    );
    say(&mut studio, Step::Wait(Wait::Seconds { seconds: 0.2 }));
    let overlaid = studio.play().unwrap().picture.clone().unwrap();
    assert_eq!(overlaid.frame, frozen.frame);
    assert_ne!(overlaid.pixels, frozen.pixels, "the overlay is drawn");
    say(
        &mut studio,
        Step::Do(Command::GameDebugOverlay { on: Some(false) }),
    );
    say(&mut studio, Step::Wait(Wait::Seconds { seconds: 0.2 }));
    assert_eq!(
        studio.play().unwrap().picture.as_ref().unwrap().pixels,
        frozen.pixels
    );

    // F12: the picture is saved at the game's own size.
    let said = studio.run(Command::GameScreenshot).unwrap();
    let name = said.rsplit('/').next().unwrap();
    let saved = studio
        .folder
        .as_ref()
        .unwrap()
        .screenshots_folder()
        .join(name);
    let (width, height, pixels) = png_pixels(&saved);
    assert_eq!((width, height), (frozen.width, frozen.height));
    assert_eq!(pixels, frozen.pixels, "{said}");
    std::fs::remove_file(saved).unwrap();

    // Frozen, one frame at a time.
    say(&mut studio, Step::Do(Command::GameStep { frames: 1 }));
    say(&mut studio, Step::Wait(Wait::Seconds { seconds: 0.1 }));
    assert_eq!(studio.play().unwrap().frame, frozen.frame + 1);
    assert_eq!(studio.play().unwrap().state, PlayState::Frozen);

    // Tab again: it carries on from there. (Right isn't held any more as
    // far as the Studio knows, so it's let go: nothing sticks.)
    say(&mut studio, Step::Do(Command::GameToggle));
    say(&mut studio, press("Left"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 45 }));
    say(&mut studio, release("Left"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 5 }));

    // The numbers: a steady 60 updates a second, with pictures on time.
    let timing = studio.timing(Instant::now());
    println!("Timing through the real link: {timing:?}");
    assert!(timing.ticks > 30, "{timing:?}");

    // Stop. The session is saved as an input script.
    say(&mut studio, Step::Do(Command::GameFreeze));
    let last = studio.play().unwrap().picture.clone().unwrap();
    say(&mut studio, Step::Do(Command::GameStop));
    assert!(!studio.play().unwrap().is_running());
    let session = studio.open.as_ref().unwrap().last_session.clone().unwrap();
    let script = duckforge::InputScript::load(&session).unwrap();
    assert_eq!(script.frames, Some(last.frame));
    assert_eq!(script.seed, Some(hello.seed));
    let pressed = |key| {
        script
            .events
            .iter()
            .filter(|event| event.action == duckforge::InputAction::Press(key))
            .count()
    };
    assert_eq!(pressed(duckforge::prelude::Key::Space), 1);
    assert_eq!(pressed(duckforge::prelude::Key::Right), 1);
    assert_eq!(pressed(duckforge::prelude::Key::Left), 1);

    // Replay it with no window: the same game, to the last pixel.
    let replays = TestFolder::new("replay-dodge");
    let out = replays.join("out");
    let replay = std::process::Command::new("cargo")
        .args([
            "run",
            "-q",
            "--no-default-features",
            "--example",
            "dodge",
            "--",
        ])
        .args(["--headless", "--inputs"])
        .arg(&session)
        .arg("--out")
        .arg(&out)
        .current_dir(duck_forge())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&replay.stdout);
    assert!(
        replay.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert!(
        !said.contains("playthrough changed"),
        "the replay ended as the session did: {said}"
    );
    let (width, height, pixels) = png_pixels(&out.join("last.png"));
    assert_eq!((width, height), (last.width, last.height));
    assert!(
        pixels == last.pixels,
        "the replay's last picture is the session's, pixel for pixel"
    );
}

#[test]
fn a_script_can_drive_the_studio_and_a_missing_game_is_a_plain_problem() {
    let mut studio = studio();
    let problem = run_script(&mut studio, r#"[{"do": "game.open", "name": "nope"}]"#).unwrap_err();
    assert!(
        problem
            .what
            .starts_with("Step 1 of the script: There's no game called nope")
    );
    // Frames still run with nothing open.
    frame(&mut studio, Instant::now());
    assert!(studio.play().is_none());
}

/// Puts a file back as it was when this is dropped, even if the test fails.
struct PutBack {
    path: PathBuf,
    text: String,
}

impl Drop for PutBack {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.path, &self.text);
    }
}

#[test]
fn a_level_painted_while_frozen_is_in_the_game_and_in_its_replay() {
    let mut studio = studio();
    let say = |studio: &mut Studio, step: Step| {
        run_step(studio, &step).unwrap_or_else(|problem| {
            panic!("{problem}\n{}", problem.details);
        })
    };
    let level_file = duck_forge().join("examples/template/level.txt");
    let _put_back = PutBack {
        text: std::fs::read_to_string(&level_file).unwrap(),
        path: level_file.clone(),
    };
    say(
        &mut studio,
        Step::Do(Command::GameOpen {
            name: String::from("template"),
        }),
    );
    say(&mut studio, Step::Wait(Wait::Playing));
    say(&mut studio, press("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 20 }));
    say(&mut studio, release("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 10 }));
    say(&mut studio, Step::Do(Command::GameToggle));
    let frozen = studio.play().unwrap().picture.clone().unwrap();
    let maps = studio.play().unwrap().maps.clone();
    assert_eq!(maps.len(), 1, "the picture says where the map is");
    assert_eq!(maps[0].file, "examples/template/level.txt");

    // Paint a wall of bricks in front of the duck, in one stroke: the
    // frozen game takes it, and draws the same frame again with it.
    let cells: Vec<(i32, i32)> = (9..12).map(|row| (12, row)).collect();
    say(
        &mut studio,
        Step::Do(Command::LevelPaint {
            cells,
            tile: String::from("brick"),
            stroke: Some(1),
        }),
    );
    say(&mut studio, Step::Wait(Wait::Seconds { seconds: 0.5 }));
    let painted = studio.play().unwrap().picture.clone().unwrap();
    assert_eq!(
        painted.frame, frozen.frame,
        "still frozen on the same frame"
    );
    assert_ne!(painted.pixels, frozen.pixels, "the bricks are drawn");
    assert!(
        !studio.open.as_ref().unwrap().level_needs_restart,
        "the game took it"
    );
    let on_disk = std::fs::read_to_string(&level_file).unwrap();
    assert_eq!(&on_disk[12..13], ".", "row 0 is untouched");
    assert_eq!(
        on_disk.lines().nth(9).unwrap().chars().nth(12),
        Some('B'),
        "saved at once"
    );

    // A higher jump (Feel), while frozen too.
    say(
        &mut studio,
        Step::Do(Command::FeelSet {
            name: String::from("jump_speed"),
            value: 420.0,
            stroke: None,
        }),
    );

    // Carry on: the duck runs into the new wall, and jumps.
    say(&mut studio, Step::Do(Command::GameToggle));
    say(&mut studio, press("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 60 }));
    say(&mut studio, press("Space"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 2 }));
    say(&mut studio, release("Space"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 30 }));
    say(&mut studio, release("Right"));
    say(&mut studio, Step::Do(Command::GameFreeze));
    let last = studio.play().unwrap().picture.clone().unwrap();
    say(&mut studio, Step::Do(Command::GameStop));
    let session = studio.open.as_ref().unwrap().last_session.clone().unwrap();
    let script = duckforge::InputScript::load(&session).unwrap();
    assert_eq!(
        script.changes.len(),
        2,
        "the bricks and the jump, on the frame after the freeze"
    );
    assert!(
        script
            .changes
            .iter()
            .all(|change| change.frame == frozen.frame + 1)
    );

    // Undo, twice (the jump, then the bricks), puts the files back.
    say(&mut studio, Step::Do(Command::EditUndo));
    assert!(
        !duck_forge().join("examples/template.tune.json").exists(),
        "there was no tune file before"
    );
    say(&mut studio, Step::Do(Command::EditUndo));
    assert_eq!(
        std::fs::read_to_string(&level_file).unwrap(),
        _put_back.text,
        "undone"
    );

    // Replayed with no window, with the old level on disk: the session
    // has the level as it was, and the change on its frame, so it ends on
    // the same picture, pixel for pixel.
    let replays = TestFolder::new("replay-level");
    let out = replays.join("out");
    let replay = std::process::Command::new("cargo")
        .args([
            "run",
            "-q",
            "--no-default-features",
            "--example",
            "template",
            "--",
        ])
        .args(["--headless", "--inputs"])
        .arg(&session)
        .arg("--out")
        .arg(&out)
        .current_dir(duck_forge())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&replay.stdout);
    assert!(
        replay.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert!(!said.contains("playthrough changed"), "{said}");
    let (_, _, pixels) = png_pixels(&out.join("last.png"));
    assert!(
        pixels == last.pixels,
        "the replay ends on the session's last picture"
    );
}

/// A scene file changed outside the Studio (in an editor, or by an agent)
/// while the game is frozen: the game takes it at once and draws the same
/// frame again, the duck stays where it walked to, it's a step for Undo,
/// and the session replays to the same picture.
#[test]
fn a_scene_file_edited_while_frozen_is_in_the_game_and_in_its_replay() {
    let mut studio = studio();
    let say = |studio: &mut Studio, step: Step| {
        run_step(studio, &step).unwrap_or_else(|problem| {
            panic!("{problem}\n{}", problem.details);
        })
    };
    let scene_file = duck_forge().join("examples/scene_demo/scenes/pond.json");
    let _put_back = PutBack {
        text: std::fs::read_to_string(&scene_file).unwrap(),
        path: scene_file.clone(),
    };
    say(
        &mut studio,
        Step::Do(Command::GameOpen {
            name: String::from("scene_demo"),
        }),
    );
    say(&mut studio, Step::Wait(Wait::Playing));
    say(&mut studio, press("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 20 }));
    say(&mut studio, release("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 10 }));
    say(&mut studio, Step::Do(Command::GameToggle));
    let frozen = studio.play().unwrap().picture.clone().unwrap();

    // Saved from outside: the sign leans the other way, the duck's line
    // says it starts further on, and there's a crate in the duck's way.
    let edited = _put_back
        .text
        .replace("\"turn\": -8", "\"turn\": 30")
        .replace(
            "{\"id\": 9, \"name\": \"duck\", \"at\": [40, 208]",
            "{\"id\": 12, \"name\": \"new crate\", \"at\": [100, 208], \"picture\": \"assets/crate.png\", \"solid\": true},\n    {\"id\": 9, \"name\": \"duck\", \"at\": [200, 208]",
        );
    assert_ne!(edited, _put_back.text);
    assert!(edited.contains("new crate") && edited.contains("\"turn\": 30"));
    std::fs::write(&scene_file, &edited).unwrap();
    say(&mut studio, Step::Wait(Wait::Seconds { seconds: 1.2 }));
    let changed = studio.play().unwrap().picture.clone().unwrap();
    assert_eq!(
        changed.frame, frozen.frame,
        "still frozen on the same frame"
    );
    assert_ne!(changed.pixels, frozen.pixels, "the sign and the crate show");
    assert!(
        !studio.open.as_ref().unwrap().level_needs_restart,
        "the game took it"
    );

    // Carry on: the duck is where it was (not where its line now says it
    // starts), runs into the new crate, and jumps onto it.
    say(&mut studio, Step::Do(Command::GameToggle));
    say(&mut studio, press("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 40 }));
    say(&mut studio, press("Space"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 2 }));
    say(&mut studio, release("Space"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 30 }));
    say(&mut studio, release("Right"));
    say(&mut studio, Step::Do(Command::GameFreeze));
    let last = studio.play().unwrap().picture.clone().unwrap();
    say(&mut studio, Step::Do(Command::GameStop));
    let session = studio.open.as_ref().unwrap().last_session.clone().unwrap();
    let script = duckforge::InputScript::load(&session).unwrap();
    assert_eq!(script.changes.len(), 1, "the scene file, once");
    assert_eq!(script.changes[0].frame, frozen.frame + 1);

    // Undo puts the file back as it was.
    say(&mut studio, Step::Do(Command::EditUndo));
    assert_eq!(
        std::fs::read_to_string(&scene_file).unwrap(),
        _put_back.text,
        "undone"
    );

    // Replayed with no window, with the old scene on disk: the session
    // has the scene as it was and the change on its frame, so it ends on
    // the same picture, pixel for pixel.
    let replays = TestFolder::new("replay-scene");
    let out = replays.join("out");
    let replay = std::process::Command::new("cargo")
        .args([
            "run",
            "-q",
            "--no-default-features",
            "--example",
            "scene_demo",
            "--",
        ])
        .args(["--headless", "--inputs"])
        .arg(&session)
        .arg("--out")
        .arg(&out)
        .current_dir(duck_forge())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&replay.stdout);
    assert!(
        replay.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert!(!said.contains("playthrough changed"), "{said}");
    let (_, _, pixels) = png_pixels(&out.join("last.png"));
    assert!(
        pixels == last.pixels,
        "the replay ends on the session's last picture"
    );
}

/// A game whose card has no picture gets a real one the first time it
/// plays in the Studio (from two seconds in), or the first time it's
/// checked (a check of a game with no playthrough draws nothing, so the
/// Studio then runs it for two seconds with no window).
#[test]
fn a_game_with_no_picture_gets_one_the_first_time_it_plays_or_is_checked() {
    let mut studio = studio();
    let folder = studio.folder.clone().unwrap();
    for game in ["hello", "empty"] {
        let _ = std::fs::remove_file(folder.thumbnail_file(game));
        if folder.card_picture(game).is_some() {
            eprintln!("skipped {game}: its card already has a picture from an earlier check");
            continue;
        }
        let script = if game == "hello" {
            r#"[{"do": "game.open", "name": "hello"}, {"wait": "playing"},
                {"wait": "frames", "frames": 130}, {"do": "game.stop"}]"#
        } else {
            r#"[{"do": "game.open", "name": "empty"}, {"wait": "playing"},
                {"do": "game.stop"}, {"do": "check.run"}, {"wait": "idle"}]"#
        };
        run_script(&mut studio, script).unwrap_or_else(|problem| panic!("{game}: {problem}"));
        let picture = folder.thumbnail_file(game);
        assert_eq!(folder.card_picture(game).as_ref(), Some(&picture), "{game}");
        let (width, height, pixels) = png_pixels(&picture);
        assert!(
            width >= 160 && height >= 120,
            "{game}: the game's own size, {width} x {height}"
        );
        assert!(
            pixels.iter().any(|&p| p != 0),
            "{game}: not a black picture"
        );
        let card = studio
            .cards
            .iter()
            .find(|card| card.game.name == game)
            .unwrap();
        assert_eq!(
            card.picture.as_ref(),
            Some(&picture),
            "{game}: Home shows it"
        );
        let _ = std::fs::remove_file(picture);
        let _ = std::fs::remove_dir_all(folder.check_folder(game));
        let _ = std::fs::remove_dir_all(folder.sessions_folder(game));
    }
}

/// The editor on a frozen game. The sign's start is moved (the file
/// changes, the game shows it at once). The duck, which has walked, is
/// moved for this run only (the file stays, the picture shows it in its
/// new place and says where it starts). Undo takes each back. Then the
/// game plays on, and its session replays to the same picture.
#[test]
fn objects_are_selected_and_moved_in_the_frozen_game_and_it_all_replays() {
    use duckforge::prelude::Vec2;
    use studio_core::scene_edit::{RowState, Show};

    let mut studio = studio();
    let say = |studio: &mut Studio, step: Step| {
        run_step(studio, &step).unwrap_or_else(|problem| {
            panic!("{problem}\n{}", problem.details);
        })
    };
    let run = |studio: &mut Studio, command: serde_json::Value| {
        let command: Command = serde_json::from_value(command).unwrap();
        run_step(studio, &Step::Do(command)).unwrap_or_else(|problem| {
            panic!("{problem}\n{}", problem.details);
        })
    };
    let settle = |studio: &mut Studio| {
        run_step(studio, &Step::Wait(Wait::Seconds { seconds: 0.3 })).unwrap();
    };
    let scene_file = duck_forge().join("examples/scene_demo/scenes/pond.json");
    let _put_back = PutBack {
        text: std::fs::read_to_string(&scene_file).unwrap(),
        path: scene_file.clone(),
    };
    say(
        &mut studio,
        Step::Do(Command::GameOpen {
            name: String::from("scene_demo"),
        }),
    );
    say(&mut studio, Step::Wait(Wait::Playing));
    say(&mut studio, press("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 20 }));
    say(&mut studio, release("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 10 }));
    say(&mut studio, Step::Do(Command::GameToggle));
    settle(&mut studio);
    let frozen = studio.play().unwrap().picture.clone().unwrap();

    // The frozen picture says where every object is.
    let open = studio.open.as_ref().unwrap();
    let doc = open.scene.as_ref().expect("the scene the game is in");
    assert_eq!(doc.short_name(), "pond.json");
    let rows = open.scene_rows();
    let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "sky",
            "hills",
            "ground",
            "sign",
            "crate",
            "big crate",
            "coin",
            "coin",
            "duck",
            "bush",
            "coin icon"
        ]
    );
    assert_eq!(
        rows[2].painted.as_deref(),
        Some("4 coins, 1 flag"),
        "what's painted on the tile map"
    );
    assert!(rows[8].moved, "the duck has walked since the start");
    assert!(rows.iter().all(|row| row.state == RowState::There));
    let drawn = open.drawn_scene().expect("a frozen picture has the scene");
    let sign_was = drawn.object(4).unwrap().rect.unwrap();
    let duck = drawn.object(9).unwrap();
    let duck_was = duck.rect.unwrap();
    let duck_started = duck.start_rect.expect("it shows where the duck starts");
    assert!(duck_was.x > duck_started.x + 10.0);
    assert!(
        drawn.object(4).unwrap().start_rect.is_none(),
        "the sign hasn't moved"
    );

    // A click on the sign picks it; its start moves 40 pixels right.
    let middle = Vec2::new(sign_was.x + sign_was.w / 2.0, sign_was.y + sign_was.h / 2.0);
    assert_eq!(
        studio_core::scene_edit::object_at(drawn, middle, None),
        Some(4)
    );
    assert_eq!(
        run(
            &mut studio,
            serde_json::json!({"do": "select.set", "ids": [4]})
        ),
        "Selected sign"
    );
    assert_eq!(
        run(
            &mut studio,
            serde_json::json!({"do": "object.move", "ids": [4], "by": [40, 0]})
        ),
        "Move sign"
    );
    settle(&mut studio);
    let text = std::fs::read_to_string(&scene_file).unwrap();
    assert!(
        text.contains(r#"{"id": 4, "name": "sign", "at": [160, 208]"#),
        "{text}"
    );
    assert_eq!(
        text.replace("[160, 208]", "[120, 208]"),
        _put_back.text.replace("\r\n", "\n"),
        "nothing else in the file changed"
    );
    let open = studio.open.as_ref().unwrap();
    let moved = open.drawn_scene().unwrap().object(4).unwrap().rect.unwrap();
    assert_eq!(
        moved.x,
        sign_was.x + 40.0 * open.drawn_scene().unwrap().scale
    );
    let after_sign = studio.play().unwrap().picture.clone().unwrap();
    assert_eq!(after_sign.frame, frozen.frame, "still the same frame");
    assert_ne!(
        after_sign.pixels, frozen.pixels,
        "the sign shows in its new place"
    );
    assert!(!open.level_needs_restart);

    // The duck, for this run only: the file is left alone.
    assert_eq!(
        run(
            &mut studio,
            serde_json::json!({"do": "object.move_now", "id": 9, "to": [200, 208]})
        ),
        "Move duck for this run"
    );
    settle(&mut studio);
    assert_eq!(std::fs::read_to_string(&scene_file).unwrap(), text);
    let open = studio.open.as_ref().unwrap();
    let duck = open.drawn_scene().unwrap().object(9).unwrap().clone();
    assert_eq!(duck.now["at"], serde_json::json!([200, 208]));
    assert_eq!(
        duck.start_rect,
        Some(duck_started),
        "it still starts where it did"
    );
    assert!(duck.rect.unwrap().x > duck_was.x);

    // "As it starts" draws the scene as the file has it; the game is
    // untouched.
    run(
        &mut studio,
        serde_json::json!({"do": "view.show", "what": "start"}),
    );
    settle(&mut studio);
    let open = studio.open.as_ref().unwrap();
    assert_eq!(open.view.show, Show::Start);
    let starts = open.drawn_scene().unwrap();
    assert!(starts.as_it_starts);
    assert_eq!(
        starts.object(9).unwrap().rect.map(|r| r.w),
        Some(duck_started.w)
    );
    assert!(starts.object(9).unwrap().now.is_empty());
    let problem = studio
        .run(
            serde_json::from_value(
                serde_json::json!({"do": "object.move_now", "id": 9, "to": [8, 8]}),
            )
            .unwrap(),
        )
        .unwrap_err();
    assert!(problem.what.contains("as it starts"), "{problem:?}");
    studio.problem = None;
    run(
        &mut studio,
        serde_json::json!({"do": "view.show", "what": "frozen"}),
    );
    settle(&mut studio);

    // Undo: the duck goes back, then the sign.
    assert_eq!(
        say(&mut studio, Step::Do(Command::EditUndo)),
        "Undid: Move duck for this run"
    );
    settle(&mut studio);
    let open = studio.open.as_ref().unwrap();
    assert_eq!(
        open.drawn_scene().unwrap().object(9).unwrap().rect,
        Some(duck_was)
    );
    assert_eq!(
        say(&mut studio, Step::Do(Command::EditUndo)),
        "Undid: Move sign"
    );
    settle(&mut studio);
    let back = studio.play().unwrap().picture.clone().unwrap();
    assert!(
        back.pixels == frozen.pixels,
        "everything is as it was frozen"
    );
    assert_eq!(
        std::fs::read_to_string(&scene_file)
            .unwrap()
            .replace("\r\n", "\n"),
        _put_back.text.replace("\r\n", "\n")
    );

    // Redo both, make the duck's new place its start, and play on.
    say(&mut studio, Step::Do(Command::EditRedo));
    say(&mut studio, Step::Do(Command::EditRedo));
    settle(&mut studio);
    assert_eq!(
        run(
            &mut studio,
            serde_json::json!({"do": "object.start_here", "ids": [9]})
        ),
        "Start duck here"
    );
    settle(&mut studio);
    let text = std::fs::read_to_string(&scene_file).unwrap();
    assert!(
        text.contains(r#"{"id": 9, "name": "duck", "at": [200, 208]"#),
        "{text}"
    );
    let open = studio.open.as_ref().unwrap();
    let duck = open.drawn_scene().unwrap().object(9).unwrap();
    assert!(
        duck.start_rect.is_none() && !duck.now.contains_key("at"),
        "it is where it starts: {:?}",
        duck.now
    );

    // Undo of "Start here" leaves the duck where it is and brings its old
    // start back; Redo makes it its start again.
    say(&mut studio, Step::Do(Command::EditUndo));
    settle(&mut studio);
    let open = studio.open.as_ref().unwrap();
    let duck = open.drawn_scene().unwrap().object(9).unwrap();
    assert_eq!(duck.now["at"], serde_json::json!([200, 208]));
    assert_eq!(duck.start_rect, Some(duck_started));
    say(&mut studio, Step::Do(Command::EditRedo));
    settle(&mut studio);
    let open = studio.open.as_ref().unwrap();
    let duck = open.drawn_scene().unwrap().object(9).unwrap();
    assert!(duck.start_rect.is_none() && !duck.now.contains_key("at"));

    // The duck is put on a coin and the game plays three frames: the coin
    // is taken. It's still in the list (in italics, in the app), and
    // where it starts can still be changed.
    run(
        &mut studio,
        serde_json::json!({"do": "object.move_now", "id": 9, "to": [248, 180]}),
    );
    settle(&mut studio);
    say(&mut studio, Step::Do(Command::GameStep { frames: 3 }));
    settle(&mut studio);
    let open = studio.open.as_ref().unwrap();
    let rows = open.scene_rows();
    assert_eq!(rows[6].id, 7);
    assert_eq!(rows[6].state, RowState::Gone, "the duck took it");
    let coin = open.drawn_scene().unwrap().object(7).unwrap().clone();
    assert!(coin.gone && coin.rect.is_none());
    let coin_started = coin.start_rect.expect("it shows where the coin starts");
    assert_eq!(
        run(
            &mut studio,
            serde_json::json!({"do": "object.move", "ids": [7], "by": [-96, 0]}),
        ),
        "Move coin"
    );
    settle(&mut studio);
    let text = std::fs::read_to_string(&scene_file).unwrap();
    assert!(
        text.contains(r#"{"id": 7, "name": "coin", "kind": "coin", "at": [152, 176]"#),
        "{text}"
    );
    let open = studio.open.as_ref().unwrap();
    let coin = open.drawn_scene().unwrap().object(7).unwrap();
    assert!(coin.gone, "it stays taken in this run");
    let scale = open.drawn_scene().unwrap().scale;
    assert_eq!(coin.start_rect.unwrap().x, coin_started.x - 96.0 * scale);

    say(&mut studio, Step::Do(Command::GameToggle));
    say(&mut studio, press("Right"));
    say(&mut studio, Step::Wait(Wait::Frames { frames: 30 }));
    say(&mut studio, release("Right"));
    say(&mut studio, Step::Do(Command::GameFreeze));
    settle(&mut studio);
    assert!(
        studio
            .open
            .as_ref()
            .unwrap()
            .undo
            .steps()
            .iter()
            .all(|step| step.now.is_empty()),
        "the game has played on: a move for that moment can't be undone any more"
    );
    let last = studio.play().unwrap().picture.clone().unwrap();
    say(&mut studio, Step::Do(Command::GameStop));
    let session = studio.open.as_ref().unwrap().last_session.clone().unwrap();

    // The file goes back as it was before the replay, which must not
    // need it: the session has the scene and every change on its frame.
    std::fs::write(&scene_file, &_put_back.text).unwrap();
    let replays = TestFolder::new("replay-objects");
    let out = replays.join("out");
    let replay = std::process::Command::new("cargo")
        .args([
            "run",
            "-q",
            "--no-default-features",
            "--example",
            "scene_demo",
            "--",
        ])
        .args(["--headless", "--inputs"])
        .arg(&session)
        .arg("--out")
        .arg(&out)
        .current_dir(duck_forge())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&replay.stdout);
    assert!(
        replay.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let (_, _, pixels) = png_pixels(&out.join("last.png"));
    assert!(
        pixels == last.pixels,
        "the replay ends on the session's last picture"
    );
}

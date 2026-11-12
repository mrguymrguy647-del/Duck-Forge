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

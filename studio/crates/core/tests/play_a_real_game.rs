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
use studio_core::{Command, Studio};

fn duck_forge() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

fn studio() -> Studio {
    // SAFETY: tests in this crate only ever set this variable to a scratch
    // file, so the person's own settings are never touched.
    unsafe {
        std::env::set_var(
            "DUCK_FORGE_STUDIO_SETTINGS",
            std::env::temp_dir().join("duck-forge-studio-test-settings.json"),
        );
    }
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
    let out = std::env::temp_dir().join(format!("duck-forge-studio-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
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
    let _ = std::fs::remove_dir_all(&out);
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

//! Review mode, one check at a time: each test makes a tiny film or game
//! that breaks one thing on purpose, reviews it, and looks for that one
//! finding (and, where it matters, for no finding when it's done right).

use duckforge::prelude::*;
use duckforge::{HeadlessOptions, InputAction, InputEvent, run_headless};
use serde_json::Value;
use std::path::PathBuf;

/// A film made of two functions: what happens each update, and what's
/// drawn. `t` is the frame.
#[derive(Clone)]
struct Film {
    t: u64,
    x: f32,
    update: fn(&mut Film, &mut Context),
    draw: fn(&Film, &mut Canvas),
}

impl Game for Film {
    fn update(&mut self, ctx: &mut Context) {
        self.t = ctx.frame();
        (self.update)(self, ctx);
    }
    fn draw(&self, canvas: &mut Canvas) {
        (self.draw)(self, canvas);
    }
}

fn film(update: fn(&mut Film, &mut Context), draw: fn(&Film, &mut Canvas)) -> Film {
    Film {
        t: 0,
        x: 0.0,
        update,
        draw,
    }
}

fn nothing(_: &mut Film, _: &mut Context) {}

fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("review-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Reviews `game` for `frames` at `width` x `height`, with these inputs,
/// and gives back review.json.
fn review_sized(
    name: &str,
    game: Film,
    frames: u64,
    size: (usize, usize),
    inputs: Option<InputScript>,
) -> Value {
    let out = folder(name);
    let config = Config {
        width: size.0,
        height: size.1,
        ..Config::default()
    };
    let options = HeadlessOptions {
        frames,
        out: Some(out.clone()),
        review: true,
        inputs,
        ..HeadlessOptions::default()
    };
    run_headless(config, game, &options).expect("the run");
    let text = std::fs::read_to_string(out.join("review/review.json")).expect("review.json");
    serde_json::from_str(&text).expect("JSON")
}

fn review(name: &str, game: Film, frames: u64) -> Value {
    review_sized(name, game, frames, (320, 240), None)
}

/// The findings from `check`: (severity, frame, text).
fn found(review: &Value, check: &str) -> Vec<(String, u64, String)> {
    review["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["check"] == check)
        .map(|f| {
            (
                f["severity"].as_str().unwrap().to_string(),
                f["frame"].as_u64().unwrap(),
                f["text"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn checks(review: &Value) -> Vec<String> {
    review["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["check"].as_str().unwrap().to_string())
        .collect()
}

// ---- text ---------------------------------------------------------------

#[test]
fn low_contrast_text_is_flagged_and_an_outline_fixes_it() {
    fn draw(_: &Film, c: &mut Canvas) {
        c.clear(Color::rgb(40, 40, 40));
        c.draw_text("DIM WORDS", vec2(20.0, 40.0), 3, Color::rgb(70, 70, 70));
        c.draw_text("BRIGHT WORDS", vec2(20.0, 120.0), 3, Color::WHITE);
    }
    let r = review("contrast", film(nothing, draw), 30);
    let flagged = found(&r, "text contrast");
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert_eq!(flagged[0].0, "flag");
    assert!(flagged[0].2.contains("DIM WORDS"), "{flagged:?}");

    // The same dim words with a light outline (drawn as copies round
    // them, as games do) are readable.
    fn outlined(_: &Film, c: &mut Canvas) {
        c.clear(Color::rgb(40, 40, 40));
        for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
            c.draw_text("DIM WORDS", vec2(20.0 + dx, 40.0 + dy), 3, Color::WHITE);
        }
        c.draw_text("DIM WORDS", vec2(20.0, 40.0), 3, Color::rgb(20, 20, 20));
    }
    let r = review("contrast-outlined", film(nothing, outlined), 30);
    assert!(
        found(&r, "text contrast").is_empty(),
        "{:?}",
        found(&r, "text contrast")
    );
}

#[test]
fn small_text_and_text_under_an_apps_buttons_are_named() {
    // A vertical video: a tiny caption, and a big one at the very bottom
    // where TikTok, Reels and Shorts put their captions and buttons.
    fn draw(_: &Film, c: &mut Canvas) {
        c.clear(Color::rgb(20, 30, 60));
        c.draw_text("tiny caption", vec2(200.0, 400.0), 1, Color::WHITE);
        c.draw_text("FOLLOW", vec2(150.0, 880.0), 6, Color::WHITE);
    }
    let r = review_sized("text-size-cover", film(nothing, draw), 30, (540, 960), None);
    let profiles: Vec<&str> = r["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert!(
        profiles.contains(&"TikTok") && profiles.contains(&"YouTube Shorts"),
        "{profiles:?}"
    );
    let small = found(&r, "text size");
    assert!(
        small
            .iter()
            .any(|f| f.2.contains("tiny caption") && f.2.contains("phone")),
        "{small:?}"
    );
    let covered = found(&r, "text covered");
    let follow = covered
        .iter()
        .find(|f| f.2.contains("FOLLOW"))
        .expect("FOLLOW is covered");
    assert!(
        follow.2.contains("TikTok") && follow.2.contains("bottom edge"),
        "{follow:?}"
    );
    assert!(
        !covered.iter().any(|f| f.2.contains("tiny caption")),
        "the middle is clear: {covered:?}"
    );
}

#[test]
fn a_tv_profile_only_warns_about_the_paused_controls() {
    fn draw(_: &Film, c: &mut Canvas) {
        c.clear(Color::BLACK);
        c.draw_text("SUBSCRIBE", vec2(300.0, 470.0), 4, Color::WHITE); // 1280x720: above the title-safe edge, under the paused controls
    }
    let r = review_sized("tv", film(nothing, draw), 20, (1280, 720), None);
    let covered = found(&r, "text covered");
    assert!(
        covered
            .iter()
            .any(|f| f.0 == "warn" && f.2.contains("player controls (when paused)"))
            || covered.is_empty(),
        "{covered:?}"
    );
    assert!(!covered.iter().any(|f| f.0 == "flag"), "{covered:?}");
}

// ---- visibility and expectations -----------------------------------------

#[test]
fn a_tiny_model_is_noticed_and_expectations_pass_or_fail() {
    fn update(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 1 {
            ctx.expect(Expect::at(0.5).thing("host").fills_height(0.4));
            ctx.expect(
                Expect::at(0.5)
                    .thing("panel")
                    .fills_height(0.4)
                    .lit_above(0.5),
            );
            ctx.expect(Expect::at(0.5).thing("nobody"));
            ctx.expect(Expect::at(0.5).caption_readable());
        }
    }
    fn draw(_: &Film, c: &mut Canvas) {
        c.clear(Color::rgb(30, 30, 40));
        // A cube far away: a few pixels tall.
        let camera = Camera3D::looking_at(vec3(0.0, 0.0, -60.0), Vec3::ZERO);
        let cube = Mesh::cube(Color::YELLOW);
        c.draw_mesh(&cube, &Transform::default(), &camera);
        c.label_model("host", &cube, &Transform::default(), &camera);
        // A big bright 2D panel.
        let panel = Rect::new(20.0, 20.0, 80.0, 200.0);
        c.fill_rect(panel, Color::WHITE);
        c.label("panel", panel);
    }
    let r = review("visibility", film(update, draw), 90);
    let tiny = found(&r, "visibility");
    assert!(
        tiny.iter().any(|f| f.2.contains("\"host\" is tiny")),
        "{tiny:?}"
    );
    let outcomes = r["expectations"].as_array().unwrap();
    let get = |words: &str| {
        outcomes
            .iter()
            .find(|o| o["expectation"].as_str().unwrap().contains(words))
            .unwrap()
    };
    assert_eq!(get("\"host\"")["passed"], false);
    assert!(
        get("\"host\"")["why"]
            .as_str()
            .unwrap()
            .contains("of the height")
    );
    assert_eq!(get("\"panel\"")["passed"], true, "{}", get("\"panel\""));
    assert_eq!(get("\"nobody\"")["passed"], false);
    assert!(
        get("\"nobody\"")["why"]
            .as_str()
            .unwrap()
            .contains("nothing is labelled")
    );
    assert_eq!(get("readable")["passed"], false, "no text at all");
}

// ---- motion ----------------------------------------------------------------

/// A lit floor and a few boxes seen from `camera`.
fn scene(c: &mut Canvas, camera: &Camera3D) {
    c.clear(Color::rgb(90, 140, 200));
    let floor = Mesh::cube(Color::rgb(90, 160, 90));
    c.draw_mesh(
        &floor,
        &Transform::at(vec3(0.0, -0.5, 0.0)).scaled(vec3(20.0, 0.2, 20.0)),
        camera,
    );
    for i in 0..5 {
        let x = i as f32 * 2.0 - 4.0;
        c.draw_mesh(
            &Mesh::cube(Color::rgb(200, 120, 60)),
            &Transform::at(vec3(x, 0.5, 3.0)),
            camera,
        );
    }
}

#[test]
fn a_shaking_camera_is_jitter_unless_it_is_meant() {
    fn draw(f: &Film, c: &mut Canvas) {
        let shake = if (30..90).contains(&f.t) {
            if f.t.is_multiple_of(2) { 0.08 } else { -0.08 }
        } else {
            0.0
        };
        let camera = Camera3D::looking_at(vec3(shake, 1.5, -5.0), vec3(0.0, 0.5, 2.0));
        scene(c, &camera);
    }
    let r = review("jitter", film(nothing, draw), 120);
    let jitter = found(&r, "camera jitter");
    assert_eq!(jitter.len(), 1, "{jitter:?}");
    assert!((25..=45).contains(&jitter[0].1), "{jitter:?}");

    fn meant(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 30 {
            ctx.intend("impact shake", 1.0);
        }
    }
    let r = review("jitter-meant", film(meant, draw), 120);
    assert!(
        found(&r, "camera jitter").is_empty(),
        "{:?}",
        found(&r, "camera jitter")
    );
}

#[test]
fn a_camera_jump_is_flagged_unless_a_cut_is_marked() {
    fn draw(f: &Film, c: &mut Canvas) {
        let x = if f.t < 60 {
            f.t as f32 * 0.02
        } else {
            12.0 + f.t as f32 * 0.02
        };
        let camera = Camera3D::looking_at(vec3(x, 1.5, -5.0), vec3(x, 0.5, 2.0));
        scene(c, &camera);
    }
    let r = review("jump", film(nothing, draw), 90);
    let jumps = found(&r, "camera jump");
    assert_eq!(jumps.len(), 1, "{jumps:?}");
    assert_eq!(jumps[0].1, 60);
    fn cut(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 60 {
            ctx.cut();
        }
    }
    let r = review("jump-cut", film(cut, draw), 90);
    assert!(found(&r, "camera jump").is_empty());
    let marks = r["marks"].as_array().unwrap();
    assert_eq!(marks[0]["kind"], "cut");
    assert_eq!(marks[0]["frame"], 60);
}

#[test]
fn frozen_dark_blown_and_flickering_stretches() {
    fn draw(f: &Film, c: &mut Canvas) {
        let color = match f.t {
            0..=200 => Color::rgb(4, 4, 6), // dark and still for over 3 s
            201..=380 => Color::WHITE,      // blown out for 3 s
            381..=390 => {
                if f.t.is_multiple_of(2) {
                    Color::BLACK
                } else {
                    Color::rgb(200, 200, 200)
                }
            } // flicker
            _ => Color::rgb(80, 120, 160),
        };
        c.clear(color);
        if f.t > 390 {
            c.fill_rect(
                Rect::new((f.t % 300) as f32, 100.0, 20.0, 20.0),
                Color::YELLOW,
            );
        }
    }
    let r = review("light", film(nothing, draw), 450);
    let dark = found(&r, "too dark");
    assert_eq!(dark.len(), 1, "{dark:?}");
    assert!(found(&r, "blown out").len() == 1, "{:?}", checks(&r));
    let flicker = found(&r, "flicker");
    assert_eq!(flicker.len(), 1, "{flicker:?}");
    assert_eq!(flicker[0].0, "flag");
    assert!(!found(&r, "frozen").is_empty(), "{:?}", checks(&r));
}

#[test]
fn a_camera_inside_a_model_or_a_block() {
    fn draw(f: &Film, c: &mut Canvas) {
        // The camera walks into a big box at frame 40 (seen from inside,
        // its faces are culled: one flat colour).
        let z = if f.t < 40 { -8.0 } else { 0.0 };
        let camera = Camera3D::looking_at(vec3(0.0, 1.0, z), vec3(0.0, 1.0, z + 5.0));
        c.clear(Color::rgb(10, 10, 10));
        let wall = Mesh::cube(Color::rgb(150, 150, 150));
        let at = Transform::at(vec3(0.0, 1.0, 0.0)).scaled(vec3(3.0, 3.0, 3.0));
        c.draw_mesh(&wall, &at, &camera);
        c.draw_mesh(
            &Mesh::cube(Color::RED),
            &Transform::at(vec3(2.5, 0.5, -3.0)),
            &camera,
        );
        c.label_model("wall", &wall, &at, &camera);
    }
    let r = review("inside-model", film(nothing, draw), 70);
    let inside = found(&r, "camera inside");
    assert!(
        inside.iter().any(|f| f.2.contains("\"wall\"") && f.1 == 40),
        "{inside:?} {:?}",
        checks(&r)
    );

    fn blocks(f: &Film, c: &mut Canvas) {
        let mut world = BlockWorld::new(8, 8, 8);
        world.set(4, 1, 4, 1);
        let x = if f.t < 20 { 1.5 } else { 4.5 };
        let camera = Camera3D::looking_at(vec3(x, 1.5, 4.5), vec3(x, 1.5, 8.0));
        c.clear(Color::rgb(90, 140, 200));
        world.draw(c, &camera);
    }
    let r = review("inside-block", film(nothing, blocks), 30);
    let inside = found(&r, "camera inside");
    assert_eq!(inside.len(), 1, "{inside:?}");
    assert!(inside[0].2.contains("a block at 4, 1, 4"), "{inside:?}");
    assert_eq!(inside[0].1, 20);
}

// ---- sound -----------------------------------------------------------------

/// A mono .wav: `silence` seconds of nothing, then a quarter second of tone.
fn wav_with_silence(name: &str, silence: f32) -> Wav {
    let rate = 44_100u32;
    let mut samples: Vec<i16> = vec![0; (silence * rate as f32) as usize];
    samples.extend((0..rate / 4).map(|i| ((i as f32 * 0.06).sin() * 20_000.0) as i16));
    let mut bytes = Vec::new();
    let data = (samples.len() * 2) as u32;
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * 2).to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    for s in samples {
        bytes.extend(s.to_le_bytes());
    }
    let path = folder("sounds").with_file_name(format!("{name}.wav"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    Wav::load(&path).unwrap()
}

fn plain(_: &Film, c: &mut Canvas) {
    c.clear(Color::rgb(40, 60, 80));
}

#[test]
fn a_sound_heard_late_for_its_event_is_flagged() {
    fn update(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 1 {
            ctx.expect(Expect::on("impact").sound("thump"));
            ctx.expect(Expect::sound_near("click", "impact", 2));
        }
        if ctx.frame() == 30 {
            ctx.event("impact");
            // 0.2 s of silence at its start: heard 12 frames late.
            ctx.play_for("impact", &wav_with_silence("thump", 0.2), 1.0, 0.0);
            ctx.play_for("impact", &wav_with_silence("click", 0.0), 0.5, 0.0);
        }
    }
    let r = review("sound-late", film(update, plain), 90);
    let late = found(&r, "sound timing");
    assert_eq!(late.len(), 1, "{late:?}");
    assert!(
        late[0]
            .2
            .contains("\"thump\" is heard 12 frames after \"impact\""),
        "{late:?}"
    );
    let outcomes = r["expectations"].as_array().unwrap();
    assert_eq!(outcomes[0]["passed"], false);
    assert!(
        outcomes[0]["why"]
            .as_str()
            .unwrap()
            .contains("12 frames after"),
        "{}",
        outcomes[0]
    );
    assert_eq!(outcomes[1]["passed"], true, "{}", outcomes[1]);
}

#[test]
fn a_mix_too_loud_names_its_sounds_and_suggests_volumes() {
    fn update(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 20 {
            for _ in 0..4 {
                ctx.play_with(Sound::explosion(), 1.0, 0.0);
            }
            ctx.play_with(Sound::coin(), 1.0, 0.0);
        }
    }
    let r = review("sound-loud", film(update, plain), 60);
    let loud = found(&r, "too loud");
    assert!(!loud.is_empty(), "{:?}", checks(&r));
    assert!(
        loud[0].2.contains("explosion") && loud[0].2.contains("try"),
        "{loud:?}"
    );
    assert!(r["peak_before_limiter"].as_f64().unwrap() > 1.0);
    assert!(
        PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("review-tests/sound-loud/review/spectrogram.png")
            .exists()
    );
}

#[test]
fn a_long_silence_is_noticed() {
    fn update(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 1 || ctx.frame() == 330 {
            ctx.play(Sound::coin());
        }
    }
    let r = review("silence", film(update, plain), 360);
    let quiet = found(&r, "silence");
    assert_eq!(quiet.len(), 1, "{quiet:?}");
    assert!(quiet[0].2.contains("silent for"), "{quiet:?}");
}

// ---- games -------------------------------------------------------------------

fn presses(frames: &[u64], key: Key, length: u64) -> InputScript {
    let mut script = InputScript {
        frames: Some(length),
        ..InputScript::default()
    };
    for &f in frames {
        script.events.push(InputEvent {
            frame: f,
            action: InputAction::Press(key),
        });
        script.events.push(InputEvent {
            frame: f + 2,
            action: InputAction::Release(key),
        });
    }
    script
}

#[test]
fn a_slow_response_to_a_key_is_flagged_with_a_replay() {
    // The square moves 6 frames after Space is pressed.
    fn update(f: &mut Film, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Space) {
            f.x = ctx.frame() as f32 + 6.0; // when to move
        }
    }
    fn draw(f: &Film, c: &mut Canvas) {
        c.clear(Color::BLACK);
        let moved = f.x > 0.0 && f.t as f32 >= f.x;
        let x = if moved { 200.0 } else { 40.0 };
        c.fill_rect(Rect::new(x, 100.0, 40.0, 40.0), Color::WHITE);
    }
    let r = review_sized(
        "response",
        film(update, draw),
        90,
        (320, 240),
        Some(presses(&[30], Key::Space, 90)),
    );
    let slow = found(&r, "response");
    assert!(
        slow.iter()
            .any(|f| f.0 == "flag" && f.2.contains("6 frames later")),
        "{slow:?}"
    );
    let replay = r["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["check"] == "response")
        .unwrap()["replay"]
        .as_str()
        .unwrap()
        .to_string();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("review-tests/response/review")
        .join(&replay);
    let script = InputScript::load(&path).unwrap();
    assert!(script.events.iter().any(|e| e.frame == 30));
    assert!(script.frames.unwrap() >= 36);
}

#[test]
fn a_quick_response_is_not_flagged() {
    fn update(f: &mut Film, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Space) {
            f.x = 1.0;
        }
    }
    fn draw(f: &Film, c: &mut Canvas) {
        c.clear(Color::BLACK);
        c.fill_rect(
            Rect::new(if f.x > 0.0 { 200.0 } else { 40.0 }, 100.0, 40.0, 40.0),
            Color::WHITE,
        );
    }
    let r = review_sized(
        "response-quick",
        film(update, draw),
        90,
        (320, 240),
        Some(presses(&[30], Key::Space, 90)),
    );
    assert!(
        found(&r, "response").iter().all(|f| f.0 != "flag"),
        "{:?}",
        found(&r, "response")
    );
}

#[test]
fn a_player_stuck_while_keys_are_held() {
    fn update(f: &mut Film, ctx: &mut Context) {
        // It walks right, until a wall at x = 100 it never gets past.
        if ctx.input.is_down(Key::Right) {
            f.x = (f.x + 2.0).min(100.0);
        }
    }
    fn draw(f: &Film, c: &mut Canvas) {
        c.clear(Color::rgb(30, 30, 50));
        let player = Rect::new(f.x, 120.0, 16.0, 16.0);
        c.fill_rect(player, Color::YELLOW);
        c.label("player", player);
    }
    let mut script = InputScript {
        frames: Some(480),
        ..InputScript::default()
    };
    script.events.push(InputEvent {
        frame: 1,
        action: InputAction::Press(Key::Right),
    });
    let r = review_sized("stuck", film(update, draw), 480, (320, 240), Some(script));
    let stuck = found(&r, "stuck");
    assert_eq!(stuck.len(), 1, "{stuck:?}");
    assert!((50..=52).contains(&stuck[0].1), "{stuck:?}");
}

#[test]
fn a_slow_update_is_named_with_its_frame() {
    fn update(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 30 {
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
    let r = review("speed", film(update, plain), 60);
    let slow = found(&r, "speed");
    assert!(slow.iter().any(|f| f.0 == "warn" && f.1 == 30), "{slow:?}");
    assert!(
        slow.iter()
            .any(|f| f.0 == "note" && f.2.contains("slowest frames")),
        "{slow:?}"
    );
}

#[test]
fn the_review_folder_has_its_pictures_and_events_get_filmstrips() {
    fn update(_: &mut Film, ctx: &mut Context) {
        if ctx.frame() == 20 {
            ctx.place("cellar");
        }
        if ctx.frame() == 40 {
            ctx.event("door opens");
            ctx.play_for("door opens", Sound::hit(), 0.6, 0.0);
        }
    }
    fn draw(f: &Film, c: &mut Canvas) {
        c.clear(Color::rgb(40, 50, 70));
        c.draw_text("A CAPTION", vec2(40.0, 40.0), 3, Color::WHITE);
        c.fill_rect(
            Rect::new(f.t as f32 * 3.0, 150.0, 20.0, 20.0),
            Color::YELLOW,
        );
    }
    let r = review("pictures", film(update, draw), 80);
    let files: Vec<&str> = r["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    for want in [
        "overview.png",
        "viewer.png",
        "spectrogram.png",
        "review.txt",
        "review.json",
    ] {
        assert!(files.contains(&want), "{want} missing: {files:?}");
    }
    assert!(
        files.iter().any(|f| f.starts_with("event-01-door-opens")),
        "{files:?}"
    );
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("review-tests/pictures/review/review.txt"),
    )
    .unwrap();
    assert!(text.starts_with("Review of"), "{text}");
    assert!(
        text.contains("Judged as seen on: this game's window (320x240)"),
        "{text}"
    );
}

// ---- videos made anywhere ----------------------------------------------------

fn ffmpeg_here() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_video_from_elsewhere_is_reviewed_from_its_pixels_and_sound() {
    if !ffmpeg_here() {
        eprintln!("skipped: no ffmpeg");
        return;
    }
    // Made with ffmpeg, not the engine: two seconds of a test pattern and
    // a tone, then a second of flashing black and white.
    let dir = folder("external");
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("made-elsewhere.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=360x640:rate=30:duration=2",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=c=black:size=360x640:rate=30:duration=1",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args([
            "-filter_complex",
            "[1:v]geq=lum='if(mod(N,2),235,16)':cb=128:cr=128[flash];[0:v][flash]concat=n=2:v=1[v]",
        ])
        .args([
            "-map", "[v]", "-map", "2:a", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-c:a", "aac",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());
    let review = duckforge::review::review_video(&video, None, None).unwrap();
    assert_eq!((review.width, review.height), (360, 640));
    assert!((88..=92).contains(&review.frames), "{}", review.frames);
    assert!(
        review.profiles.iter().any(|p| p == "TikTok"),
        "{:?}",
        review.profiles
    );
    assert!(!review.of("flicker").is_empty(), "{}", review.to_text());
    let folder = dir.join("made-elsewhere-review");
    for want in [
        "overview.png",
        "spectrogram.png",
        "review.txt",
        "review.json",
    ] {
        assert!(folder.join(want).exists(), "{want}");
    }
    assert!(review.took < 30.0);
}

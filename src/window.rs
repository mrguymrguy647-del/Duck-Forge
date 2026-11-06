//! The window: opening it, the real-time game loop, and reading the
//! keyboard and mouse from it.
//!
//! This is the only file that talks to the window library (`minifb`), and
//! it's only built with the `window` feature (on by default). Everything
//! else in the engine is plain Rust, so switching to a different window
//! library would mean rewriting just this file, and the engine also builds
//! on a machine with no screen at all.

use std::error::Error;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use minifb::{MouseMode, ScaleMode, Window, WindowOptions};

use crate::canvas::Canvas;
use crate::cli::Options;
use crate::color::Color;
use crate::engine::{Config, EngineError, Game, Timestep};
use crate::input::{Key, MouseButton};
use crate::math::{Rect, Vec2, vec2};
use crate::render3d::Renderer;
use crate::rng;
use crate::script::{InputScript, timestep_warning};
use crate::simulation::Simulation;

/// In [`Timestep::RealTime`] mode, the longest time step a game will ever
/// see, in seconds.
///
/// If the game freezes for a moment (say, while the window is being dragged)
/// the next frame would otherwise get a huge `dt`, and fast objects would
/// teleport straight through walls.
const MAX_DT: f32 = 1.0 / 30.0;

/// In [`Timestep::Fixed`] mode, the most real time one drawn frame can catch
/// up on, in seconds.
///
/// After a freeze, the game would otherwise run dozens of updates in a row
/// to catch up. On a computer that is simply too slow, that makes every frame
/// slower still, and it never catches up. So past this, the game slows down
/// instead.
const MAX_CATCH_UP: f64 = 0.1;

/// Decides how many updates each drawn frame needs, from how long the frame took.
///
/// With a fixed timestep, real time is paid into `owed`, and every update
/// pays out one step. This is the classic "fix your timestep" loop.
#[derive(Debug)]
struct Pacer {
    timestep: Timestep,
    /// Real time that has passed but that the game hasn't caught up on yet, in seconds.
    owed: f64,
}

impl Pacer {
    fn new(timestep: Timestep) -> Self {
        let mut pacer = Self {
            timestep,
            owed: 0.0,
        };
        // One and a half steps: the first frame updates once before it
        // draws, and after that `owed` sits half a step away from both
        // edges. Frames are never exactly 1/60 s apart, and a frame that is
        // a little early or late then still gets exactly one update, instead
        // of zero followed by two (which would look like a stutter).
        pacer.owed = pacer.step() * 1.5;
        pacer
    }

    /// Seconds per update with a fixed timestep (0.0 in real-time mode).
    fn step(&self) -> f64 {
        match self.timestep {
            Timestep::Fixed { updates_per_second } => 1.0 / updates_per_second.max(1) as f64,
            Timestep::RealTime => 0.0,
        }
    }

    /// For a frame that took `frame_time` seconds: how many updates to run
    /// before drawing it, and the `dt` to give each one.
    fn updates_due(&mut self, frame_time: f64) -> (u32, f32) {
        if self.timestep == Timestep::RealTime {
            return (1, (frame_time as f32).min(MAX_DT));
        }
        let step = self.step();
        if frame_time > MAX_CATCH_UP {
            // Too far behind to catch up: slow down, and start over centered.
            self.owed = step * 1.5;
        } else {
            self.owed += frame_time;
        }
        let updates = (self.owed / step).floor();
        self.owed -= updates * step;
        (updates as u32, step as f32)
    }
}

/// Opens a window and runs `game` until the window is closed, the game
/// calls [`Context::quit`](crate::engine::Context::quit), or `options.frames` updates have run.
///
/// This is the *game loop*, the core of every game engine. Each frame it:
/// 1. measures how much time has passed, and so how many updates are due,
/// 2. reads the keyboard and mouse,
/// 3. lets the game update itself (usually once per frame),
/// 4. lets the game draw itself,
/// 5. shows the result on screen.
///
/// The game itself runs inside a [`Simulation`], the same thing tests use,
/// so it behaves the same in a window and in a test. The loop just adds a
/// clock and a screen.
///
/// These keys work in every game: **F1** shows or hides the debug overlay,
/// **F3** an FPS counter, **F10** starts and stops recording, and **F12**
/// saves a screenshot.
pub(crate) fn run<G: Game>(
    mut config: Config,
    game: G,
    options: &Options,
) -> Result<(), Box<dyn Error>> {
    config.check().map_err(EngineError)?;
    if let Some(corner) = options.debug_corner {
        config.debug_corner = corner;
    }
    let script = match &options.inputs {
        Some(path) => Some(InputScript::load(path).map_err(EngineError)?),
        None => None,
    };
    if let Some(warning) = script
        .as_ref()
        .and_then(|script| timestep_warning(script, &config))
    {
        eprintln!("duckforge: {warning}");
    }
    // Pick the seed now, so a run without one still knows which it used
    // (a recording needs it). A replay uses the seed it was recorded with.
    config.seed = Some(
        options
            .seed
            .or(script.as_ref().and_then(|script| script.seed))
            .or(config.seed)
            .unwrap_or_else(rng::seed_from_clock),
    );
    let window_options = WindowOptions {
        resize: true,
        scale_mode: ScaleMode::AspectRatioStretch,
        ..WindowOptions::default()
    };
    let mut window = Window::new(
        &config.title,
        config.width * config.scale,
        config.height * config.scale,
        window_options,
    )
    .map_err(|err| {
        EngineError(format!(
            "could not open a window ({err}).\n\
             On a computer without a screen, run the game with --headless instead."
        ))
    })?;
    // minifb waits inside `update_with_buffer` so we don't run faster than this.
    window.set_target_fps(config.target_fps);

    let renderer = renderer_from_env().unwrap_or(config.renderer);
    let tunes = crate::tune::Tunes::for_run(
        options.tune.as_deref(),
        config.tune_file.as_deref(),
        &options.set,
    )
    .map_err(EngineError)?;
    let mut sim = Simulation::start(&config, game, renderer);
    sim.set_tunes(tunes);
    sim.forget_report_log();
    #[cfg(feature = "audio")]
    let speaker = match crate::speaker::Speaker::open() {
        Ok(speaker) => Some(speaker),
        Err(problem) => {
            println!("duckforge: no sound: {problem}; the game carries on silently");
            None
        }
    };
    if let Some(script) = script {
        sim.follow(script);
    }
    let mut recorder = Recorder {
        file: options.record.clone(),
        on: options.record.is_some(),
    };
    let mut title = String::new();
    let mut was_replaying = sim.is_replaying();
    let mut pacer = Pacer::new(config.timestep);
    let mut show_fps = config.show_fps;
    let mut show_debug = options.debug;
    sim.set_debug_overlay(show_debug);
    let mut keys_last_frame = Vec::new();
    let mut last_frame = Instant::now();
    let mut last_tune_check = Instant::now();
    let mut checks_reported = 0;

    let frames = options.frames.unwrap_or(u64::MAX);

    while window.is_open() && !sim.quit_requested() && sim.context().frame() < frames {
        // 1. Time.
        let now = Instant::now();
        let frame_time = now.duration_since(last_frame).as_secs_f64();
        last_frame = now;
        sim.measure_frame(frame_time as f32);
        let (updates, dt) = pacer.updates_due(frame_time);

        // A resizable canvas follows the window's size (but not down to
        // nothing while the window is minimized).
        let (window_w, window_h) = window.get_size();
        if config.resizable && window_w > 0 && window_h > 0 {
            sim.resize(window_w / config.scale, window_h / config.scale);
        }

        // 2. Input: translate the window library's keys and buttons into our
        // own types. During a replay, the script plays instead of the player.
        let keys: Vec<Key> = window
            .get_keys()
            .into_iter()
            .filter_map(convert_key)
            .collect();
        if !sim.is_replaying() {
            sim.hold_keys(&keys);
            let canvas_size = (
                sim.context().width() as usize,
                sim.context().height() as usize,
            );
            read_mouse(&window, canvas_size, &mut sim);
        }

        // 3. The game's turn to update, as many times as are due. On a
        // computer that keeps up, that's once.
        for _ in 0..updates {
            sim.step_by(dt);
            if sim.quit_requested() || sim.context().frame() >= frames {
                break;
            }
        }

        // A check that just failed for the first time gets a line in the console.
        for failed in &sim.failed_checks()[checks_reported..] {
            println!(
                "duckforge: CHECK FAILED on frame {}: {}",
                failed.frame, failed.name
            );
        }
        checks_reported = sim.failed_checks().len();

        // Sounds played during those updates go to the speakers.
        let sounds = sim.take_new_sounds();
        #[cfg(feature = "audio")]
        if let Some(speaker) = &speaker {
            for sound in sounds {
                speaker.play(sound.samples);
            }
        }
        let loops = sim.take_new_loop_commands();
        #[cfg(feature = "audio")]
        if let Some(speaker) = &speaker {
            for command in loops {
                speaker.change_loop(command);
            }
        }
        #[cfg(not(feature = "audio"))]
        drop((sounds, loops)); // built without the "audio" feature: silent

        // 4. The game's turn to draw.
        let fps = sim.context().fps();
        let pressed = |key| keys.contains(&key) && !keys_last_frame.contains(&key);
        if pressed(Key::F1) {
            show_debug = !show_debug;
            sim.set_debug_overlay(show_debug);
        }
        let canvas = sim.draw_for_screen();
        if pressed(Key::F3) {
            show_fps = !show_fps;
        }
        if show_fps {
            draw_fps(canvas, fps);
        }
        if pressed(Key::F12) {
            save_screenshot(canvas);
        }
        let toggle_recording = pressed(Key::F10);

        // 5. Show the frame. This also collects new keyboard and mouse events from the OS.
        window.update_with_buffer(canvas.pixels(), canvas.width(), canvas.height())?;
        keys_last_frame = keys;

        if toggle_recording {
            recorder.toggle(&sim);
        }
        if was_replaying && !sim.is_replaying() && !sim.quit_requested() {
            println!("duckforge: the replay is over, and you're in control");
        }
        was_replaying = sim.is_replaying();
        let wanted = match (recorder.on, was_replaying) {
            (true, _) => format!("{} [REC]", config.title),
            (false, true) => format!("{} [REPLAY]", config.title),
            (false, false) => config.title.clone(),
        };
        if wanted != title {
            window.set_title(&wanted);
            title = wanted;
        }

        // Pick up edits to the tune file, twice a second.
        if last_tune_check.elapsed().as_secs_f32() >= 0.5 {
            last_tune_check = Instant::now();
            match sim.tunes_mut().reload_if_changed() {
                Some(Ok(message)) => println!("duckforge: {message}"),
                Some(Err(problem)) => println!("duckforge: {problem}; keeping the old values"),
                None => {}
            }
        }
    }

    // Closing the window doesn't lose a recording.
    if recorder.on {
        recorder.toggle(&sim);
    }
    for name in sim.tunes().unused_sets() {
        println!("duckforge: warning: --set {name} was never asked for (check the spelling)");
    }
    if options.write_tune {
        let path = sim.tunes().write_file().map_err(EngineError)?;
        println!("duckforge: wrote the tune values to {}", path.display());
    }
    Ok(())
}

/// Whether F10 (or `--record`) is recording, and where to save it.
struct Recorder {
    /// The file from `--record`, or `None` to pick `recording-1.json` and so on.
    file: Option<PathBuf>,
    on: bool,
}

impl Recorder {
    /// Starts recording, or stops and saves.
    fn toggle<G: Game>(&mut self, sim: &Simulation<G>) {
        if !self.on {
            self.on = true;
            println!("duckforge: recording; press F10 again to save");
            return;
        }
        self.on = false;
        let path = self.file.clone().unwrap_or_else(next_recording_file);
        // A replay always starts from the beginning of the game, so the file
        // holds everything since the game started, not just since F10.
        let recording = sim.recording();
        match recording.save(&path) {
            Ok(()) => println!(
                "duckforge: saved {} ({} frames, {} input events); replay it with --inputs {}",
                path.display(),
                recording.frames.unwrap_or(0),
                recording.events.len(),
                path.display(),
            ),
            Err(err) => eprintln!("duckforge: {err}"),
        }
    }
}

/// The first of `recording-1.json`, `recording-2.json`, ... that doesn't exist yet.
fn next_recording_file() -> PathBuf {
    (1..)
        .map(|n| PathBuf::from(format!("recording-{n}.json")))
        .find(|path| !path.exists())
        .expect("there is always a free number")
}

/// Passes the mouse's position, buttons and scroll wheel on to the game.
fn read_mouse<G: Game>(window: &Window, canvas_size: (usize, usize), sim: &mut Simulation<G>) {
    let position = window
        .get_mouse_pos(MouseMode::Pass)
        .and_then(|mouse| window_to_canvas(mouse, window.get_size(), canvas_size));
    if let Some(position) = position {
        sim.move_mouse(position);
    }

    let buttons = [
        (minifb::MouseButton::Left, MouseButton::Left),
        (minifb::MouseButton::Right, MouseButton::Right),
        (minifb::MouseButton::Middle, MouseButton::Middle),
    ];
    let held: Vec<MouseButton> = buttons
        .into_iter()
        .filter(|&(theirs, _)| window.get_mouse_down(theirs))
        .map(|(_, ours)| ours)
        .collect();
    sim.hold_mouse_buttons(&held);

    // Different systems report different amounts per wheel "click", so
    // `signum` boils it down to just the direction: 1.0 or -1.0.
    if let Some((_, y)) = window.get_scroll_wheel() {
        if y != 0.0 {
            sim.scroll(y.signum());
        }
    }
}

/// Converts a mouse position in window pixels into canvas pixels.
///
/// The canvas is scaled up as much as fits while keeping its shape, then
/// centered (with black bars filling any leftover space), so we undo the
/// centering and the scaling. Returns `None` if the window has no size.
fn window_to_canvas(
    mouse: (f32, f32),
    window_size: (usize, usize),
    canvas_size: (usize, usize),
) -> Option<Vec2> {
    let (window_w, window_h) = (window_size.0 as f32, window_size.1 as f32);
    let (canvas_w, canvas_h) = (canvas_size.0 as f32, canvas_size.1 as f32);
    if canvas_w == 0.0 || canvas_h == 0.0 {
        return None; // nothing to point at
    }
    let scale = (window_w / canvas_w).min(window_h / canvas_h);
    if scale <= 0.0 {
        return None; // minimized
    }
    let offset_x = (window_w - canvas_w * scale) / 2.0;
    let offset_y = (window_h - canvas_h * scale) / 2.0;
    Some(vec2(
        (mouse.0 - offset_x) / scale,
        (mouse.1 - offset_y) / scale,
    ))
}

/// Reads the `DUCKFORGE_RENDERER` environment variable, if it's set:
/// `cpu`, `gpu` or `auto`. It lets you switch renderers without changing code.
fn renderer_from_env() -> Option<Renderer> {
    let value = std::env::var("DUCKFORGE_RENDERER").ok()?;
    match value.to_lowercase().as_str() {
        "cpu" => Some(Renderer::Cpu),
        "gpu" => Some(Renderer::Gpu),
        "auto" => Some(Renderer::Auto),
        other => {
            eprintln!("duckforge: ignoring DUCKFORGE_RENDERER={other:?} (use cpu, gpu or auto)");
            None
        }
    }
}

fn draw_fps(canvas: &mut Canvas, fps: f32) {
    let text = format!("FPS {}", fps.round());
    let y = canvas.height() as f32 - 9.0;
    let background = Rect::new(0.0, y, Canvas::text_width(&text, 1) + 4.0, 9.0);
    canvas.fill_rect(background, Color::BLACK);
    canvas.draw_text(&text, vec2(2.0, y + 2.0), 1, Color::YELLOW);
}

fn save_screenshot(canvas: &Canvas) {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = format!("screenshot-{millis}.png");

    // A failed screenshot shouldn't crash the game, so we report the error
    // instead of passing it up with `?`.
    match canvas.save_png(&path) {
        Ok(()) => println!("Saved {path}"),
        Err(err) => eprintln!("Could not save {path}: {err}"),
    }
}

/// Maps a `minifb` key to our own [`Key`], or `None` for keys games can't ask about.
fn convert_key(key: minifb::Key) -> Option<Key> {
    use minifb::Key as K;

    let key = match key {
        K::A => Key::A,
        K::B => Key::B,
        K::C => Key::C,
        K::D => Key::D,
        K::E => Key::E,
        K::F => Key::F,
        K::G => Key::G,
        K::H => Key::H,
        K::I => Key::I,
        K::J => Key::J,
        K::K => Key::K,
        K::L => Key::L,
        K::M => Key::M,
        K::N => Key::N,
        K::O => Key::O,
        K::P => Key::P,
        K::Q => Key::Q,
        K::R => Key::R,
        K::S => Key::S,
        K::T => Key::T,
        K::U => Key::U,
        K::V => Key::V,
        K::W => Key::W,
        K::X => Key::X,
        K::Y => Key::Y,
        K::Z => Key::Z,
        K::Key0 => Key::Digit0,
        K::Key1 => Key::Digit1,
        K::Key2 => Key::Digit2,
        K::Key3 => Key::Digit3,
        K::Key4 => Key::Digit4,
        K::Key5 => Key::Digit5,
        K::Key6 => Key::Digit6,
        K::Key7 => Key::Digit7,
        K::Key8 => Key::Digit8,
        K::Key9 => Key::Digit9,
        K::Left => Key::Left,
        K::Right => Key::Right,
        K::Up => Key::Up,
        K::Down => Key::Down,
        K::Space => Key::Space,
        K::Enter => Key::Enter,
        K::Escape => Key::Escape,
        K::Tab => Key::Tab,
        K::Backspace => Key::Backspace,
        K::LeftShift => Key::LeftShift,
        K::RightShift => Key::RightShift,
        K::LeftCtrl => Key::LeftCtrl,
        K::RightCtrl => Key::RightCtrl,
        K::LeftAlt => Key::LeftAlt,
        K::RightAlt => Key::RightAlt,
        K::F1 => Key::F1,
        K::F2 => Key::F2,
        K::F3 => Key::F3,
        K::F4 => Key::F4,
        K::F5 => Key::F5,
        K::F6 => Key::F6,
        K::F7 => Key::F7,
        K::F8 => Key::F8,
        K::F9 => Key::F9,
        K::F10 => Key::F10,
        K::F11 => Key::F11,
        K::F12 => Key::F12,
        _ => return None,
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the pacer over a list of frame times, and returns how many
    /// updates each frame got.
    fn updates_per_frame(timestep: Timestep, frame_times: &[f64]) -> Vec<u32> {
        let mut pacer = Pacer::new(timestep);
        frame_times
            .iter()
            .map(|&time| pacer.updates_due(time).0)
            .collect()
    }

    #[test]
    fn a_fixed_timestep_runs_as_many_updates_as_the_time_needs() {
        let sixty = Timestep::default();
        // The first frame (timed from just before the loop) still updates once.
        assert_eq!(updates_per_frame(sixty, &[0.0]), [1]);
        // A computer drawing 60 frames per second: one update each.
        assert_eq!(
            updates_per_frame(sixty, &[0.0, 1.0 / 60.0, 1.0 / 60.0]),
            [1, 1, 1]
        );
        // Only 30 frames per second: two updates each, so the game keeps its speed.
        assert_eq!(
            updates_per_frame(sixty, &[0.0, 1.0 / 30.0, 1.0 / 30.0]),
            [1, 2, 2]
        );
        // A 120 Hz screen: an update every other frame.
        let fast_screen = [0.0, 1.0 / 120.0, 1.0 / 120.0, 1.0 / 120.0, 1.0 / 120.0];
        assert_eq!(updates_per_frame(sixty, &fast_screen), [1, 1, 0, 1, 0]);
        // `dt` is always the same.
        assert_eq!(Pacer::new(sixty).updates_due(0.05).1, 1.0 / 60.0);
    }

    #[test]
    fn a_little_jitter_doesnt_cause_stutters() {
        // Frames are never exactly 1/60 s apart. Each one should still get one update.
        let jittery = [0.0, 0.0155, 0.0178, 0.0160, 0.0175, 0.0152, 0.0181];
        assert_eq!(updates_per_frame(Timestep::default(), &jittery), [1; 7]);
    }

    #[test]
    fn after_a_freeze_the_game_slows_down_instead_of_racing_to_catch_up() {
        let mut pacer = Pacer::new(Timestep::default());
        let (updates, _) = pacer.updates_due(2.0); // the window was dragged for 2 seconds
        assert_eq!(updates, 1);
        assert_eq!(pacer.updates_due(1.0 / 60.0).0, 1);
        // A slow frame within the limit is caught up on in full: 0.1 s is 6 updates.
        assert_eq!(
            updates_per_frame(Timestep::default(), &[0.0, MAX_CATCH_UP]),
            [1, 6]
        );
    }

    #[test]
    fn real_time_mode_uses_the_clock_but_caps_dt() {
        let mut pacer = Pacer::new(Timestep::RealTime);
        assert_eq!(pacer.updates_due(0.01), (1, 0.01));
        assert_eq!(pacer.updates_due(0.5), (1, MAX_DT));
    }

    #[test]
    fn keys_are_converted() {
        assert_eq!(convert_key(minifb::Key::Space), Some(Key::Space));
        assert_eq!(convert_key(minifb::Key::Key7), Some(Key::Digit7));
        assert_eq!(convert_key(minifb::Key::Q), Some(Key::Q));
        assert_eq!(convert_key(minifb::Key::NumLock), None);
    }

    #[test]
    fn mouse_maps_from_window_to_canvas() {
        // Window exactly 3x the canvas: just divide by 3.
        let p = window_to_canvas((480.0, 360.0), (960, 720), (320, 240));
        assert_eq!(p, Some(vec2(160.0, 120.0)));

        // A wider window: the canvas is centered with 120-pixel bars on each side.
        let p = window_to_canvas((120.0, 0.0), (1200, 720), (320, 240));
        assert_eq!(p, Some(vec2(0.0, 0.0)));

        // A minimized window has no size.
        assert_eq!(window_to_canvas((5.0, 5.0), (0, 0), (320, 240)), None);
    }

    #[test]
    fn a_canvas_with_no_pixels_has_no_mouse_position() {
        assert_eq!(window_to_canvas((5.0, 5.0), (100, 100), (0, 0)), None);
    }
}

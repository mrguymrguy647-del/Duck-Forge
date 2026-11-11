//! The window: opening it, the real-time game loop, and reading the
//! keyboard and mouse from it.
//!
//! This is the only file that talks to the window libraries, and it's only
//! built with the `window` feature (on by default). `winit` opens the
//! window and reports the keyboard and mouse (and can lock the pointer for
//! mouse look); `softbuffer` puts our pixels on the screen. Everything else
//! in the engine is plain Rust, so switching to a different window library
//! would mean rewriting just this file, and the engine also builds on a
//! machine with no screen at all.

use std::error::Error;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::canvas::Canvas;
use crate::cli::Options;
use crate::color::Color;
use crate::engine::{Config, EngineError, Game, Timestep};
use crate::input::{Key, MouseButton};
use crate::math::{Rect, Vec2, vec2};
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
    let no_window = |problem: String| {
        EngineError(format!(
            "could not open a window ({problem}).\n\
             On a computer without a screen, run the game with --headless instead."
        ))
    };
    let (mut event_loop, mut screen) = open_screen(&config).map_err(no_window)?;
    let frame_budget = match config.target_fps {
        0 => Duration::ZERO, // no limit
        fps => Duration::from_secs_f64(1.0 / fps as f64),
    };

    let renderer = crate::render3d::renderer_from_env().unwrap_or(config.renderer);
    let tunes = crate::tune::Tunes::for_run(
        options.tune.as_deref(),
        config.tune_file.as_deref(),
        &options.set,
    )
    .map_err(EngineError)?;
    let mut sim = Simulation::start(&config, game, renderer);
    sim.set_run_mode(crate::engine::RunMode::Window);
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
    let mut title = config.title.clone();
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

    while !screen.closed && !sim.quit_requested() && sim.context().frame() < frames {
        // Collect what happened since last frame: keys, mouse, resizing, closing.
        if !pump(&mut event_loop, &mut screen, Duration::ZERO) {
            break;
        }
        if screen.closed {
            break;
        }

        // 1. Time.
        let now = Instant::now();
        let frame_time = now.duration_since(last_frame).as_secs_f64();
        last_frame = now;
        sim.measure_frame(frame_time as f32);
        let (updates, dt) = pacer.updates_due(frame_time);

        // A resizable canvas follows the window's size (but not down to
        // nothing while the window is minimized).
        let (window_w, window_h) = screen.logical_size();
        if config.resizable && window_w > 0 && window_h > 0 {
            sim.resize(window_w / config.scale, window_h / config.scale);
        }

        // 2. Input, already in our own types. During a replay, the script
        // plays instead of the player.
        let canvas_size = (
            sim.context().width() as usize,
            sim.context().height() as usize,
        );
        let input = screen.take_input(canvas_size);
        if !sim.is_replaying() {
            sim.hold_keys(&input.keys);
            match input.mouse {
                MouseInput::At(position) => sim.move_mouse(position),
                MouseInput::Moved(amount) => sim.move_mouse_by(amount),
                MouseInput::Nothing => {}
            }
            sim.hold_mouse_buttons(&input.buttons);
            if input.scroll != 0.0 {
                sim.scroll(input.scroll);
            }
        }

        // 3. The game's turn to update, as many times as are due. On a
        // computer that keeps up, that's once.
        for _ in 0..updates {
            sim.step_by(dt);
            if sim.quit_requested() || sim.context().frame() >= frames {
                break;
            }
        }
        // The game may have asked to lock or free the pointer.
        screen.hold_pointer(sim.context().pointer_locked());

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
                speaker.play(sound.samples, sound.pan);
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
        let keys = input.keys;
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

        // 5. Show the frame.
        screen.show(canvas)?;
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
            screen.set_title(&wanted);
            title = wanted;
        }

        // Pick up edits to the tune file and the files the game read
        // (its level), twice a second.
        if last_tune_check.elapsed().as_secs_f32() >= 0.5 {
            last_tune_check = Instant::now();
            pick_up_edits(&mut sim);
        }

        // Don't run faster than `target_fps`: wait out the rest of the frame.
        if let Some(spare) = frame_budget.checked_sub(now.elapsed()) {
            std::thread::sleep(spare);
        }
    }
    screen.hold_pointer(false);
    // Every way out of the loop ends here: the close button, ctx.quit(),
    // --frames. The game gets to say goodbye (and save).
    sim.quit();

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

/// Hands the window system's news (keys, the mouse, resizing, closing) to
/// the screen, waiting up to `patience` for some. False once the window
/// system says the program should end.
///
/// Every place that needs the news comes through here. winit's code for
/// it is built afresh for each place that asks, and it's big: asking from
/// one place keeps a game's program about 150 KB smaller.
#[inline(never)]
fn pump(event_loop: &mut EventLoop<()>, screen: &mut Screen, patience: Duration) -> bool {
    !matches!(
        event_loop.pump_app_events(Some(patience), screen),
        PumpStatus::Exit(_)
    )
}

/// Takes in edits made while the game runs: the tune file's values, and
/// the files the game read (its level). Each is a change the recording
/// keeps on its frame, so a replay plays out the same.
fn pick_up_edits<G: Game>(sim: &mut Simulation<G>) {
    match sim.tunes_mut().reload_if_changed() {
        Some(Ok(message)) => {
            println!("duckforge: {message}");
            let changed = sim.tunes_mut().differences_from_file();
            sim.change_tunes(changed);
        }
        Some(Err(problem)) => println!("duckforge: {problem}; keeping the old values"),
        None => {}
    }
    for (name, text) in crate::files::changed_on_disk() {
        if sim.change_file(&name, &text) {
            println!("duckforge: {name} changed; the game read it again");
        } else {
            println!(
                "duckforge: {name} changed, but this game only reads it when it starts \
                 (it has no reload): start it again to see the change"
            );
        }
    }
}

/// Opens the game's window, or says why it couldn't.
fn open_screen(config: &Config) -> Result<(EventLoop<()>, Screen), String> {
    let mut event_loop = new_event_loop()?;
    let mut screen = Screen::new(
        &config.title,
        LogicalSize::new(
            (config.width * config.scale) as f64,
            (config.height * config.scale) as f64,
        ),
    );
    // The window is made during the first few events (winit says when it's ready).
    let opening = Instant::now();
    while screen.window.is_none() && screen.problem.is_none() {
        if !pump(&mut event_loop, &mut screen, Duration::from_millis(10)) {
            break;
        }
        if opening.elapsed() > Duration::from_secs(10) {
            screen.problem = Some(String::from("the system never said the window was ready"));
        }
    }
    match screen.problem.take() {
        Some(problem) => Err(problem),
        None => Ok((event_loop, screen)),
    }
}

/// A window for a game that's already running: Duck Forge Studio's "Pop
/// out". The game carries on from the frame it was on, with the same loop
/// as [`run`], one turn at a time, so the Studio's link can listen between
/// turns. A program can only connect to the system's windows once, so
/// "bring it back" hides the window and the next pop out shows it again.
#[cfg(feature = "studio")]
pub(crate) struct PoppedOut {
    event_loop: EventLoop<()>,
    screen: Screen,
    pacer: Pacer,
    last_frame: Instant,
    frame_budget: Duration,
    scale: usize,
    resizable: bool,
}

#[cfg(feature = "studio")]
impl PoppedOut {
    pub(crate) fn open(config: &Config) -> Result<PoppedOut, String> {
        let (event_loop, screen) = open_screen(config)?;
        Ok(PoppedOut {
            event_loop,
            screen,
            pacer: Pacer::new(config.timestep),
            last_frame: Instant::now(),
            frame_budget: match config.target_fps {
                0 => Duration::ZERO,
                fps => Duration::from_secs_f64(1.0 / fps as f64),
            },
            scale: config.scale,
            resizable: config.resizable,
        })
    }

    /// Shows the window (again) or hides it. Showing starts the clock
    /// afresh, so the time it was hidden isn't caught up on.
    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.screen.hold_pointer(false);
        if let Some(window) = &self.screen.window {
            window.set_visible(visible);
            if visible {
                window.focus_window();
            }
        }
        self.screen.closed = false;
        self.pacer = Pacer::new(self.pacer.timestep);
        self.last_frame = Instant::now();
    }

    /// One turn of the game loop: read the player's keys and mouse, run
    /// the updates that are due, draw and show. Says how many updates ran,
    /// or `None` once the player has closed the window.
    pub(crate) fn turn<G: Game>(
        &mut self,
        sim: &mut Simulation<G>,
    ) -> Result<Option<u32>, Box<dyn Error>> {
        if !pump(&mut self.event_loop, &mut self.screen, Duration::ZERO) {
            return Ok(None);
        }
        if self.screen.closed {
            return Ok(None);
        }
        let now = Instant::now();
        let frame_time = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        sim.measure_frame(frame_time as f32);
        let (updates, dt) = self.pacer.updates_due(frame_time);

        let (window_w, window_h) = self.screen.logical_size();
        if self.resizable && window_w > 0 && window_h > 0 {
            sim.resize(window_w / self.scale, window_h / self.scale);
        }
        let canvas_size = (
            sim.context().width() as usize,
            sim.context().height() as usize,
        );
        let input = self.screen.take_input(canvas_size);
        sim.hold_keys(&input.keys);
        match input.mouse {
            MouseInput::At(position) => sim.move_mouse(position),
            MouseInput::Moved(amount) => sim.move_mouse_by(amount),
            MouseInput::Nothing => {}
        }
        sim.hold_mouse_buttons(&input.buttons);
        if input.scroll != 0.0 {
            sim.scroll(input.scroll);
        }
        let mut ran = 0;
        for _ in 0..updates {
            sim.step_by(dt);
            ran += 1;
            if sim.quit_requested() {
                break;
            }
        }
        self.screen.hold_pointer(sim.context().pointer_locked());
        let canvas = sim.draw_for_screen();
        self.screen.show(canvas)?;
        if let Some(spare) = self.frame_budget.checked_sub(now.elapsed()) {
            std::thread::sleep(spare);
        }
        Ok(Some(ran))
    }
}

/// Connects to the system's windows. On Linux, winit stops the program
/// (panics) instead of returning an error when a system library is
/// missing, so we catch that and turn it into a message.
fn new_event_loop() -> Result<EventLoop<()>, String> {
    let usual = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // we print our own message
    let made = std::panic::catch_unwind(EventLoop::new);
    std::panic::set_hook(usual);
    match made {
        Ok(Ok(event_loop)) => Ok(event_loop),
        Ok(Err(err)) => Err(err.to_string()),
        Err(panic) => {
            let text = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_else(|| String::from("the window library stopped"));
            let hint = if text.contains("xkbcommon") {
                "; on Ubuntu or Debian, `sudo apt install libxkbcommon-x11-0` adds it"
            } else {
                ""
            };
            Err(format!("{text}{hint}"))
        }
    }
}

/// What the mouse did since last frame, ready for the game.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MouseInput {
    /// The pointer is here, in canvas pixels.
    At(Vec2),
    /// The pointer is locked, and the mouse moved this much, in canvas pixels.
    Moved(Vec2),
    /// Nothing new.
    Nothing,
}

/// Everything the player did since last frame, in our own types.
struct PlayerInput {
    /// Keys held, and keys tapped since last frame even if already let go.
    keys: Vec<Key>,
    mouse: MouseInput,
    buttons: Vec<MouseButton>,
    scroll: f32,
}

/// The window, and what winit has told us about the keyboard and mouse.
///
/// winit reports *events* ("A went down", "the mouse moved 3 pixels").
/// The game wants *state* once per frame ("is A held?"), so `Screen` keeps
/// the state up to date as events arrive, and the game loop reads it with
/// [`take_input`](Screen::take_input).
struct Screen {
    title: String,
    size: LogicalSize<f64>,
    window: Option<Rc<Window>>,
    /// Where our pixels go. It needs the window twice: once to know which
    /// screen (the "display") and once for the window itself.
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    /// Why the window couldn't open.
    problem: Option<String>,
    closed: bool,
    focused: bool,
    keys_held: Vec<Key>,
    /// Pressed since the last frame, so a tap shorter than a frame still counts.
    keys_tapped: Vec<Key>,
    buttons_held: Vec<MouseButton>,
    buttons_tapped: Vec<MouseButton>,
    /// The pointer, in window pixels, once it has been over the window.
    cursor: Option<PhysicalPosition<f64>>,
    cursor_moved: bool,
    /// Mouse movement since last frame straight from the mouse, which keeps
    /// coming when the pointer is stuck at an edge (roughly window pixels).
    motion: (f64, f64),
    scroll: f32,
    /// How the pointer is held: `None` when it's free, `Some(None)` when
    /// it's only hidden and brought back to the middle every frame.
    grab: Option<CursorGrabMode>,
}

impl Screen {
    fn new(title: &str, size: LogicalSize<f64>) -> Self {
        Self {
            title: title.to_string(),
            size,
            window: None,
            surface: None,
            problem: None,
            closed: false,
            focused: true,
            keys_held: Vec::new(),
            keys_tapped: Vec::new(),
            buttons_held: Vec::new(),
            buttons_tapped: Vec::new(),
            cursor: None,
            cursor_moved: false,
            motion: (0.0, 0.0),
            scroll: 0.0,
            grab: None,
        }
    }

    /// The window's inside size in physical pixels (0 while minimized).
    fn physical_size(&self) -> (usize, usize) {
        match &self.window {
            Some(window) => {
                let size = window.inner_size();
                (size.width as usize, size.height as usize)
            }
            None => (0, 0),
        }
    }

    /// The window's inside size in logical pixels: physical pixels divided
    /// by the system's display scaling (150% on many laptops), the units
    /// the window was opened in.
    fn logical_size(&self) -> (usize, usize) {
        match &self.window {
            Some(window) => {
                let size: LogicalSize<f64> = window.inner_size().to_logical(window.scale_factor());
                (size.width as usize, size.height as usize)
            }
            None => (0, 0),
        }
    }

    fn set_title(&self, title: &str) {
        if let Some(window) = &self.window {
            window.set_title(title);
        }
    }

    /// What the player did since last time, with the mouse in canvas pixels.
    fn take_input(&mut self, canvas_size: (usize, usize)) -> PlayerInput {
        let mut keys = self.keys_held.clone();
        keys.extend(
            self.keys_tapped
                .drain(..)
                .filter(|key| !self.keys_held.contains(key)),
        );
        let mut buttons = self.buttons_held.clone();
        buttons.extend(
            self.buttons_tapped
                .drain(..)
                .filter(|button| !self.buttons_held.contains(button)),
        );
        let window_size = self.physical_size();
        let mouse = if self.grab.is_some() {
            match motion_to_canvas(self.motion, window_size, canvas_size) {
                Some(amount) if amount != Vec2::ZERO => MouseInput::Moved(amount),
                _ => MouseInput::Nothing,
            }
        } else {
            match self.cursor {
                Some(cursor) if self.cursor_moved => {
                    window_to_canvas((cursor.x as f32, cursor.y as f32), window_size, canvas_size)
                        .map_or(MouseInput::Nothing, MouseInput::At)
                }
                _ => MouseInput::Nothing,
            }
        };
        self.motion = (0.0, 0.0);
        self.cursor_moved = false;
        PlayerInput {
            keys,
            mouse,
            buttons,
            scroll: std::mem::take(&mut self.scroll),
        }
    }

    /// Locks the pointer (hidden, held in the window) or frees it. It's
    /// only locked while the window is the active one, so Alt+Tab still
    /// gives the pointer back.
    fn hold_pointer(&mut self, wanted: bool) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let wanted = wanted && self.focused;
        if wanted && self.grab.is_none() {
            // `Locked` keeps the pointer in one place (macOS, Wayland). Where
            // it isn't there, the pointer is hidden and put back in the
            // middle of the window every frame (below), so it never rests
            // on an edge. Windows also gets `Confined`, so the pointer can't
            // leave the window between frames. (X11 can confine too, but
            // then it reports every mouse movement twice.)
            let modes: &[CursorGrabMode] = if cfg!(target_os = "linux") {
                &[CursorGrabMode::Locked]
            } else {
                &[CursorGrabMode::Locked, CursorGrabMode::Confined]
            };
            let mode = modes
                .iter()
                .copied()
                .find(|&mode| window.set_cursor_grab(mode).is_ok());
            window.set_cursor_visible(false);
            self.grab = Some(mode.unwrap_or(CursorGrabMode::None));
            self.motion = (0.0, 0.0); // movement from before the lock doesn't count
        } else if !wanted && self.grab.is_some() {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
            self.grab = None;
            self.cursor_moved = false; // the old position is stale: wait for a real one
        }
        if self.grab != Some(CursorGrabMode::Locked) && self.grab.is_some() {
            let size = window.inner_size();
            let middle = PhysicalPosition::new(size.width as f64 / 2.0, size.height as f64 / 2.0);
            let _ = window.set_cursor_position(middle);
        }
    }

    /// Puts the canvas on the screen, stretched to fill the window while
    /// keeping its shape (black bars fill the rest).
    fn show(&mut self, canvas: &Canvas) -> Result<(), Box<dyn Error>> {
        let (width, height) = self.physical_size();
        let (Some(surface), Some(w), Some(h)) = (
            self.surface.as_mut(),
            NonZeroU32::new(width as u32),
            NonZeroU32::new(height as u32),
        ) else {
            return Ok(()); // minimized: nothing to show
        };
        surface.resize(w, h)?;
        let mut buffer = surface.buffer_mut()?;
        stretch(
            canvas.pixels(),
            (canvas.width(), canvas.height()),
            &mut buffer,
            (width, height),
        );
        buffer.present()?;
        Ok(())
    }
}

/// winit calls these as things happen.
impl ApplicationHandler for Screen {
    /// The system is ready for a window (once, at the start).
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(self.size);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(err) => {
                self.problem = Some(err.to_string());
                return;
            }
        };
        let surface = softbuffer::Context::new(window.clone())
            .and_then(|context| softbuffer::Surface::new(&context, window.clone()));
        match surface {
            Ok(surface) => self.surface = Some(surface),
            Err(err) => self.problem = Some(format!("can't draw in it: {err}")),
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.closed = true,
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                if !focused {
                    // Keys let go while another window had the keyboard never
                    // reach us, so forget them all rather than leave one stuck.
                    self.keys_held.clear();
                    self.buttons_held.clear();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(key) = convert_key(code) else {
                    return;
                };
                match event.state {
                    ElementState::Pressed if !self.keys_held.contains(&key) => {
                        self.keys_held.push(key);
                        self.keys_tapped.push(key);
                    }
                    ElementState::Pressed => {} // the keyboard repeating a held key
                    ElementState::Released => self.keys_held.retain(|&held| held != key),
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };
                match state {
                    ElementState::Pressed if !self.buttons_held.contains(&button) => {
                        self.buttons_held.push(button);
                        self.buttons_tapped.push(button);
                    }
                    ElementState::Pressed => {}
                    ElementState::Released => self.buttons_held.retain(|&held| held != button),
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some(position);
                self.cursor_moved = true;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Different systems report different amounts per wheel
                // "click", so `signum` boils it down to the direction: 1 or -1.
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pixels) => pixels.y as f32,
                };
                if y != 0.0 {
                    self.scroll = y.signum();
                }
            }
            _ => {}
        }
    }

    /// Events straight from the mouse, not tied to the pointer. Only these
    /// keep counting when the pointer is locked.
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.motion.0 += delta.0;
            self.motion.1 += delta.1;
        }
    }
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

/// How many window pixels one canvas pixel covers, when the canvas is
/// scaled up as much as fits while keeping its shape. `None` if either has
/// no size.
fn display_scale(window_size: (usize, usize), canvas_size: (usize, usize)) -> Option<f32> {
    let (window_w, window_h) = (window_size.0 as f32, window_size.1 as f32);
    let (canvas_w, canvas_h) = (canvas_size.0 as f32, canvas_size.1 as f32);
    if canvas_w == 0.0 || canvas_h == 0.0 {
        return None; // nothing to point at
    }
    let scale = (window_w / canvas_w).min(window_h / canvas_h);
    (scale > 0.0).then_some(scale) // 0 while minimized
}

/// Converts mouse movement in window pixels into canvas pixels, so moving
/// the mouse across the game area moves `mouse_delta()` by the canvas's
/// width, locked or not.
fn motion_to_canvas(
    motion: (f64, f64),
    window_size: (usize, usize),
    canvas_size: (usize, usize),
) -> Option<Vec2> {
    let scale = display_scale(window_size, canvas_size)?;
    Some(vec2(motion.0 as f32 / scale, motion.1 as f32 / scale))
}

/// Copies the canvas into the window's pixels, scaled up as much as fits
/// while keeping its shape and centered, with black bars around it. Each
/// window pixel takes the color of the canvas pixel under it ("nearest
/// neighbour"), so pixel art stays sharp.
fn stretch(
    canvas: &[u32],
    (canvas_w, canvas_h): (usize, usize),
    window: &mut [u32],
    (window_w, window_h): (usize, usize),
) {
    window.fill(0); // the black bars
    let Some(scale) = display_scale((window_w, window_h), (canvas_w, canvas_h)) else {
        return;
    };
    // The picture's size and where it starts, in window pixels.
    let shown_w = ((canvas_w as f32 * scale).round() as usize).min(window_w);
    let shown_h = ((canvas_h as f32 * scale).round() as usize).min(window_h);
    let left = (window_w - shown_w) / 2;
    let top = (window_h - shown_h) / 2;
    // Which canvas column each window column shows, worked out once.
    let columns: Vec<usize> = (0..shown_w)
        .map(|x| ((x as f32 / scale) as usize).min(canvas_w - 1))
        .collect();
    for y in 0..shown_h {
        let source_row = ((y as f32 / scale) as usize).min(canvas_h - 1) * canvas_w;
        let start = (top + y) * window_w + left;
        let row = &mut window[start..start + shown_w];
        for (pixel, &column) in row.iter_mut().zip(&columns) {
            *pixel = canvas[source_row + column];
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
    let scale = display_scale(window_size, canvas_size)?;
    let offset_x = (window_size.0 as f32 - canvas_size.0 as f32 * scale) / 2.0;
    let offset_y = (window_size.1 as f32 - canvas_size.1 as f32 * scale) / 2.0;
    Some(vec2(
        (mouse.0 - offset_x) / scale,
        (mouse.1 - offset_y) / scale,
    ))
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

/// Maps a key from winit to our own [`Key`], or `None` for keys games can't
/// ask about. winit's `KeyCode` names the key's *place* on the keyboard, so
/// WASD stays under the left hand on a French (AZERTY) keyboard too.
fn convert_key(code: KeyCode) -> Option<Key> {
    use KeyCode as K;

    let key = match code {
        K::KeyA => Key::A,
        K::KeyB => Key::B,
        K::KeyC => Key::C,
        K::KeyD => Key::D,
        K::KeyE => Key::E,
        K::KeyF => Key::F,
        K::KeyG => Key::G,
        K::KeyH => Key::H,
        K::KeyI => Key::I,
        K::KeyJ => Key::J,
        K::KeyK => Key::K,
        K::KeyL => Key::L,
        K::KeyM => Key::M,
        K::KeyN => Key::N,
        K::KeyO => Key::O,
        K::KeyP => Key::P,
        K::KeyQ => Key::Q,
        K::KeyR => Key::R,
        K::KeyS => Key::S,
        K::KeyT => Key::T,
        K::KeyU => Key::U,
        K::KeyV => Key::V,
        K::KeyW => Key::W,
        K::KeyX => Key::X,
        K::KeyY => Key::Y,
        K::KeyZ => Key::Z,
        K::Digit0 => Key::Digit0,
        K::Digit1 => Key::Digit1,
        K::Digit2 => Key::Digit2,
        K::Digit3 => Key::Digit3,
        K::Digit4 => Key::Digit4,
        K::Digit5 => Key::Digit5,
        K::Digit6 => Key::Digit6,
        K::Digit7 => Key::Digit7,
        K::Digit8 => Key::Digit8,
        K::Digit9 => Key::Digit9,
        K::ArrowLeft => Key::Left,
        K::ArrowRight => Key::Right,
        K::ArrowUp => Key::Up,
        K::ArrowDown => Key::Down,
        K::Space => Key::Space,
        K::Enter | K::NumpadEnter => Key::Enter,
        K::Escape => Key::Escape,
        K::Tab => Key::Tab,
        K::Backspace => Key::Backspace,
        K::ShiftLeft => Key::LeftShift,
        K::ShiftRight => Key::RightShift,
        K::ControlLeft => Key::LeftCtrl,
        K::ControlRight => Key::RightCtrl,
        K::AltLeft => Key::LeftAlt,
        K::AltRight => Key::RightAlt,
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
        assert_eq!(convert_key(KeyCode::Space), Some(Key::Space));
        assert_eq!(convert_key(KeyCode::Digit7), Some(Key::Digit7));
        assert_eq!(convert_key(KeyCode::KeyQ), Some(Key::Q));
        assert_eq!(convert_key(KeyCode::ArrowLeft), Some(Key::Left));
        assert_eq!(convert_key(KeyCode::NumLock), None);
        // Every one of our keys can be typed.
        let all = [
            KeyCode::KeyA,
            KeyCode::KeyB,
            KeyCode::KeyC,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyF,
            KeyCode::KeyG,
            KeyCode::KeyH,
            KeyCode::KeyI,
            KeyCode::KeyJ,
            KeyCode::KeyK,
            KeyCode::KeyL,
            KeyCode::KeyM,
            KeyCode::KeyN,
            KeyCode::KeyO,
            KeyCode::KeyP,
            KeyCode::KeyQ,
            KeyCode::KeyR,
            KeyCode::KeyS,
            KeyCode::KeyT,
            KeyCode::KeyU,
            KeyCode::KeyV,
            KeyCode::KeyW,
            KeyCode::KeyX,
            KeyCode::KeyY,
            KeyCode::KeyZ,
            KeyCode::Digit0,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::Space,
            KeyCode::Enter,
            KeyCode::Escape,
            KeyCode::Tab,
            KeyCode::Backspace,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::F1,
            KeyCode::F2,
            KeyCode::F3,
            KeyCode::F4,
            KeyCode::F5,
            KeyCode::F6,
            KeyCode::F7,
            KeyCode::F8,
            KeyCode::F9,
            KeyCode::F10,
            KeyCode::F11,
            KeyCode::F12,
        ];
        let reached: std::collections::HashSet<Key> =
            all.into_iter().filter_map(convert_key).collect();
        assert_eq!(reached.len(), Key::ALL.len());
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
    fn locked_mouse_movement_is_measured_in_canvas_pixels() {
        // 3 window pixels per canvas pixel.
        let moved = motion_to_canvas((30.0, -6.0), (960, 720), (320, 240));
        assert_eq!(moved, Some(vec2(10.0, -2.0)));
        // A minimized window can't say.
        assert_eq!(motion_to_canvas((30.0, -6.0), (0, 0), (320, 240)), None);
    }

    #[test]
    fn the_canvas_is_stretched_to_fill_the_window_keeping_its_shape() {
        // A 2x1 canvas (red, blue) in a 4x4 window: 2x bigger, with a black
        // bar above and below.
        let (red, blue) = (0x00FF_0000, 0x0000_00FF);
        let mut window = vec![0x00AB_CDEF; 16]; // old junk must go
        stretch(&[red, blue], (2, 1), &mut window, (4, 4));
        #[rustfmt::skip]
        let wanted = [
            0, 0, 0, 0,
            red, red, blue, blue,
            red, red, blue, blue,
            0, 0, 0, 0,
        ];
        assert_eq!(window, wanted);

        // Not a whole number of times bigger: every window pixel still
        // shows the canvas pixel under it.
        let mut window = vec![0; 15];
        stretch(&[red, blue], (2, 1), &mut window, (5, 3)); // 2.5x
        assert_eq!(window[..5], [red, red, red, blue, blue]);
        assert_eq!(window[10..], [red, red, red, blue, blue]);

        // A minimized window, or an empty canvas: nothing breaks.
        stretch(&[], (0, 0), &mut window, (5, 1));
        stretch(&[red], (1, 1), &mut [], (0, 0));
    }

    #[test]
    fn a_canvas_with_no_pixels_has_no_mouse_position() {
        assert_eq!(window_to_canvas((5.0, 5.0), (100, 100), (0, 0)), None);
    }
}

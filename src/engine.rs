//! The heart of the engine: the [`Game`] trait, [`Config`], [`Context`] and
//! the game loop in [`run`].
//!
//! This is the only file that talks to the window library (`minifb`).
//! Everything else in the engine is plain Rust, so switching to a different
//! window library would mean rewriting just this file.

use std::error::Error;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use minifb::{MouseMode, ScaleMode, Window, WindowOptions};

use crate::canvas::Canvas;
use crate::color::Color;
use crate::input::{Input, Key, MouseButton};
use crate::math::{Rect, Vec2, vec2};
use crate::render3d::Renderer;
use crate::rng::{self, Rng};
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

/// What every game must provide. The engine calls both methods once per frame.
pub trait Game {
    /// Advance the game by one step: read input, move things, check collisions.
    fn update(&mut self, ctx: &mut Context);

    /// Draw the current state of the game onto the canvas.
    ///
    /// This takes `&self`, not `&mut self`: drawing can look at the game but
    /// never change it, and the compiler enforces that for us.
    fn draw(&self, canvas: &mut Canvas);
}

/// How game time moves forward from one [`Game::update`] to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timestep {
    /// Every update moves the game forward by exactly `1 / updates_per_second`
    /// seconds, so `ctx.dt()` is always the same number.
    ///
    /// The game plays out the same way on a fast computer and a slow one: a
    /// computer that can't draw that many frames per second runs two updates
    /// before some of its frames, instead of giving them a bigger `dt`. With
    /// the same inputs and seed, every run is exactly the same.
    ///
    /// This is the default, with 60 updates per second.
    Fixed { updates_per_second: u32 },
    /// One update per drawn frame, with `ctx.dt()` measured by the clock
    /// (capped at 1/30 of a second). Smoother on screens faster than 60 Hz,
    /// but no two runs are exactly alike, so replays and tests can drift.
    RealTime,
}

impl Default for Timestep {
    fn default() -> Self {
        Timestep::Fixed {
            updates_per_second: 60,
        }
    }
}

/// Settings for the window and the game loop.
#[derive(Debug, Clone)]
pub struct Config {
    /// Text in the window's title bar.
    pub title: String,
    /// Width of the canvas in pixels.
    pub width: usize,
    /// Height of the canvas in pixels.
    pub height: usize,
    /// The window starts `scale` times bigger than the canvas, for a chunky
    /// pixel-art look. The window can also be resized freely.
    pub scale: usize,
    /// How many frames per second the loop aims for.
    pub target_fps: usize,
    /// Show a frames-per-second counter in the corner. F3 toggles it while playing.
    pub show_fps: bool,
    /// What draws 3D: the graphics card if possible (`Auto`), or always the
    /// CPU. The `DUCKFORGE_RENDERER` environment variable overrides this.
    pub renderer: Renderer,
    /// How game time moves forward: in fixed steps (the default, 60 per
    /// second) or by the real clock. See [`Timestep`].
    pub timestep: Timestep,
    /// The seed for `ctx.rng`. With `Some(number)`, the random numbers come
    /// out the same on every run, so the same inputs always give exactly the
    /// same game. With `None` (the default), each run in a window gets a new
    /// seed from the clock.
    pub seed: Option<u32>,
}

impl Config {
    /// Checks that the settings can make a window, with a message saying
    /// what's wrong if not.
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err(format!(
                "the canvas must be at least 1 x 1 pixels, not {} x {}",
                self.width, self.height
            ));
        }
        if self.scale == 0 {
            return Err(String::from("scale must be at least 1"));
        }
        let window_fits = self.width.checked_mul(self.scale).is_some()
            && self.height.checked_mul(self.scale).is_some();
        if !window_fits {
            return Err(String::from("width or height times scale is too big"));
        }
        if self.timestep
            == (Timestep::Fixed {
                updates_per_second: 0,
            })
        {
            return Err(String::from("updates_per_second must be at least 1"));
        }
        Ok(())
    }

    /// The `dt` of one update when there is no clock to measure it, as in a
    /// [`Simulation`]: the fixed step, or for [`Timestep::RealTime`] the
    /// frame time at `target_fps` (60 if that is 0, meaning "no limit").
    pub(crate) fn step_time(&self) -> f32 {
        match self.timestep {
            Timestep::Fixed { updates_per_second } => 1.0 / updates_per_second.max(1) as f32,
            Timestep::RealTime if self.target_fps > 0 => 1.0 / self.target_fps as f32,
            Timestep::RealTime => 1.0 / 60.0,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: String::from("duckforge"),
            width: 320,
            height: 240,
            scale: 3,
            target_fps: 60,
            show_fps: false,
            renderer: Renderer::Auto,
            timestep: Timestep::default(),
            seed: None,
        }
    }
}

/// Everything the engine hands to a game during [`Game::update`].
pub struct Context {
    /// The keyboard and mouse.
    pub input: Input,
    /// A random number generator, seeded from [`Config::seed`], or
    /// differently on every run if that is `None`.
    pub rng: Rng,
    dt: f32,
    /// Seconds since the start, as an `f64`: adding up small `f32` steps
    /// loses precision after an hour or so, and the clock would drift.
    time: f64,
    frame: u64,
    /// Seconds per drawn frame, averaged over recent frames. 0.0 until measured.
    average_frame_time: f32,
    /// How many frame times `measure_frame` has been given.
    frames_measured: u64,
    renderer_name: String,
    width: usize,
    height: usize,
    quit_requested: bool,
}

impl Context {
    pub(crate) fn new(width: usize, height: usize, seed: u32) -> Self {
        Self {
            input: Input::new(),
            rng: Rng::new(seed),
            dt: 0.0,
            time: 0.0,
            frame: 0,
            average_frame_time: 0.0,
            frames_measured: 0,
            renderer_name: String::new(),
            width,
            height,
            quit_requested: false,
        }
    }

    /// Seconds of game time since the previous update ("delta time").
    /// Multiply speeds by this so things move at the same speed no matter
    /// how often the game updates. With the default [`Timestep`], it is
    /// always exactly 1/60.
    pub fn dt(&self) -> f32 {
        self.dt
    }

    /// Seconds of game time since the game started.
    pub fn time(&self) -> f32 {
        self.time as f32
    }

    /// Which update this is, counting from 1: it is 1 during the first
    /// call to [`Game::update`], 2 during the second, and so on.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Frames drawn per second, averaged over the last second or so.
    /// (Without a window, nothing is drawn on a clock, so this is the
    /// number of updates per game second instead.)
    pub fn fps(&self) -> f32 {
        if self.average_frame_time > 0.0 {
            1.0 / self.average_frame_time
        } else {
            0.0
        }
    }

    /// What draws 3D: `"CPU"`, or `"GPU: "` plus the graphics card's name.
    /// Known from the second frame on, because the GPU starts up when the
    /// first mesh is drawn.
    pub fn renderer_name(&self) -> &str {
        &self.renderer_name
    }

    /// Width of the canvas in pixels.
    pub fn width(&self) -> f32 {
        self.width as f32
    }

    /// Height of the canvas in pixels.
    pub fn height(&self) -> f32 {
        self.height as f32
    }

    /// Ask the engine to close the window at the end of this frame.
    pub fn quit(&mut self) {
        self.quit_requested = true;
    }

    pub(crate) fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub(crate) fn set_renderer_name(&mut self, name: &str) {
        if self.renderer_name != name {
            self.renderer_name = name.to_string();
        }
    }

    /// Prepares the context for the next update, which moves the game
    /// forward by `dt` seconds.
    pub(crate) fn begin_frame(&mut self, dt: f32, keys_down: impl IntoIterator<Item = Key>) {
        self.dt = dt;
        self.time += dt as f64;
        self.frame += 1;
        self.input.begin_frame(keys_down);
    }

    /// Feeds the FPS counter how long one drawn frame took, in seconds.
    pub(crate) fn measure_frame(&mut self, frame_time: f32) {
        self.frames_measured += 1;
        // The first frame is timed from just before the loop started, so its
        // time is meaningless (almost zero): skip it.
        if self.frames_measured > 1 && frame_time > 0.0 {
            // A moving average: each frame nudges the value 5% towards the
            // latest measurement, so the number on screen doesn't flicker.
            // We average frame *times*, not frames-per-second values: one
            // unusually quick frame would make a per-second average jump.
            self.average_frame_time = if self.average_frame_time == 0.0 {
                frame_time
            } else {
                self.average_frame_time + (frame_time - self.average_frame_time) * 0.05
            };
        }
    }
}

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

/// Opens a window and runs `game` until the window is closed or the game
/// calls [`Context::quit`].
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
/// Two keys work in every game: **F3** shows or hides an FPS counter, and
/// **F12** saves a screenshot.
pub fn run<G: Game>(mut config: Config, game: G) -> Result<(), Box<dyn Error>> {
    config.check()?;
    // Pick the seed now, so a run without one still knows which it used.
    config.seed = Some(config.seed.unwrap_or_else(rng::seed_from_clock));
    let options = WindowOptions {
        resize: true,
        scale_mode: ScaleMode::AspectRatioStretch,
        ..WindowOptions::default()
    };
    let mut window = Window::new(
        &config.title,
        config.width * config.scale,
        config.height * config.scale,
        options,
    )?;
    // minifb waits inside `update_with_buffer` so we don't run faster than this.
    window.set_target_fps(config.target_fps);

    let renderer = renderer_from_env().unwrap_or(config.renderer);
    let mut sim = Simulation::start(&config, game, renderer);
    let mut pacer = Pacer::new(config.timestep);
    let mut show_fps = config.show_fps;
    let mut keys_last_frame = Vec::new();
    let mut last_frame = Instant::now();

    while window.is_open() && !sim.quit_requested() {
        // 1. Time.
        let now = Instant::now();
        let frame_time = now.duration_since(last_frame).as_secs_f64();
        last_frame = now;
        sim.measure_frame(frame_time as f32);
        let (updates, dt) = pacer.updates_due(frame_time);

        // 2. Input: translate the window library's keys and buttons into our own types.
        let keys: Vec<Key> = window
            .get_keys()
            .into_iter()
            .filter_map(convert_key)
            .collect();
        sim.hold_keys(&keys);
        read_mouse(&window, (config.width, config.height), &mut sim);

        // 3. The game's turn to update, as many times as are due. On a
        // computer that keeps up, that's once.
        for _ in 0..updates {
            sim.step_by(dt);
            if sim.quit_requested() {
                break;
            }
        }

        // 4. The game's turn to draw.
        let fps = sim.context().fps();
        let canvas = sim.draw_for_screen();
        let pressed = |key| keys.contains(&key) && !keys_last_frame.contains(&key);
        if pressed(Key::F3) {
            show_fps = !show_fps;
        }
        if show_fps {
            draw_fps(canvas, fps);
        }
        if pressed(Key::F12) {
            save_screenshot(canvas);
        }

        // 5. Show the frame. This also collects new keyboard and mouse events from the OS.
        window.update_with_buffer(canvas.pixels(), canvas.width(), canvas.height())?;
        keys_last_frame = keys;
    }

    Ok(())
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
    let path = format!("screenshot-{millis}.bmp");

    // A failed screenshot shouldn't crash the game, so we report the error
    // instead of passing it up with `?`.
    match canvas.save_bmp(&path) {
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

    #[test]
    fn context_tracks_time_and_frames() {
        let mut ctx = Context::new(100, 50, 1);
        ctx.begin_frame(0.5, [Key::Space]);
        ctx.begin_frame(0.025, []);
        assert_eq!(ctx.dt(), 0.025);
        assert_eq!(ctx.time(), 0.525);
        assert_eq!(ctx.frame(), 2);
        assert_eq!((ctx.width(), ctx.height()), (100.0, 50.0));
        assert!(ctx.input.was_released(Key::Space));
    }

    #[test]
    fn fps_ignores_the_near_zero_first_frame() {
        let mut ctx = Context::new(100, 50, 1);
        ctx.measure_frame(0.000_000_05); // 50 nanoseconds: "20 million FPS"
        for _ in 0..10 {
            ctx.measure_frame(1.0 / 75.0);
        }
        assert!((ctx.fps() - 75.0).abs() < 0.1, "fps = {}", ctx.fps());
    }

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
    fn time_stays_accurate_over_long_sessions() {
        let mut ctx = Context::new(10, 10, 1);
        let frames = 1_000_000; // about 4.6 hours at 60 FPS
        for _ in 0..frames {
            ctx.begin_frame(1.0 / 60.0, []);
        }
        let expected = frames as f32 / 60.0;
        assert!(
            (ctx.time() - expected).abs() < 0.1,
            "time = {}, expected {expected}",
            ctx.time()
        );
    }

    #[test]
    fn impossible_configs_get_a_clear_error() {
        assert!(Config::default().check().is_ok());
        let no_width = Config {
            width: 0,
            ..Config::default()
        };
        assert!(no_width.check().unwrap_err().contains("at least 1 x 1"));
        let no_scale = Config {
            scale: 0,
            ..Config::default()
        };
        assert!(no_scale.check().is_err());
        let overflowing = Config {
            width: usize::MAX,
            ..Config::default()
        };
        assert!(overflowing.check().is_err());
        let never_updates = Config {
            timestep: Timestep::Fixed {
                updates_per_second: 0,
            },
            ..Config::default()
        };
        assert!(never_updates.check().unwrap_err().contains("at least 1"));
    }

    #[test]
    fn a_canvas_with_no_pixels_has_no_mouse_position() {
        assert_eq!(window_to_canvas((5.0, 5.0), (100, 100), (0, 0)), None);
    }
}

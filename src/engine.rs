//! The heart of the engine: the [`Game`] trait, [`Config`], [`Context`]
//! and [`run`], which starts a game in a window or, with `--headless`,
//! without one.

use std::error::Error;
use std::fmt;

use crate::canvas::Canvas;
use crate::cli::{self, Options};
use crate::headless;
use crate::input::{Input, Key};
use crate::render3d::Renderer;
use crate::report::ReportValue;
use crate::rng::Rng;
use crate::sound::{Sound, SoundRequest, Wav};

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
    /// What the game reported during this update.
    reports: Vec<(String, ReportValue)>,
    /// The sounds the game played during this update.
    sounds: Vec<SoundRequest>,
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
            reports: Vec::new(),
            sounds: Vec::new(),
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

    /// [`time`](Context::time) at full precision.
    pub(crate) fn precise_time(&self) -> f64 {
        self.time
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

    /// Shares a value about the game, like `ctx.report("score", self.score)`.
    ///
    /// Tests and headless runs can then check what happened without looking
    /// at pixels: a headless run saves every change in `log.json`, and
    /// [`Simulation::reported`](crate::Simulation::reported) gives the latest
    /// value. In a window, reports are thrown away, so it's fine to report
    /// every frame. Values can be numbers, `true`/`false` or text:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # fn example(ctx: &mut Context) {
    /// ctx.report("score", 120);
    /// ctx.report("game_over", false);
    /// ctx.report("state", "Playing");
    /// # }
    /// ```
    pub fn report(&mut self, name: &str, value: impl Into<ReportValue>) {
        self.reports.push((name.to_string(), value.into()));
    }

    /// Plays a sound from the built-in synthesizer: a ready-made one like
    /// `Sound::coin()`, or your own:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # fn example(ctx: &mut Context) {
    /// ctx.play(Sound::coin());
    /// ctx.play(Sound { name: "zap", wave: Wave::Triangle, freq: 900.0, slide_to: 300.0, ..Default::default() });
    /// # }
    /// ```
    ///
    /// In a window it comes out of the speakers. Without a window it's
    /// mixed into `audio.wav` at this frame and listed in `log.json`.
    pub fn play(&mut self, sound: Sound) {
        self.sounds.push(SoundRequest::synth(sound));
    }

    /// Plays a plain square-wave beep: `freq` in Hz (440 is the note A),
    /// lasting `ms` milliseconds. Its name in logs is `"beep"`.
    pub fn beep(&mut self, freq: f32, ms: f32) {
        self.play(Sound {
            name: "beep",
            freq,
            ms,
            ..Sound::default()
        });
    }

    /// Plays a sound loaded from a `.wav` file with
    /// [`Wav::load`](crate::sound::Wav::load). Its name in logs is the
    /// file name without `.wav`.
    pub fn play_wav(&mut self, wav: &Wav) {
        self.sounds.push(SoundRequest::wav(wav));
    }

    /// Hands over this update's sounds and starts a fresh list.
    pub(crate) fn take_sounds(&mut self) -> Vec<SoundRequest> {
        std::mem::take(&mut self.sounds)
    }

    /// Hands over this update's reports and starts a fresh list.
    pub(crate) fn take_reports(&mut self) -> Vec<(String, ReportValue)> {
        std::mem::take(&mut self.reports)
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

/// Runs `game`: in a window, or without one if the command line says
/// `--headless`.
///
/// Every game understands the same command-line options, with no code of
/// its own. Run any game with `--help` to see them all, for example
/// `cargo run --example breakout -- --help`. The main ones:
///
/// - `--headless` runs the game with no window, drawing with the CPU, and
///   saves what it drew (see [`run_headless`](crate::run_headless)).
/// - `--frames N` stops after N updates (600 by default with `--headless`).
/// - `--out DIR` is where `--headless` saves its files.
/// - `--seed N` sets the random seed.
///
/// In a window, the game runs until the window is closed or the game calls
/// [`Context::quit`]. Two keys work in every game: **F3** shows or hides an
/// FPS counter, and **F12** saves a screenshot.
///
/// The engine can be built without the window library (with
/// `--no-default-features`), for machines with no screen. Then games can
/// only run with `--headless`, and `run` says so if you forget it.
pub fn run<G: Game>(config: Config, game: G) -> Result<(), Box<dyn Error>> {
    let options = Options::from_command_line().map_err(EngineError)?;
    if options.help {
        print!("{}", cli::help());
        return Ok(());
    }
    if options.headless {
        headless::run_from_command_line(config, game, &options).map_err(EngineError)?;
        return Ok(());
    }
    run_window(config, game, &options)
}

#[cfg(feature = "window")]
fn run_window<G: Game>(config: Config, game: G, options: &Options) -> Result<(), Box<dyn Error>> {
    crate::window::run(config, game, options)
}

#[cfg(not(feature = "window"))]
fn run_window<G: Game>(_: Config, _: G, _: &Options) -> Result<(), Box<dyn Error>> {
    Err(EngineError(String::from(
        "this copy of duckforge was built without a window (the \"window\" feature is off), \
         so games can only run with --headless. For example:\n  \
         cargo run --no-default-features --example breakout -- --headless",
    ))
    .into())
}

/// An error message from the engine.
///
/// When `main` returns an error, Rust prints it with `{:?}` (the `Debug`
/// format). For a plain `String` that adds quotes and turns line breaks
/// into `\n`, so this type's `Debug` prints the message as it is.
pub(crate) struct EngineError(pub(crate) String);

impl fmt::Debug for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for EngineError {}

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
}

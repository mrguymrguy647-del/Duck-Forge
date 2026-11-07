//! The heart of the engine: the [`Game`] trait, [`Config`], [`Context`]
//! and [`run`], which starts a game in a window or, with `--headless`,
//! without one.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use crate::canvas::Canvas;
use crate::cli::{self, Options};
use crate::debug::DebugShape;
use crate::headless;
use crate::input::{Input, Key};
use crate::math::{Rect, Vec2};
use crate::render3d::Renderer;
use crate::report::ReportValue;
use crate::rng::Rng;
use crate::sound::{LoopCommand, LoopId, LoopSound, Sound, SoundRequest, Wav};
use crate::tune::Tunes;

/// What every game must provide. The engine calls `update` and `draw` once
/// per frame.
///
/// `init` and `resize` are optional: they have empty bodies here (*default
/// methods*), so a game only writes them if it needs them.
pub trait Game {
    /// Runs once, just before the first `update`. Use it for setup that
    /// needs the context, like reading `ctx.width()`, picking random
    /// starting positions with `ctx.rng`, or playing a start-up sound.
    /// Anything it reports or plays shows up as frame 1.
    fn init(&mut self, ctx: &mut Context) {
        let _ = ctx; // nothing to do unless the game says otherwise
    }

    /// Runs when the canvas changes size, with its new `width` and
    /// `height` in pixels (also in `ctx.width()` and `ctx.height()`).
    ///
    /// That only happens with [`Config::resizable`] set, when the player
    /// resizes the window, or in a test with
    /// [`Simulation::resize`](crate::Simulation::resize). Use it to move
    /// things that are placed relative to the edges, like the score.
    fn resize(&mut self, ctx: &mut Context, width: f32, height: f32) {
        let _ = (ctx, width, height);
    }

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
    /// With `true`, the canvas grows and shrinks with the window (each
    /// canvas pixel stays `scale` window pixels big), and the game's
    /// [`Game::resize`] is told the new size. With `false` (the default),
    /// the canvas always stays `width` x `height`, and a bigger window
    /// just shows it bigger. Headless runs never resize by themselves.
    pub resizable: bool,
    /// Which corner of the screen the debug overlay's info box sits in
    /// (F1 in a window, `--debug` headless). Bottom-right by default,
    /// where games rarely put their score. `--debug-corner` overrides it.
    pub debug_corner: Corner,
    /// The file `ctx.tune` reads, like `"examples/mygame.tune.json"`.
    /// `None` (the default) means `examples/<program name>.tune.json`.
    /// Naming it means every run of the game (a window, `--headless`, a
    /// bot or play test with another program name) reads the same values.
    /// It's fine for it not to exist yet.
    pub tune_file: Option<std::path::PathBuf>,
}

/// A corner of the screen, for [`Config::debug_corner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    /// The default: most games put their score and lives along the top,
    /// and the F3 FPS counter sits bottom-left.
    #[default]
    BottomRight,
}

impl Corner {
    /// Reads `top-left`, `top-right`, `bottom-left` or `bottom-right`.
    pub(crate) fn parse(text: &str) -> Result<Corner, String> {
        match text.to_ascii_lowercase().replace('_', "-").as_str() {
            "top-left" => Ok(Corner::TopLeft),
            "top-right" => Ok(Corner::TopRight),
            "bottom-left" => Ok(Corner::BottomLeft),
            "bottom-right" => Ok(Corner::BottomRight),
            _ => Err(format!(
                "--debug-corner must be top-left, top-right, bottom-left or bottom-right, not {text:?}"
            )),
        }
    }
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
            resizable: false,
            debug_corner: Corner::default(),
            tune_file: None,
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
    /// Whether the game wants the mouse pointer hidden and held in the window.
    pointer_locked: bool,
    /// What the game reported during this update.
    reports: Vec<(String, ReportValue)>,
    /// The sounds the game played during this update.
    sounds: Vec<SoundRequest>,
    /// What the game asked its looping sounds to do during this update.
    loop_commands: Vec<LoopCommand>,
    /// The number of the next loop started.
    next_loop: u32,
    /// Shapes to outline in the debug overlay, from the latest update.
    debug_shapes: Vec<DebugShape>,
    /// Tuned numbers: from `--set`, the tune file, or the game's defaults.
    tunes: Tunes,
    /// The names of every rule checked with `ctx.check`.
    checked: HashSet<String>,
    /// The rules that failed, each with the first frame it failed on.
    failed_checks: Vec<FailedCheck>,
    /// Whether the game said waiting is fine during this update.
    idle_ok: bool,
}

/// A rule from [`Context::check`] that didn't hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedCheck {
    /// The rule's name, as given to `ctx.check`.
    pub name: String,
    /// The first frame it failed on.
    pub frame: u64,
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
            pointer_locked: false,
            reports: Vec::new(),
            sounds: Vec::new(),
            loop_commands: Vec::new(),
            next_loop: 1,
            debug_shapes: Vec::new(),
            tunes: Tunes::default(),
            checked: HashSet::new(),
            failed_checks: Vec::new(),
            idle_ok: false,
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

    /// The canvas changed size.
    pub(crate) fn set_size(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
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
    /// On the CPU (always, headless) it's known from the first update;
    /// with the GPU, from the second frame on, because the GPU starts up
    /// when the first mesh is drawn.
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

    /// Hides the mouse pointer and holds it inside the window (`true`), or
    /// gives it back (`false`). This is *pointer lock*, for first-person
    /// mouse look: while it's on, the pointer can't hit the edge of the
    /// window or the screen, so read the movement with
    /// [`input.mouse_delta()`](Input::mouse_delta), which keeps counting
    /// however far the mouse goes. `mouse_position()` stays where it was.
    ///
    /// The window lets go of the pointer while it isn't the active window
    /// (say, after Alt+Tab) and takes it again when the player comes back.
    /// Give the player a way out, like Escape to unlock and a click to lock
    /// again. Without a window (`--headless`, tests) nothing is hidden, but
    /// [`pointer_locked`](Context::pointer_locked) still says what the game asked for.
    pub fn lock_pointer(&mut self, locked: bool) {
        self.pointer_locked = locked;
    }

    /// Whether the game asked for the pointer to be locked, with
    /// [`lock_pointer`](Context::lock_pointer).
    pub fn pointer_locked(&self) -> bool {
        self.pointer_locked
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

    /// A number the game can tune without being rebuilt: the value of
    /// `name` from `--set name=VALUE`, else from the tune file
    /// (`examples/<game>.tune.json`, or `--tune FILE`), else `default`.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # fn example(ctx: &mut Context) {
    /// let jump_speed = ctx.tune("jump_speed", 330.0);
    /// # }
    /// ```
    ///
    /// Call it where the number is used, every update: it's cheap, and
    /// then a window picks up edits to the tune file while the game runs.
    /// `log.json` lists every value used and where it came from, and
    /// `--write-tune` writes all the names the game asked for to the file.
    pub fn tune(&self, name: &str, default: f32) -> f32 {
        self.tunes.get(name, default)
    }

    /// States a rule that must always hold, like "the player is inside the
    /// level". Call it in `update`, every frame:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # fn example(ctx: &mut Context, player: Rect) {
    /// let level = Rect::new(0.0, 0.0, 960.0, 240.0);
    /// ctx.check("player inside level", level.contains(player.center()));
    /// ctx.check("lives never negative", true);
    /// # }
    /// ```
    ///
    /// Nothing happens while it holds. The first frame it doesn't, the
    /// engine records the name and frame (once per name): the run's
    /// summary and `log.json` list it, the debug overlay shows it, a
    /// window prints it, a [`PlayTest`](crate::PlayTest) fails, and
    /// `--fuzz` reports the seed and inputs that broke it. It never stops
    /// the game.
    pub fn check(&mut self, name: &str, condition: bool) {
        if !self.checked.contains(name) {
            self.checked.insert(name.to_string());
        }
        if !condition && !self.failed_checks.iter().any(|f| f.name == name) {
            self.failed_checks.push(FailedCheck {
                name: name.to_string(),
                frame: self.frame,
            });
        }
    }

    /// Says that nothing happening right now is expected: the game is on a
    /// title screen, paused, or waiting for "press a key". Call it in
    /// `update`, on every frame it's true:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # fn example(ctx: &mut Context, paused: bool) {
    /// if paused {
    ///     ctx.idle_ok();
    /// }
    /// # }
    /// ```
    ///
    /// `--fuzz` then doesn't call those frames a freeze. (`log.json` lists
    /// them under `"idle"`.)
    pub fn idle_ok(&mut self) {
        self.idle_ok = true;
    }

    pub(crate) fn was_idle_ok(&self) -> bool {
        self.idle_ok
    }

    /// Every rule that failed so far, in the order they first failed.
    pub fn failed_checks(&self) -> &[FailedCheck] {
        &self.failed_checks
    }

    /// How many different rules the game has checked.
    pub(crate) fn checks_seen(&self) -> usize {
        self.checked.len()
    }

    pub(crate) fn tunes(&self) -> &Tunes {
        &self.tunes
    }

    pub(crate) fn tunes_mut(&mut self) -> &mut Tunes {
        &mut self.tunes
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

    /// Starts a sound that repeats with no gap until you stop it: music, an
    /// engine's hum, rain. `sound` is a `.wav` file (`&music`, loaded once
    /// with [`Wav::load`](crate::sound::Wav::load); a Duck Echo loop
    /// repeats seamlessly) or a synthesizer [`Sound`]. `volume` is 0.0
    /// (silent) to 1.0 (as loud as the file), and more to make it louder.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # fn example(ctx: &mut Context, music: &Wav) {
    /// let theme = ctx.play_loop(music, 0.6); // keep the LoopId
    /// ctx.set_loop_volume(theme, 0.3);       // quieter during a pause
    /// ctx.stop_loop(theme);
    /// let hum = ctx.play_loop(Sound { name: "hum", wave: Wave::Sine, freq: 80.0, ms: 500.0, ..Default::default() }, 0.2);
    /// # let _ = hum;
    /// # }
    /// ```
    ///
    /// In a window it comes out of the speakers. Without one it's mixed
    /// into `audio.wav` from this frame on, and `log.json` lists it under
    /// `"loops"` with when it started and stopped and its volumes. Play
    /// tests read it with [`loops`](crate::Simulation::loops).
    pub fn play_loop(&mut self, sound: impl Into<LoopSound>, volume: f32) -> LoopId {
        let id = LoopId(self.next_loop);
        self.next_loop += 1;
        let LoopSound(request) = sound.into();
        self.loop_commands
            .push(LoopCommand::Start(id, request, clean_volume(volume)));
        id
    }

    /// Changes a looping sound's volume from this frame on (0.0 is silent).
    /// For a fade, change it a little every frame. A loop that has stopped
    /// is left alone.
    pub fn set_loop_volume(&mut self, id: LoopId, volume: f32) {
        self.loop_commands
            .push(LoopCommand::Volume(id, clean_volume(volume)));
    }

    /// Stops a looping sound. Stopping one that has already stopped does
    /// nothing.
    pub fn stop_loop(&mut self, id: LoopId) {
        self.loop_commands.push(LoopCommand::Stop(id));
    }

    /// Hands over this update's loop changes and starts a fresh list.
    pub(crate) fn take_loop_commands(&mut self) -> Vec<LoopCommand> {
        std::mem::take(&mut self.loop_commands)
    }

    /// Outlines `rect` in the debug overlay (F1 in a window, `--debug` on
    /// the command line), in world coordinates if the game draws through a
    /// camera. Call it in `update`, every frame you want to see it: a
    /// hitbox, a trigger zone, where an enemy is aiming. When the overlay
    /// is off, it costs next to nothing.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # fn example(ctx: &mut Context, player: Rect) {
    /// ctx.debug_rect(player);
    /// # }
    /// ```
    pub fn debug_rect(&mut self, rect: Rect) {
        self.debug_shapes.push(DebugShape::Rect(rect));
    }

    /// Outlines a circle in the debug overlay, like
    /// [`debug_rect`](Context::debug_rect).
    pub fn debug_circle(&mut self, center: Vec2, radius: f32) {
        self.debug_shapes.push(DebugShape::Circle(center, radius));
    }

    pub(crate) fn debug_shapes(&self) -> &[DebugShape] {
        &self.debug_shapes
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
        self.debug_shapes.clear(); // only the latest update's shapes are shown
        self.idle_ok = false;
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
/// [`Context::quit`]. These keys work in every game: **F1** shows or hides
/// the debug overlay, **F3** an FPS counter, **F10** starts and stops
/// recording, and **F12** saves a screenshot.
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
    if options.fuzz.is_some() {
        // Fuzzing runs the game headless, many times, in separate runs.
        crate::fuzz::run(&config, &options).map_err(EngineError)?;
        return Ok(());
    }
    if let Some((old, new)) = &options.diff {
        // Comparing two runs needs no game: it's done before starting one.
        println!("{}", crate::diff::diff_runs(old, new).map_err(EngineError)?);
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

/// A loop's volume: at least 0, and 0 if it isn't a number.
fn clean_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.max(0.0)
    } else {
        0.0
    }
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

//! Running a game one update at a time, with no window and no clock.
//!
//! The window's game loop in [`run`](crate::run) is built on [`Simulation`]
//! too: it only adds a clock (to decide how many updates are due) and a
//! screen (to show the pictures). So a game behaves exactly the same in a
//! window and in a test.

use crate::canvas::Canvas;
use crate::engine::{Config, Context, Game, Timestep};
use crate::input::{Key, MouseButton};
use crate::math::Vec2;
use crate::render3d::Renderer;
use crate::report::{FrameReport, ReportLog, ReportValue};
use crate::script::{InputAction, InputEvent, InputScript};
use crate::sound::{PlayedSound, SAMPLE_RATE};

/// Keys the engine itself uses in a window (FPS counter, recording,
/// screenshots). They still reach the game, but aren't recorded, so a replay
/// doesn't start recording or take screenshots.
const ENGINE_KEYS: [Key; 4] = [Key::F1, Key::F3, Key::F10, Key::F12];

/// The seed a [`Simulation`] uses when [`Config::seed`] is `None`.
///
/// It's always the same, so running the same test twice gives the same
/// result. Set `Config::seed` to try a different one.
pub const DEFAULT_SEED: u32 = 1;

/// A game running without a window, moved forward one update at a time by you.
///
/// It's for tests and tools: press keys, step, then look at the game or at
/// the picture it draws. Every step moves the game forward by the fixed
/// timestep (1/60 of a second by default), never by the clock, and the
/// random numbers come from [`Config::seed`] (or [`DEFAULT_SEED`]). So a
/// simulation given the same inputs always plays out exactly the same way.
///
/// ```
/// use duckforge::prelude::*;
///
/// struct Counter {
///     presses: u32,
/// }
///
/// impl Game for Counter {
///     fn update(&mut self, ctx: &mut Context) {
///         if ctx.input.was_pressed(Key::Space) {
///             self.presses += 1;
///         }
///     }
///
///     fn draw(&self, canvas: &mut Canvas) {
///         canvas.clear(Color::BLACK);
///     }
/// }
///
/// let mut sim = Simulation::new(Config::default(), Counter { presses: 0 });
/// sim.press(Key::Space);
/// sim.step(); // the game sees Space go down
/// sim.release(Key::Space);
/// sim.run(10);
/// assert_eq!(sim.game().presses, 1);
/// assert_eq!(sim.context().frame(), 11);
/// ```
pub struct Simulation<G> {
    game: G,
    ctx: Context,
    canvas: Canvas,
    /// Seconds of game time per step.
    dt: f32,
    seed: u32,
    /// The keys held right now, in the order they were pressed.
    keys_down: Vec<Key>,
    /// `None` until the mouse is first moved.
    mouse_position: Option<Vec2>,
    mouse_buttons_down: Vec<MouseButton>,
    /// Scrolling waiting for the next step.
    scroll: f32,
    /// The frame the canvas shows a picture of, if it holds a clean one.
    drawn_frame: Option<u64>,
    /// `Some` with a fixed timestep: how many updates per second.
    updates_per_second: Option<u32>,
    /// Every input change so far, for [`recording`](Simulation::recording).
    history: Vec<InputEvent>,
    /// A script being replayed, and how many of its events are done.
    script: Option<(InputScript, usize)>,
    /// What the game has reported with `ctx.report`.
    reports: ReportLog,
    /// Every sound the game played (in a window, only those not yet sent
    /// to the speakers).
    sounds: Vec<PlayedSound>,
    /// False in a window: sounds are forgotten once they've been played.
    keep_sounds: bool,
    /// How many of `sounds` have been handed to the speakers.
    sounds_sent: usize,
    /// Whether the game's `init` has run.
    initialized: bool,
    /// What draws 3D, for a new canvas after a resize.
    renderer: Renderer,
    /// Draw the debug overlay on every picture.
    debug_overlay: bool,
}

impl<G: Game> Simulation<G> {
    /// Starts `game` with the settings in `config`. Nothing has run yet:
    /// call [`step`](Simulation::step) or [`run`](Simulation::run).
    ///
    /// 3D is always drawn on the CPU, so the pictures are the same on every
    /// computer and no graphics card is needed.
    ///
    /// # Panics
    ///
    /// If the config is impossible, like a canvas 0 pixels wide.
    pub fn new(config: Config, game: G) -> Self {
        Self::start(&config, game, Renderer::Cpu)
    }

    /// Like [`new`](Simulation::new), with a choice of 3D renderer (the window uses this).
    pub(crate) fn start(config: &Config, game: G, renderer: Renderer) -> Self {
        if let Err(message) = config.check() {
            panic!("bad Config: {message}");
        }
        let seed = config.seed.unwrap_or(DEFAULT_SEED);
        let mut canvas = Canvas::new(config.width, config.height);
        canvas.set_renderer(renderer);
        Self {
            game,
            ctx: Context::new(config.width, config.height, seed),
            canvas,
            dt: config.step_time(),
            seed,
            keys_down: Vec::new(),
            mouse_position: None,
            mouse_buttons_down: Vec::new(),
            scroll: 0.0,
            drawn_frame: None,
            updates_per_second: match config.timestep {
                Timestep::Fixed { updates_per_second } => Some(updates_per_second),
                Timestep::RealTime => None,
            },
            history: Vec::new(),
            script: None,
            reports: ReportLog {
                keep_changes: true,
                ..ReportLog::default()
            },
            sounds: Vec::new(),
            keep_sounds: true,
            sounds_sent: 0,
            initialized: false,
            renderer,
            debug_overlay: false,
        }
    }

    /// Runs one update: the game's `update` is called once, and sees the
    /// keys and buttons held right now.
    pub fn step(&mut self) {
        // Without a clock, the FPS counter shows the update rate.
        self.ctx.measure_frame(self.dt);
        self.step_by(self.dt);
    }

    /// Runs `frames` updates, or fewer if the game calls [`Context::quit`].
    pub fn run(&mut self, frames: u64) {
        for _ in 0..frames {
            if self.quit_requested() {
                break;
            }
            self.step();
        }
    }

    /// One update that moves the game forward by `dt` seconds.
    pub(crate) fn step_by(&mut self, dt: f32) {
        if !self.initialized {
            self.initialized = true;
            self.game.init(&mut self.ctx);
        }
        self.apply_script_events();
        self.ctx.begin_frame(dt, self.keys_down.iter().copied());
        let buttons = self.mouse_buttons_down.iter().copied();
        self.ctx
            .input
            .update_mouse(self.mouse_position, buttons, self.scroll);
        self.scroll = 0.0;
        self.game.update(&mut self.ctx);
        let reported = self.ctx.take_reports();
        self.reports.record(self.ctx.frame(), reported);
        let start = self.sample_at(self.ctx.frame());
        for request in self.ctx.take_sounds() {
            self.sounds.push(PlayedSound {
                frame: self.ctx.frame(),
                name: request.name,
                source: request.source,
                samples: request.samples,
                start,
            });
        }
    }

    /// The sample in `audio.wav` where update `frame` begins. Update 1
    /// begins at sample 0; with 60 updates per second each update is 735
    /// samples (44100 / 60) after the one before.
    pub(crate) fn sample_at(&self, frame: u64) -> u64 {
        let before = frame.saturating_sub(1);
        match self.updates_per_second {
            // Whole-number math, so it never drifts, even after hours.
            Some(ups) => before * SAMPLE_RATE as u64 / ups.max(1) as u64,
            None => {
                let seconds = if frame == self.ctx.frame() {
                    self.ctx.precise_time() - self.ctx.dt() as f64
                } else {
                    before as f64 * self.dt as f64
                };
                (seconds.max(0.0) * SAMPLE_RATE as f64).round() as u64
            }
        }
    }

    // ---- Input. Each change is seen by the game at the next step. ----

    /// Holds a key down until [`release`](Simulation::release). The next
    /// step sees it as just pressed.
    ///
    /// To tap a key, press it, step, then release it: pressing and releasing
    /// between the same two steps means the game never sees it.
    pub fn press(&mut self, key: Key) {
        self.apply(InputAction::Press(key));
    }

    /// Lets go of a key.
    pub fn release(&mut self, key: Key) {
        self.apply(InputAction::Release(key));
    }

    /// Moves the mouse pointer to `position`, in canvas pixels.
    pub fn move_mouse(&mut self, position: Vec2) {
        self.apply(InputAction::MouseMove(position));
    }

    /// Holds a mouse button down until [`release_mouse`](Simulation::release_mouse).
    pub fn press_mouse(&mut self, button: MouseButton) {
        self.apply(InputAction::MousePress(button));
    }

    /// Lets go of a mouse button.
    pub fn release_mouse(&mut self, button: MouseButton) {
        self.apply(InputAction::MouseRelease(button));
    }

    /// Rolls the scroll wheel during the next step: up (away from you) for
    /// a positive `amount`, down for a negative one.
    pub fn scroll(&mut self, amount: f32) {
        self.apply(InputAction::Scroll(amount));
    }

    /// Does one input action, seen by the game at the next step. All the
    /// methods above come here, and so does a replayed script.
    ///
    /// Actions that change something are remembered, for
    /// [`recording`](Simulation::recording). Pressing a key that's already
    /// held changes nothing, so it isn't.
    pub fn apply(&mut self, action: InputAction) {
        let changed = match action {
            InputAction::Press(key) => {
                let new = !self.keys_down.contains(&key);
                if new {
                    self.keys_down.push(key);
                }
                new
            }
            InputAction::Release(key) => {
                let held = self.keys_down.contains(&key);
                self.keys_down.retain(|&down| down != key);
                held
            }
            InputAction::MouseMove(position) => {
                let moved = self.mouse_position != Some(position);
                self.mouse_position = Some(position);
                moved
            }
            InputAction::MousePress(button) => {
                let new = !self.mouse_buttons_down.contains(&button);
                if new {
                    self.mouse_buttons_down.push(button);
                }
                new
            }
            InputAction::MouseRelease(button) => {
                let held = self.mouse_buttons_down.contains(&button);
                self.mouse_buttons_down.retain(|&down| down != button);
                held
            }
            InputAction::Scroll(amount) => {
                self.scroll = (self.scroll + amount).signum();
                amount != 0.0
            }
        };
        let engine_key = match action {
            InputAction::Press(key) | InputAction::Release(key) => ENGINE_KEYS.contains(&key),
            _ => false,
        };
        if changed && !engine_key {
            self.history.push(InputEvent {
                frame: self.ctx.frame() + 1, // the frame that will see it
                action,
            });
        }
    }

    // ---- Recording and replaying ----

    /// Replays `script`: from now on, each step first does the script's
    /// events for that frame, exactly as if a player did them.
    ///
    /// Frame numbers in the script count from the start of the game, so
    /// follow a script before the first step. (For the same game to unfold,
    /// the seed must match too: use the script's `seed` in the config.)
    pub fn follow(&mut self, script: InputScript) {
        self.script = Some((script, 0));
    }

    /// Is a script being replayed, with events or frames still to come?
    pub fn is_replaying(&self) -> bool {
        match &self.script {
            Some((script, done)) => {
                *done < script.events.len() || self.ctx.frame() < script.frames.unwrap_or(0)
            }
            None => false,
        }
    }

    /// Everything done to the game so far, as a script that replays it:
    /// every input change since the first frame, with the seed and the
    /// number of frames.
    pub fn recording(&self) -> InputScript {
        InputScript {
            seed: Some(self.seed),
            updates_per_second: self.updates_per_second,
            frames: Some(self.ctx.frame()),
            events: self.history.clone(),
        }
    }

    /// Does the script's events that are due by the coming frame.
    fn apply_script_events(&mut self) {
        let Some((script, done)) = &mut self.script else {
            return;
        };
        let coming_frame = self.ctx.frame() + 1;
        let mut due = Vec::new();
        while let Some(event) = script.events.get(*done) {
            if event.frame > coming_frame {
                break;
            }
            due.push(event.action);
            *done += 1;
        }
        for action in due {
            self.apply(action);
        }
    }

    /// Makes exactly `keys` the held keys: presses the new ones and releases the rest.
    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window uses it
    pub(crate) fn hold_keys(&mut self, keys: &[Key]) {
        for key in self.keys_down.clone() {
            if !keys.contains(&key) {
                self.release(key);
            }
        }
        for &key in keys {
            self.press(key);
        }
    }

    /// Makes exactly `buttons` the held mouse buttons.
    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window uses it
    pub(crate) fn hold_mouse_buttons(&mut self, buttons: &[MouseButton]) {
        for button in self.mouse_buttons_down.clone() {
            if !buttons.contains(&button) {
                self.release_mouse(button);
            }
        }
        for &button in buttons {
            self.press_mouse(button);
        }
    }

    // ---- Looking at the game ----

    /// The game, to check its state in a test.
    pub fn game(&self) -> &G {
        &self.game
    }

    /// The game, to change it.
    pub fn game_mut(&mut self) -> &mut G {
        &mut self.game
    }

    /// Ends the simulation and hands back the game.
    pub fn into_game(self) -> G {
        self.game
    }

    /// The context the game sees: [`frame`](Context::frame) says how many
    /// updates have run, [`time`](Context::time) how much game time has passed.
    pub fn context(&self) -> &Context {
        &self.ctx
    }

    /// The seed the random numbers came from.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// The latest value the game reported under `name` with
    /// [`Context::report`], or `None` if it never did.
    pub fn reported(&self, name: &str) -> Option<&ReportValue> {
        self.reports.latest.get(name)
    }

    /// The latest number the game reported as `name`, for checking in a test:
    /// `assert!(sim.number("score") >= 10.0)`.
    ///
    /// # Panics
    ///
    /// If the game never reported `name`, or it isn't a number. The message
    /// lists what the game did report.
    #[track_caller]
    pub fn number(&self, name: &str) -> f64 {
        let value = self.expect_reported(name);
        value
            .as_number()
            .unwrap_or_else(|| panic!("{name:?} is {value}, not a number"))
    }

    /// The latest yes/no the game reported as `name`. Panics like
    /// [`number`](Simulation::number).
    #[track_caller]
    pub fn flag(&self, name: &str) -> bool {
        let value = self.expect_reported(name);
        value
            .as_bool()
            .unwrap_or_else(|| panic!("{name:?} is {value}, not true or false"))
    }

    /// The latest text the game reported as `name`. Panics like
    /// [`number`](Simulation::number).
    #[track_caller]
    pub fn text(&self, name: &str) -> String {
        let value = self.expect_reported(name);
        match value.as_text() {
            Some(text) => text.to_string(),
            None => panic!("{name:?} is {value}, not text"),
        }
    }

    #[track_caller]
    fn expect_reported(&self, name: &str) -> &ReportValue {
        self.reported(name).unwrap_or_else(|| {
            let names: Vec<&str> = self.reports().map(|(name, _)| name).collect();
            if names.is_empty() {
                panic!(
                    "the game never reported {name:?}, or anything else \
                     (add ctx.report({name:?}, ...) to its update)"
                )
            } else {
                panic!(
                    "the game never reported {name:?}; it reported: {}",
                    names.join(", ")
                )
            }
        })
    }

    /// The value `name` had at the end of `frame`.
    pub fn reported_at(&self, frame: u64, name: &str) -> Option<&ReportValue> {
        self.reports.value_at(frame, name)
    }

    /// Every reported value: the name and its latest value, in alphabetical order.
    pub fn reports(&self) -> impl Iterator<Item = (&str, &ReportValue)> {
        self.reports
            .latest
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    /// Every value `name` had, with the frame it changed on, oldest first.
    /// The first entry is the value it started with (usually on frame 1).
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # struct Game2;
    /// # impl Game for Game2 {
    /// #     fn update(&mut self, ctx: &mut Context) {
    /// #         ctx.report("lives", if ctx.frame() < 100 { 3 } else { 2 });
    /// #     }
    /// #     fn draw(&self, _: &mut Canvas) {}
    /// # }
    /// # let mut sim = Simulation::new(Config::default(), Game2);
    /// sim.run(200);
    /// let lives = sim.history("lives");
    /// assert_eq!(lives[0], (1, &ReportValue::Number(3.0)));
    /// assert_eq!(lives[1], (100, &ReportValue::Number(2.0))); // lost a life on frame 100
    /// ```
    pub fn history(&self, name: &str) -> Vec<(u64, &ReportValue)> {
        self.reports
            .changes
            .iter()
            .filter_map(|report| report.changes.get(name).map(|value| (report.frame, value)))
            .collect()
    }

    /// When each reported value changed: one entry per frame where
    /// something changed, oldest first.
    pub fn report_log(&self) -> &[FrameReport] {
        &self.reports.changes
    }

    /// Stops keeping the log of changes (a window running for hours doesn't need it).
    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window uses it
    pub(crate) fn forget_report_log(&mut self) {
        self.keep_sounds = false;
        self.sounds.clear();
        self.sounds_sent = 0;
        self.reports.keep_changes = false;
        self.reports.changes.clear();
    }

    /// Turns the debug overlay on or off for every picture from now on
    /// (what F1 does in a window, and `--debug` on the command line). It
    /// shows the FPS, frame, camera, mouse, and the outlines the game
    /// registered with [`Context::debug_rect`] and
    /// [`Context::debug_circle`].
    pub fn set_debug_overlay(&mut self, on: bool) {
        if self.debug_overlay != on {
            self.debug_overlay = on;
            self.drawn_frame = None; // the picture needs drawing again
        }
    }

    /// Changes the canvas to `width` x `height` pixels and calls the game's
    /// [`Game::resize`], as if the player had resized a
    /// [`resizable`](Config::resizable) window. For testing how a game
    /// handles a new size. Sizes below 1 become 1; the same size as now
    /// does nothing.
    pub fn resize(&mut self, width: usize, height: usize) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.canvas.width(), self.canvas.height()) {
            return;
        }
        let mut canvas = Canvas::new(width, height);
        canvas.set_renderer(self.renderer);
        self.canvas = canvas;
        self.drawn_frame = None;
        self.ctx.set_size(width, height);
        self.game.resize(&mut self.ctx, width as f32, height as f32);
    }

    /// Every sound the game played, oldest first, each with its `frame`
    /// and `name`.
    pub fn sounds(&self) -> &[PlayedSound] {
        &self.sounds
    }

    /// The frames on which a sound called `name` was played, like
    /// `[12, 80]`. Names are [`Sound::name`](crate::sound::Sound::name)
    /// (`"coin"`, `"jump"`...), `"beep"`, or a `.wav` file's name.
    pub fn sound_frames(&self, name: &str) -> Vec<u64> {
        self.sounds
            .iter()
            .filter(|sound| sound.name == name)
            .map(|sound| sound.frame)
            .collect()
    }

    /// Checks that a sound called `name` was played on `frame`, or at most
    /// `within` frames before or after it, and panics with a clear message
    /// (naming what *was* played) if not. For play tests:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # struct G;
    /// # impl Game for G {
    /// #     fn update(&mut self, ctx: &mut Context) { if ctx.frame() == 31 { ctx.play(Sound::coin()); } }
    /// #     fn draw(&self, _: &mut Canvas) {}
    /// # }
    /// let played = PlayTest::new(Config::default(), G).frames(60).run();
    /// played.assert_sound("coin", 30, 2); // a coin sound on frames 28 to 32
    /// ```
    #[track_caller]
    pub fn assert_sound(&self, name: &str, frame: u64, within: u64) {
        let frames = self.sound_frames(name);
        if frames.iter().any(|&f| f.abs_diff(frame) <= within) {
            return;
        }
        let window = format!("frame {frame} (give or take {within})");
        if frames.is_empty() {
            let played: Vec<String> = self
                .sounds
                .iter()
                .take(20)
                .map(|s| format!("{} at {}", s.name, s.frame))
                .collect();
            let played = if played.is_empty() {
                String::from("no sounds at all")
            } else {
                played.join(", ")
            };
            panic!(
                "expected the sound {name:?} at {window}, but it was never played; the game played {played}"
            );
        }
        panic!("expected the sound {name:?} at {window}, but it was played on frames {frames:?}");
    }

    /// The sounds played since the last call, for the speakers. In a
    /// window they're then forgotten, so a long game doesn't fill memory.
    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window uses it
    pub(crate) fn take_new_sounds(&mut self) -> Vec<PlayedSound> {
        let new = self.sounds[self.sounds_sent..].to_vec();
        if self.keep_sounds {
            self.sounds_sent = self.sounds.len();
        } else {
            self.sounds.clear();
            self.sounds_sent = 0;
        }
        new
    }

    /// Has the game called [`Context::quit`]?
    pub fn quit_requested(&self) -> bool {
        self.ctx.quit_requested()
    }

    /// The picture of the game as it is now.
    ///
    /// Drawing only happens when a picture is asked for, so steps are
    /// quick. That's safe because [`Game::draw`] can't change the game: a
    /// game plays out the same whether it's drawn every frame or never.
    pub fn picture(&mut self) -> &Canvas {
        let frame = self.ctx.frame();
        if self.drawn_frame != Some(frame) {
            self.draw();
            self.drawn_frame = Some(frame);
        }
        &self.canvas
    }

    /// Draws the game and hands over the canvas, so the window can add the
    /// FPS counter on top before showing it.
    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window uses it
    pub(crate) fn draw_for_screen(&mut self) -> &mut Canvas {
        self.draw();
        self.drawn_frame = None; // about to be drawn on, so no longer clean
        &mut self.canvas
    }

    #[cfg_attr(not(feature = "window"), allow(dead_code))] // only the window uses it
    pub(crate) fn measure_frame(&mut self, frame_time: f32) {
        self.ctx.measure_frame(frame_time);
    }

    fn draw(&mut self) {
        self.canvas.reset_view(); // every frame starts with no camera
        self.canvas.take_frame_camera();
        self.game.draw(&mut self.canvas);
        self.canvas.reset_view(); // so the engine's own drawing is in screen pixels
        let camera = self.canvas.take_frame_camera();
        if self.debug_overlay {
            let shapes = self.ctx.debug_shapes();
            crate::debug::draw_overlay(&mut self.canvas, &self.ctx, shapes, camera.as_ref());
        }
        self.canvas.flush_3d(); // make sure any 3D from the graphics card is in the pixels
        self.ctx.set_renderer_name(self.canvas.renderer_name());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::engine::Timestep;
    use crate::math::vec2;

    /// A game that writes down when its hooks run.
    #[derive(Default)]
    struct Hooks {
        log: Vec<String>,
    }

    impl Game for Hooks {
        fn init(&mut self, ctx: &mut Context) {
            self.log.push(format!(
                "init {} {}x{}",
                ctx.frame(),
                ctx.width(),
                ctx.height()
            ));
            ctx.report("started", true);
        }

        fn resize(&mut self, ctx: &mut Context, width: f32, height: f32) {
            self.log
                .push(format!("resize {width}x{height} {}", ctx.width()));
        }

        fn update(&mut self, ctx: &mut Context) {
            self.log.push(format!("update {}", ctx.frame()));
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLUE);
        }
    }

    #[test]
    fn init_runs_once_before_the_first_update() {
        let mut sim = Simulation::new(Config::default(), Hooks::default());
        assert!(
            sim.game().log.is_empty(),
            "nothing runs until the first step"
        );
        sim.run(2);
        assert_eq!(sim.game().log, ["init 0 320x240", "update 1", "update 2"]);
        assert_eq!(
            sim.reported_at(1, "started").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn resizing_changes_the_canvas_and_tells_the_game() {
        let mut sim = Simulation::new(Config::default(), Hooks::default());
        sim.step();
        sim.resize(200, 100);
        sim.resize(200, 100); // the same size again: nothing happens
        sim.step();
        assert_eq!(
            sim.game().log,
            [
                "init 0 320x240",
                "update 1",
                "resize 200x100 200",
                "update 2"
            ]
        );
        assert_eq!(
            (sim.context().width(), sim.context().height()),
            (200.0, 100.0)
        );
        let picture = sim.picture();
        assert_eq!((picture.width(), picture.height()), (200, 100));
        assert_eq!(picture.get_pixel(199, 99), Some(Color::BLUE));
        sim.resize(0, 0);
        assert_eq!(sim.context().width(), 1.0, "never smaller than 1 x 1");
    }

    #[test]
    fn games_without_hooks_still_compile_and_run() {
        // Recorder (below) writes neither init nor resize: the defaults do nothing.
        let mut sim = Simulation::new(Config::default(), Recorder::default());
        sim.resize(10, 10);
        sim.step();
        assert_eq!(sim.context().frame(), 1);
    }

    /// A test game that remembers what it saw on each update.
    #[derive(Default)]
    struct Recorder {
        dts: Vec<f32>,
        presses: u32,
        held_frames: u32,
        clicks: Vec<Vec2>,
        scrolls: Vec<f32>,
        numbers: Vec<f32>,
        quit_after: Option<u64>,
    }

    impl Game for Recorder {
        fn update(&mut self, ctx: &mut Context) {
            self.dts.push(ctx.dt());
            if ctx.input.was_pressed(Key::Space) {
                self.presses += 1;
            }
            if ctx.input.is_down(Key::Space) {
                self.held_frames += 1;
            }
            if ctx.input.was_mouse_pressed(MouseButton::Left) {
                self.clicks.push(ctx.input.mouse_position());
            }
            if ctx.input.scroll() != 0.0 {
                self.scrolls.push(ctx.input.scroll());
            }
            self.numbers.push(ctx.rng.next_f32());
            if Some(ctx.frame()) == self.quit_after {
                ctx.quit();
            }
        }

        fn draw(&self, canvas: &mut Canvas) {
            // Paint the number of presses, for the test to read back.
            canvas.clear(Color::BLACK);
            canvas.set_pixel(0, 0, Color::rgb(self.presses as u8, 0, 0));
        }
    }

    fn config() -> Config {
        Config {
            width: 8,
            height: 8,
            ..Config::default()
        }
    }

    #[test]
    fn every_step_gets_the_same_dt() {
        let mut sim = Simulation::new(config(), Recorder::default());
        sim.run(100);
        assert!(sim.game().dts.iter().all(|&dt| dt == 1.0 / 60.0));
        assert_eq!(sim.context().frame(), 100);
        assert!((sim.context().time() - 100.0 / 60.0).abs() < 1e-5);
        // Without a clock, the FPS counter shows the update rate.
        assert!((sim.context().fps() - 60.0).abs() < 0.01);

        let fast = Config {
            timestep: Timestep::Fixed {
                updates_per_second: 120,
            },
            ..config()
        };
        let mut sim = Simulation::new(fast, Recorder::default());
        sim.step();
        assert_eq!(sim.game().dts, [1.0 / 120.0]);
    }

    #[test]
    fn real_time_mode_without_a_clock_uses_the_target_fps() {
        let real_time = Config {
            timestep: Timestep::RealTime,
            target_fps: 30,
            ..config()
        };
        let mut sim = Simulation::new(real_time, Recorder::default());
        sim.step();
        assert_eq!(sim.game().dts, [1.0 / 30.0]);
    }

    #[test]
    fn keys_are_seen_at_the_next_step() {
        let mut sim = Simulation::new(config(), Recorder::default());
        sim.press(Key::Space);
        sim.press(Key::Space); // pressing a held key again changes nothing
        sim.run(3);
        sim.release(Key::Space);
        sim.run(2);
        assert_eq!(sim.game().presses, 1);
        assert_eq!(sim.game().held_frames, 3);

        // Pressed and released between the same two steps: never seen.
        sim.press(Key::Space);
        sim.release(Key::Space);
        sim.step();
        assert_eq!(sim.game().presses, 1);
    }

    #[test]
    fn mouse_clicks_and_scrolls() {
        let mut sim = Simulation::new(config(), Recorder::default());
        sim.move_mouse(vec2(3.0, 4.0));
        sim.press_mouse(MouseButton::Left);
        sim.scroll(-3.0);
        sim.step();
        sim.release_mouse(MouseButton::Left);
        sim.step(); // the scroll only lasts one step
        assert_eq!(sim.game().clicks, [vec2(3.0, 4.0)]);
        assert_eq!(sim.game().scrolls, [-1.0]);
    }

    #[test]
    fn the_seed_decides_the_random_numbers() {
        let seeded = |seed| Config {
            seed: Some(seed),
            ..config()
        };
        let numbers = |config: Config| {
            let mut sim = Simulation::new(config, Recorder::default());
            sim.run(5);
            sim.into_game().numbers
        };
        assert_eq!(numbers(seeded(42)), numbers(seeded(42)));
        assert_ne!(numbers(seeded(42)), numbers(seeded(43)));
        // No seed: the default one, so still the same every time.
        assert_eq!(numbers(config()), numbers(seeded(DEFAULT_SEED)));
    }

    #[test]
    fn run_stops_when_the_game_quits() {
        let game = Recorder {
            quit_after: Some(4),
            ..Recorder::default()
        };
        let mut sim = Simulation::new(config(), game);
        sim.run(100);
        assert!(sim.quit_requested());
        assert_eq!(sim.context().frame(), 4);
    }

    #[test]
    fn pictures_are_drawn_when_asked_for() {
        let mut sim = Simulation::new(config(), Recorder::default());
        assert_eq!(sim.picture().get_pixel(0, 0), Some(Color::rgb(0, 0, 0)));
        sim.press(Key::Space);
        sim.step();
        assert_eq!(sim.picture().get_pixel(0, 0), Some(Color::rgb(1, 0, 0)));
    }

    #[test]
    fn holding_a_set_of_keys_presses_and_releases_the_difference() {
        let mut sim = Simulation::new(config(), Recorder::default());
        sim.hold_keys(&[Key::A, Key::Space]);
        assert_eq!(sim.keys_down, [Key::A, Key::Space]);
        sim.hold_keys(&[Key::Space, Key::B]);
        assert_eq!(sim.keys_down, [Key::Space, Key::B]);
        sim.hold_mouse_buttons(&[MouseButton::Right]);
        sim.hold_mouse_buttons(&[]);
        assert!(sim.mouse_buttons_down.is_empty());
    }

    /// Plays a little: holds Space, clicks, scrolls, and presses the
    /// engine's own F12. Returns the simulation.
    fn play_a_little(seed: u32) -> Simulation<Recorder> {
        let config = Config {
            seed: Some(seed),
            ..config()
        };
        let mut sim = Simulation::new(config, Recorder::default());
        sim.step();
        sim.press(Key::Space);
        sim.press(Key::F12);
        sim.run(3);
        sim.release(Key::Space);
        sim.move_mouse(vec2(2.0, 5.0));
        sim.press_mouse(MouseButton::Left);
        sim.step();
        sim.release_mouse(MouseButton::Left);
        sim.scroll(1.0);
        sim.run(5);
        sim
    }

    #[test]
    fn a_recording_replays_exactly() {
        let played = play_a_little(42);
        let recording = played.recording();
        assert_eq!(recording.seed, Some(42));
        assert_eq!(recording.frames, Some(10));
        assert_eq!(recording.updates_per_second, Some(60));
        // Space down (seen on frame 2); then before frame 5: Space up, the
        // mouse move and the click; before frame 6: the click's release and the scroll.
        let frames: Vec<u64> = recording.events.iter().map(|event| event.frame).collect();
        assert_eq!(frames, [2, 5, 5, 5, 6, 6]);
        // F12 belongs to the engine, so it isn't in the recording.
        assert!(
            !recording
                .events
                .iter()
                .any(|event| event.action == InputAction::Press(Key::F12))
        );

        // Replay it in a new simulation, through a JSON file's worth of text.
        let script = InputScript::parse(&recording.to_json()).unwrap();
        let replay_config = Config {
            seed: script.seed,
            ..config()
        };
        let mut replay = Simulation::new(replay_config, Recorder::default());
        replay.follow(script);
        assert!(replay.is_replaying());
        replay.run(10);
        assert!(!replay.is_replaying());

        let (a, b) = (played.game(), replay.game());
        assert_eq!((a.presses, a.held_frames), (b.presses, b.held_frames));
        assert_eq!(a.clicks, b.clicks);
        assert_eq!(a.scrolls, b.scrolls);
        assert_eq!(a.numbers, b.numbers);
        // And a replay records the same thing again.
        assert_eq!(replay.recording().events, played.recording().events);
    }

    #[test]
    fn things_that_change_nothing_are_not_recorded() {
        let mut sim = Simulation::new(config(), Recorder::default());
        sim.release(Key::A); // wasn't held
        sim.press(Key::A);
        sim.press(Key::A); // already held
        sim.move_mouse(vec2(1.0, 1.0));
        sim.move_mouse(vec2(1.0, 1.0)); // same place
        sim.scroll(0.0);
        assert_eq!(sim.recording().events.len(), 2);
    }

    #[test]
    fn script_events_happen_on_their_frame() {
        let script = InputScript::parse(
            r#"{"events": [{"frame": 3, "press": "Space"}, {"frame": 4, "release": "Space"}]}"#,
        )
        .unwrap();
        let mut sim = Simulation::new(config(), Recorder::default());
        sim.follow(script);
        sim.run(2);
        assert_eq!(sim.game().presses, 0);
        sim.step(); // frame 3
        assert_eq!((sim.game().presses, sim.game().held_frames), (1, 1));
        sim.run(5);
        assert_eq!(sim.game().held_frames, 1);
    }

    #[test]
    fn reports_are_remembered() {
        struct Scorer {
            score: u32,
        }
        impl Game for Scorer {
            fn update(&mut self, ctx: &mut Context) {
                if ctx.frame().is_multiple_of(10) {
                    self.score += 1;
                }
                ctx.report("score", self.score);
                ctx.report("frame_is_even", ctx.frame().is_multiple_of(2));
            }
            fn draw(&self, _: &mut Canvas) {}
        }
        let mut sim = Simulation::new(config(), Scorer { score: 0 });
        assert_eq!(sim.reported("score"), None);
        sim.run(25);
        assert_eq!(sim.reported("score"), Some(&ReportValue::Number(2.0)));
        assert_eq!(
            sim.reported_at(15, "score"),
            Some(&ReportValue::Number(1.0))
        );
        let names: Vec<&str> = sim.reports().map(|(name, _)| name).collect();
        assert_eq!(names, ["frame_is_even", "score"]);
        // `frame_is_even` changes every frame, `score` only three times.
        assert_eq!(sim.report_log().len(), 25);
        let score_changes = sim
            .report_log()
            .iter()
            .filter(|report| report.changes.contains_key("score"))
            .count();
        assert_eq!(score_changes, 3);
    }

    #[test]
    #[should_panic(expected = "at least 1 x 1")]
    fn an_impossible_config_panics_with_a_clear_message() {
        let config = Config {
            width: 0,
            ..Config::default()
        };
        Simulation::new(config, Recorder::default());
    }
}

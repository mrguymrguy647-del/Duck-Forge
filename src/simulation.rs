//! Running a game one update at a time, with no window and no clock.
//!
//! The window's game loop in [`run`](crate::run) is built on [`Simulation`]
//! too: it only adds a clock (to decide how many updates are due) and a
//! screen (to show the pictures). So a game behaves exactly the same in a
//! window and in a test.

use crate::canvas::Canvas;
use crate::engine::{Config, Context, Game};
use crate::input::{Key, MouseButton};
use crate::math::Vec2;
use crate::render3d::Renderer;

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
        self.ctx.begin_frame(dt, self.keys_down.iter().copied());
        let buttons = self.mouse_buttons_down.iter().copied();
        self.ctx
            .input
            .update_mouse(self.mouse_position, buttons, self.scroll);
        self.scroll = 0.0;
        self.game.update(&mut self.ctx);
    }

    // ---- Input. Each change is seen by the game at the next step. ----

    /// Holds a key down until [`release`](Simulation::release). The next
    /// step sees it as just pressed.
    ///
    /// To tap a key, press it, step, then release it: pressing and releasing
    /// between the same two steps means the game never sees it.
    pub fn press(&mut self, key: Key) {
        if !self.keys_down.contains(&key) {
            self.keys_down.push(key);
        }
    }

    /// Lets go of a key.
    pub fn release(&mut self, key: Key) {
        self.keys_down.retain(|&held| held != key);
    }

    /// Moves the mouse pointer to `position`, in canvas pixels.
    pub fn move_mouse(&mut self, position: Vec2) {
        self.mouse_position = Some(position);
    }

    /// Holds a mouse button down until [`release_mouse`](Simulation::release_mouse).
    pub fn press_mouse(&mut self, button: MouseButton) {
        if !self.mouse_buttons_down.contains(&button) {
            self.mouse_buttons_down.push(button);
        }
    }

    /// Lets go of a mouse button.
    pub fn release_mouse(&mut self, button: MouseButton) {
        self.mouse_buttons_down.retain(|&held| held != button);
    }

    /// Rolls the scroll wheel during the next step: up (away from you) for
    /// a positive `amount`, down for a negative one.
    pub fn scroll(&mut self, amount: f32) {
        self.scroll = (self.scroll + amount).signum();
    }

    /// Makes exactly `keys` the held keys: presses the new ones and releases the rest.
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
    pub(crate) fn draw_for_screen(&mut self) -> &mut Canvas {
        self.draw();
        self.drawn_frame = None; // about to be drawn on, so no longer clean
        &mut self.canvas
    }

    pub(crate) fn measure_frame(&mut self, frame_time: f32) {
        self.ctx.measure_frame(frame_time);
    }

    fn draw(&mut self) {
        self.game.draw(&mut self.canvas);
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

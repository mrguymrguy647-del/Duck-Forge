//! Play tests: a normal `cargo test` that plays a game (usually by
//! replaying an input script) and checks what the game reported.
//!
//! ```
//! use duckforge::prelude::*;
//! use duckforge::PlayTest;
//!
//! struct Clicker {
//!     clicks: u32,
//! }
//!
//! impl Game for Clicker {
//!     fn update(&mut self, ctx: &mut Context) {
//!         if ctx.input.was_pressed(Key::Space) {
//!             self.clicks += 1;
//!         }
//!         ctx.report("clicks", self.clicks);
//!         ctx.report("won", self.clicks >= 2);
//!     }
//!
//!     fn draw(&self, canvas: &mut Canvas) {
//!         canvas.clear(Color::BLACK);
//!     }
//! }
//!
//! let played = PlayTest::new(Config::default(), Clicker { clicks: 0 })
//!     .script(
//!         r#"{"events": [
//!             {"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"},
//!             {"frame": 20, "press": "Space"}, {"frame": 21, "release": "Space"}
//!         ]}"#,
//!     )
//!     .frames(60)
//!     .run();
//! assert_eq!(played.number("clicks"), 2.0);
//! assert!(played.flag("won"));
//! ```
//!
//! In a real project the script is usually a file, recorded with F10 or
//! written by hand: `.inputs("playthroughs/level-1.json")`.

use std::path::PathBuf;

use crate::engine::{Config, Game};
use crate::headless::{HeadlessOptions, run_headless};
use crate::script::InputScript;
use crate::simulation::Simulation;

/// A game, the inputs to play it with, and how long to play. Build one
/// with [`PlayTest::new`], then [`run`](PlayTest::run) it to get the
/// finished [`Simulation`] and check what the game reported with
/// [`Simulation::number`], [`Simulation::flag`] and [`Simulation::text`].
pub struct PlayTest<G> {
    config: Config,
    game: G,
    options: HeadlessOptions,
    inputs: Inputs,
    frames: Option<u64>,
    ignore_checks: bool,
}

/// Where the inputs come from.
enum Inputs {
    None,
    File(PathBuf),
    Text(String),
}

impl<G: Game> PlayTest<G> {
    /// A play test of `game`, with nobody touching anything, for 600
    /// frames (10 seconds), until you say otherwise.
    pub fn new(config: Config, game: G) -> Self {
        PlayTest {
            config,
            game,
            options: HeadlessOptions::default(),
            inputs: Inputs::None,
            frames: None,
            ignore_checks: false,
        }
    }

    /// Replays the input script in this file (see `docs/input-format.md`).
    /// Relative paths start from the project folder (where `Cargo.toml`
    /// is), because that's where `cargo test` runs tests.
    pub fn inputs(mut self, path: impl Into<PathBuf>) -> Self {
        self.inputs = Inputs::File(path.into());
        self
    }

    /// Replays an input script written right here, as JSON text.
    pub fn script(mut self, json: &str) -> Self {
        self.inputs = Inputs::Text(json.to_string());
        self
    }

    /// Plays this many frames. Without this, a test with a script plays as
    /// long as the script's `"frames"` says, and otherwise 600.
    pub fn frames(mut self, frames: u64) -> Self {
        self.frames = Some(frames);
        self
    }

    /// Uses this random seed (instead of the script's, the config's, or 1).
    pub fn seed(mut self, seed: u32) -> Self {
        self.options.seed = Some(seed);
        self
    }

    /// Sets one `ctx.tune` value for this test, over the tune file (like
    /// `--set name=value`). Call it again for more.
    pub fn tune(mut self, name: &str, value: f32) -> Self {
        self.options.tune.push((name.to_string(), value));
        self
    }

    /// Reads `ctx.tune` values from this file instead of
    /// `examples/<game>.tune.json` (like `--tune FILE`).
    pub fn tune_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.options.tune_file = Some(path.into());
        self
    }

    /// Doesn't fail when a `ctx.check` rule fails. (By default a play test
    /// fails if any did, naming the rule and the frame.) Check
    /// [`Simulation::failed_checks`] yourself if a failure is expected.
    pub fn ignore_checks(mut self) -> Self {
        self.ignore_checks = true;
        self
    }

    /// Also saves pictures of the run in `folder`: `last.png`, `sheet.png`
    /// and `log.json`, to look at when a test fails.
    pub fn save_to(mut self, folder: impl Into<PathBuf>) -> Self {
        self.options.out = Some(folder.into());
        self.options.sheet = true;
        self
    }

    /// Plays the game and returns the finished simulation.
    ///
    /// # Panics
    ///
    /// With a clear message, if the script can't be read or the run fails.
    /// (This is for tests, where a panic is how a test fails.)
    #[track_caller]
    pub fn run(self) -> Simulation<G> {
        let PlayTest {
            config,
            game,
            mut options,
            inputs,
            frames,
            ignore_checks,
        } = self;
        let script = match inputs {
            Inputs::None => None,
            Inputs::File(path) => Some(InputScript::load(&path)),
            Inputs::Text(json) => Some(InputScript::parse(&json)),
        };
        options.inputs = match script {
            Some(Ok(script)) => Some(script),
            Some(Err(err)) => panic!("play test: {err}"),
            None => None,
        };
        let updates_per_second = 1.0 / f64::from(config.step_time());
        let script_frames = options
            .inputs
            .as_ref()
            .and_then(|script| script.run_length(updates_per_second));
        options.frames = frames.or(script_frames).unwrap_or(options.frames);
        let sim = match run_headless(config, game, &options) {
            Ok(sim) => sim,
            Err(err) => panic!("play test: {err}"),
        };
        if let Some(failed) = sim.failed_checks().first().filter(|_| !ignore_checks) {
            let more = sim.failed_checks().len() - 1;
            let others = if more > 0 {
                format!(" (and {more} more)")
            } else {
                String::new()
            };
            panic!(
                "play test: the check {:?} failed on frame {}{others}; replay to just before it with \
                 --frames {}, or call .ignore_checks() if the failure is expected",
                failed.name,
                failed.frame,
                failed.frame.saturating_sub(1).max(1)
            );
        }
        sim
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;
    use crate::color::Color;
    use crate::engine::Context;
    use crate::input::Key;

    struct Counter {
        presses: u32,
    }

    impl Game for Counter {
        fn update(&mut self, ctx: &mut Context) {
            if ctx.input.was_pressed(Key::Space) {
                self.presses += 1;
            }
            ctx.report("presses", self.presses);
            ctx.report("name", "counter");
            ctx.report("any", self.presses > 0);
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
        }
    }

    fn config() -> Config {
        Config {
            width: 16,
            height: 16,
            ..Config::default()
        }
    }

    const SCRIPT: &str = r#"{"frames": 40, "events": [
        {"frame": 5, "press": "Space"}, {"frame": 6, "release": "Space"}
    ]}"#;

    #[test]
    fn a_script_is_played_for_its_length() {
        let played = PlayTest::new(config(), Counter { presses: 0 })
            .script(SCRIPT)
            .run();
        assert_eq!(played.context().frame(), 40);
        assert_eq!(played.number("presses"), 1.0);
        assert!(played.flag("any"));
        assert_eq!(played.text("name"), "counter");
    }

    #[test]
    fn frames_and_seed_can_be_chosen() {
        let played = PlayTest::new(config(), Counter { presses: 0 })
            .script(SCRIPT)
            .frames(3)
            .seed(77)
            .run();
        assert_eq!(played.context().frame(), 3);
        assert_eq!(played.seed(), 77);
        assert_eq!(played.number("presses"), 0.0);
    }

    #[test]
    fn script_files_are_read() {
        let path =
            std::env::temp_dir().join(format!("duckforge-playtest-{}.json", std::process::id()));
        std::fs::write(&path, SCRIPT).unwrap();
        let played = PlayTest::new(config(), Counter { presses: 0 })
            .inputs(&path)
            .run();
        let _ = std::fs::remove_file(&path);
        assert_eq!(played.number("presses"), 1.0);
    }

    #[test]
    #[should_panic(expected = "play test: could not read nowhere.json")]
    fn a_missing_script_fails_the_test_clearly() {
        PlayTest::new(config(), Counter { presses: 0 })
            .inputs("nowhere.json")
            .run();
    }

    #[test]
    #[should_panic(expected = "the game never reported \"lives\"; it reported: any, name, presses")]
    fn asking_for_an_unreported_value_says_what_was_reported() {
        let played = PlayTest::new(config(), Counter { presses: 0 })
            .frames(1)
            .run();
        played.number("lives");
    }

    #[test]
    #[should_panic(expected = "\"name\" is \"counter\", not a number")]
    fn asking_for_the_wrong_kind_of_value_says_so() {
        let played = PlayTest::new(config(), Counter { presses: 0 })
            .frames(1)
            .run();
        played.number("name");
    }
}

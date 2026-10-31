//! Running a game with no window: for tests, for scripts, and for
//! computers with no screen (like a cloud server where an AI writes games).
//!
//! The game runs as fast as the computer allows, not in real time: ten
//! seconds of game time usually takes a fraction of a second. Pictures are
//! drawn on the CPU, so no graphics card is needed, and saved as PNG files.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::cli::{self, Options};
use crate::engine::{Config, Game};
use crate::simulation::Simulation;

/// What a headless run should do. Make one with `HeadlessOptions::default()`
/// and change what you need:
///
/// ```
/// use duckforge::HeadlessOptions;
///
/// let options = HeadlessOptions {
///     frames: 120,
///     out: Some("shots".into()),
///     ..HeadlessOptions::default()
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct HeadlessOptions {
    /// How many updates to run (60 is one second of game time with the
    /// default timestep). The run stops sooner if the game calls
    /// `ctx.quit()`.
    pub frames: u64,
    /// The folder to save files in: `last.png`, the last frame. It's
    /// created if it doesn't exist. `None` saves nothing.
    pub out: Option<PathBuf>,
    /// The random seed. `None` uses `Config::seed`, or if that's `None` too,
    /// [`DEFAULT_SEED`](crate::simulation::DEFAULT_SEED), so runs are
    /// repeatable unless you ask otherwise.
    pub seed: Option<u32>,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            frames: 600,
            out: None,
            seed: None,
        }
    }
}

/// Runs `game` for `options.frames` updates without a window, saves the
/// files `options` asks for, and hands back the finished [`Simulation`], so
/// you can look at the game, its last picture and more.
///
/// Every game can do this from the command line too, without any code:
/// `cargo run --example breakout -- --headless --frames 600 --out shots`.
///
/// Returns an error if the config is impossible or a file can't be saved.
pub fn run_headless<G: Game>(
    mut config: Config,
    game: G,
    options: &HeadlessOptions,
) -> Result<Simulation<G>, String> {
    config.check()?;
    config.seed = options.seed.or(config.seed);
    let mut sim = Simulation::new(config, game);
    sim.run(options.frames);

    if let Some(folder) = &options.out {
        fs::create_dir_all(folder)
            .map_err(|err| format!("could not create the folder {}: {err}", folder.display()))?;
        save_png(&mut sim, &folder.join("last.png"))?;
    }
    Ok(sim)
}

fn save_png<G: Game>(sim: &mut Simulation<G>, path: &Path) -> Result<(), String> {
    sim.picture()
        .save_png(path)
        .map_err(|err| format!("could not save {}: {err}", path.display()))
}

/// What `run` does with `--headless`: runs the game, then says what happened.
pub(crate) fn run_from_command_line<G: Game>(
    config: Config,
    game: G,
    options: &Options,
) -> Result<(), String> {
    let headless = HeadlessOptions {
        frames: options.frames.unwrap_or(HeadlessOptions::default().frames),
        out: Some(
            options
                .out
                .clone()
                .unwrap_or_else(|| PathBuf::from(cli::DEFAULT_OUT)),
        ),
        seed: options.seed,
    };
    let title = config.title.clone();
    let started = Instant::now();
    let sim = run_headless(config, game, &headless)?;
    let seconds = started.elapsed().as_secs_f32();

    let ctx = sim.context();
    let mut summary = format!(
        "duckforge: ran {title:?} without a window for {} frames ({:.1} s of game time) in {seconds:.2} s, seed {}",
        ctx.frame(),
        ctx.time(),
        sim.seed(),
    );
    if sim.quit_requested() {
        summary += "; the game quit by itself";
    }
    println!("{summary}");
    if let Some(folder) = &headless.out {
        println!("duckforge: saved {}", folder.join("last.png").display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;
    use crate::color::Color;
    use crate::engine::Context;
    use crate::math::Rect;

    /// A square that moves right one pixel per update, and quits at frame `quit_at`.
    struct Mover {
        x: f32,
        quit_at: u64,
    }

    impl Game for Mover {
        fn update(&mut self, ctx: &mut Context) {
            self.x += 1.0;
            if ctx.frame() == self.quit_at {
                ctx.quit();
            }
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            canvas.fill_rect(Rect::new(self.x, 0.0, 1.0, 1.0), Color::WHITE);
        }
    }

    fn config() -> Config {
        Config {
            width: 64,
            height: 8,
            ..Config::default()
        }
    }

    /// A fresh, empty path in the system's temporary folder for a test to write to.
    fn test_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir()
            .join(format!("duckforge-tests-{}", std::process::id()))
            .join(name);
        let _ = fs::remove_dir_all(&folder);
        let _ = fs::remove_file(&folder);
        folder
    }

    #[test]
    fn runs_the_frames_and_saves_the_last_one() {
        let folder = test_folder("headless-last");
        let options = HeadlessOptions {
            frames: 30,
            out: Some(folder.clone()),
            ..HeadlessOptions::default()
        };
        let game = Mover { x: 0.0, quit_at: 0 };
        let mut sim = run_headless(config(), game, &options).unwrap();
        assert_eq!(sim.context().frame(), 30);
        assert_eq!(sim.game().x, 30.0);
        assert_eq!(sim.picture().get_pixel(30, 0), Some(Color::WHITE));

        let saved = Canvas::load_png_for_tests(&folder.join("last.png"));
        assert_eq!(saved.pixels(), sim.picture().pixels());
    }

    #[test]
    fn stops_early_when_the_game_quits() {
        let game = Mover {
            x: 0.0,
            quit_at: 12,
        };
        let sim = run_headless(config(), game, &HeadlessOptions::default()).unwrap();
        assert!(sim.quit_requested());
        assert_eq!(sim.context().frame(), 12);
    }

    #[test]
    fn the_seed_option_wins_over_the_config() {
        let options = HeadlessOptions {
            frames: 1,
            seed: Some(99),
            ..HeadlessOptions::default()
        };
        let seeded = Config {
            seed: Some(5),
            ..config()
        };
        let game = Mover { x: 0.0, quit_at: 0 };
        assert_eq!(run_headless(seeded, game, &options).unwrap().seed(), 99);
    }

    #[test]
    fn problems_come_back_as_errors() {
        let game = || Mover { x: 0.0, quit_at: 0 };
        let no_size = Config {
            width: 0,
            ..config()
        };
        let err = run_headless(no_size, game(), &HeadlessOptions::default()).err();
        assert!(err.unwrap().contains("at least 1 x 1"));

        // A file where the folder should be.
        let file = test_folder("headless-not-a-folder");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "in the way").unwrap();
        let options = HeadlessOptions {
            frames: 1,
            out: Some(file.join("pictures")),
            ..HeadlessOptions::default()
        };
        let err = run_headless(config(), game(), &options).err().unwrap();
        assert!(err.contains("could not create the folder"), "{err}");
    }
}

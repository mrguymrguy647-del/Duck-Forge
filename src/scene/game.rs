//! A whole game made of scenes, with no code but its scripts.

use std::path::{Path, PathBuf};

use super::{Scene, Scripts};
use crate::canvas::Canvas;
use crate::color::Color;
use crate::engine::{Config, Context, Game};
use crate::math::vec2;

/// A game that plays its scene, its scripts doing the rest. Its `main.rs`
/// is a few lines:
///
/// ```no_run
/// use duckforge::prelude::*;
///
/// fn main() {
///     SceneGame::new("examples/bee_maze").run();
/// }
/// ```
///
/// with `.with_scripts(scripts::all())` once it has scripts. It plays the
/// scene `game.json` names (`{"title": "Bee Maze", "first": "level1"}`),
/// or, without one, the first scene in `scenes/` by name. (Going from one
/// scene to another comes with several scenes, later in Phase 5.)
#[derive(Debug)]
pub struct SceneGame {
    title: String,
    /// The scene, or why it couldn't be loaded.
    scene: Result<Scene, String>,
}

impl SceneGame {
    /// The game in `folder`, with its first scene loaded.
    pub fn new(folder: impl AsRef<Path>) -> SceneGame {
        let folder = folder.as_ref();
        let about = std::fs::read_to_string(folder.join("game.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        let title = about
            .as_ref()
            .and_then(|about| about["title"].as_str())
            .map(String::from)
            .unwrap_or_else(|| last_name(folder));
        let first = about
            .as_ref()
            .and_then(|about| about["first"].as_str())
            .map(|name| folder.join("scenes").join(format!("{name}.json")))
            .or_else(|| first_scene(folder));
        let scene = match first {
            Some(file) => Scene::load(&file).map_err(|error| error.to_string()),
            None => Err(format!(
                "{} has no scene: put one in its scenes folder, like scenes/level1.json",
                last_name(folder)
            )),
        };
        SceneGame { title, scene }
    }

    /// Gives its scenes the game's scripts.
    pub fn with_scripts(mut self, scripts: Scripts) -> SceneGame {
        if let Ok(scene) = &mut self.scene {
            scene.set_scripts(scripts);
        }
        self
    }

    /// The scene playing, if it could be loaded.
    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref().ok()
    }

    /// The scene playing, to change.
    pub fn scene_mut(&mut self) -> Option<&mut Scene> {
        self.scene.as_mut().ok()
    }

    /// Plays it, in a window (or as the command line asks: `--headless`,
    /// `--frames`, the Studio's link...).
    pub fn run(self) {
        let config = Config {
            title: self.title.clone(),
            ..Config::default()
        };
        if let Err(error) = crate::engine::run(config, self) {
            eprintln!("duckforge: {error}");
            std::process::exit(1);
        }
    }
}

/// A folder's own name: `bee_maze`.
fn last_name(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Words in lines of at most `width` letters.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.len() + 1 + word.len() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The game's first scene by name, in its `scenes` folder.
fn first_scene(folder: &Path) -> Option<PathBuf> {
    let mut scenes: Vec<PathBuf> = std::fs::read_dir(folder.join("scenes"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|e| e == "json")
                && !path.to_string_lossy().ends_with(".legend.json")
        })
        .collect();
    scenes.sort();
    scenes.into_iter().next()
}

impl Game for SceneGame {
    fn update(&mut self, ctx: &mut Context) {
        if let Ok(scene) = &mut self.scene {
            scene.update(ctx);
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        match &self.scene {
            Ok(scene) => scene.draw(canvas),
            Err(why) => {
                canvas.clear(Color::rgb(30, 20, 70));
                let mut y = 8.0;
                for line in wrapped(why, (canvas.width() / 6).max(10)) {
                    canvas.draw_text(&line, vec2(8.0, y), 1, Color::WHITE);
                    y += 10.0;
                }
            }
        }
    }

    fn reload(&mut self, ctx: &mut Context, file: &Path) -> bool {
        match &mut self.scene {
            Ok(scene) => scene.reload(ctx, file),
            Err(_) => false,
        }
    }
}

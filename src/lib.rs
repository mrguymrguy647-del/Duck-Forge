//! # duckforge
//!
//! A deliberately small 2D game engine, written for learning Rust.
//!
//! A game is any type that implements the [`Game`](engine::Game) trait.
//! Hand it to [`run`] and the engine opens a window and calls your game's
//! `update` and `draw` methods once per frame:
//!
//! ```no_run
//! use duckforge::prelude::*;
//!
//! struct MyGame {
//!     x: f32,
//! }
//!
//! impl Game for MyGame {
//!     fn update(&mut self, ctx: &mut Context) {
//!         self.x += 50.0 * ctx.dt(); // move right at 50 pixels per second
//!     }
//!
//!     fn draw(&self, canvas: &mut Canvas) {
//!         canvas.clear(Color::BLACK);
//!         canvas.fill_rect(Rect::new(self.x, 100.0, 16.0, 16.0), Color::YELLOW);
//!     }
//! }
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     duckforge::run(Config::default(), MyGame { x: 0.0 })
//! }
//! ```

pub mod background;
pub mod blocks;
pub mod camera;
pub mod canvas;
mod cli;
pub mod color;
mod debug;
mod diff;
pub mod engine;
pub mod error;
mod files;
mod fuzz;
pub mod glb;
#[cfg(feature = "gpu")]
mod gpu;
pub mod headless;
pub mod input;
pub mod math;
pub mod on_screen;
pub mod physics;
pub mod pictures;
pub mod playtest;
pub mod render3d;
pub mod report;
pub mod rng;
pub mod scene;
pub mod script;
mod sheet;
pub mod simulation;
pub mod sound;
#[cfg(all(feature = "audio", any(feature = "window", feature = "studio")))]
mod speaker;
pub mod sprite;
pub mod spritesheet;
/// The link between a game and Duck Forge Studio. Games don't use it
/// themselves: the Studio builds them with the `studio` feature and starts
/// them with `--studio`.
#[cfg(feature = "studio")]
pub mod studio_link;
mod summary;
#[cfg(test)]
mod test_folder;
pub mod tilemap;
pub mod tileset;
mod tune;
mod video;
#[cfg(feature = "window")]
mod window;

// Not `pub`: the font is an internal detail of `Canvas::draw_text`.
mod font;

pub use cli::{game_args, in_studio, is_headless};
pub use engine::run;
pub use error::LoadError;
pub use headless::{HeadlessOptions, run_headless};
pub use pictures::Crop;
pub use playtest::PlayTest;
pub use report::ReportValue;
pub use script::{InputAction, InputEvent, InputScript};
pub use simulation::Simulation;

/// Everything a game usually needs, importable in one line:
/// `use duckforge::prelude::*;`
pub mod prelude {
    pub use crate::background::Background;
    pub use crate::blocks::{AIR, BlockId, BlockKind, BlockPalette, BlockWorld, FaceLight};
    pub use crate::camera::Camera2D;
    pub use crate::canvas::{Canvas, Placed};
    pub use crate::color::Color;
    pub use crate::engine::{Config, Context, Corner, FailedCheck, Game, Timestep};
    pub use crate::input::{Input, Key, MouseButton};
    pub use crate::math::{
        Hit, Rect, Vec2, Vec3, approach, circle_vs_circle, circle_vs_rect, remap, swept_aabb, vec2,
        vec3,
    };
    pub use crate::playtest::PlayTest;
    pub use crate::render3d::{Billboard, Camera3D, Fog, Lighting, Mesh, Renderer, Transform};
    pub use crate::report::ReportValue;
    pub use crate::rng::Rng;
    pub use crate::scene::{Anchor, Id, Object, Scene, SceneGame, Script, Scripts, Settings};
    pub use crate::script::InputScript;
    pub use crate::simulation::Simulation;
    pub use crate::sound::{LoopId, Sound, Wav, Wave};
    pub use crate::sprite::{Animation, Blit, Sprite};
    pub use crate::spritesheet::SpriteSheet;
    pub use crate::tilemap::{
        Contacts, Hop, Jump, Legend, Letter, LetterKind, Reach, SpawnChanges, TileKind, TileMap,
    };
    pub use crate::tileset::Tileset;
    pub use crate::tune::{FeelPreset, feel_presets};
}

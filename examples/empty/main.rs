//! A window with a background colour, and nothing else yet.
//!
//! Where to start: write what happens each frame in `update` (read keys
//! with `ctx.input`, move things, keep score) and what's on screen in
//! `draw` (after the background). Keep what the game remembers in its
//! struct. docs/QUICKSTART.md has a whole small game to copy from, and
//! TUTORIAL.md explains every part. Pictures and sounds go in this folder
//! (the Studio's Files panel puts them here).

use duckforge::prelude::*;

/// The size of the picture, in pixels. The window shows it enlarged.
const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;

/// Everything the game remembers. Add what it needs here.
struct Empty {}

impl Game for Empty {
    fn update(&mut self, _ctx: &mut Context) {
        // Start here.
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::rgb(40, 60, 100));
    }
}

fn config() -> Config {
    Config {
        title: String::from("Empty - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Empty {})
}

//! The ready-made behaviours, played: a game with no code of its own.
//!
//!     cargo run --example ready_made
//!
//! Arrow keys (or A/D) run, Space (or Up, or W) jumps.
//!
//! Everything is in `scenes/level1.json`. The duck's line says
//! `"kind": "platformer_player"`: Duck Forge's ready-made Platformer
//! player, which runs, jumps, falls and lands on the one-way plank. Its
//! settings are on its line too (`"jump_height": 80`), or in Duck Forge
//! Studio's Inspector, with every other setting it has. "Turn into code"
//! in the Studio gives the game its own copy of the player to change.
//!
//! Each ready-made behaviour is added to this scene as it's built (a coin,
//! a snail, a lift, a door...), so this game plays them all.
//!
//! Without a screen (or to test):
//!
//!     cargo run --no-default-features --example ready_made -- --headless --inputs playthroughs/ready_made.json --out out
//!     cargo test --no-default-features --example ready_made

use duckforge::prelude::*;

fn main() {
    SceneGame::new("examples/ready_made").run();
}

// Play tests: `cargo test --example ready_made`.
#[cfg(test)]
mod tests;

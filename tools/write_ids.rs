//! Gives every object of a scene file that has no `"id"` one, and writes
//! the file again the way Duck Forge writes it (one object per line).
//!
//!     cargo run --example write_ids -- examples/mygame/scenes/level1.json
//!
//! A scene written by hand or by an agent can leave ids out: the engine
//! then gives them the same way every time (the numbers after the highest
//! id in the file, going down the file). But an id that isn't written
//! depends on the object's place, so adding an object above it changes
//! it. This writes them in, once and for all. A file whose objects all
//! have ids isn't touched. Several files can be named.
//!
//! (It's a tool, not a game, so it lives in `tools/`. Duck Forge Studio
//! writes ids itself whenever it saves a scene.)

use std::process::ExitCode;

fn main() -> ExitCode {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() || files.iter().any(|file| file.starts_with("--")) {
        eprintln!(
            "usage: cargo run --example write_ids -- <scene file> [more scene files]\n\
             gives every object without an \"id\" one, and writes the file again"
        );
        return ExitCode::from(2);
    }
    let mut failed = false;
    for file in &files {
        match duckforge::scene::write_ids(file) {
            Ok(0) => println!("{file}: every object already has an id"),
            Ok(1) => println!("{file}: gave 1 object an id, and wrote the file again"),
            Ok(n) => println!("{file}: gave {n} objects an id, and wrote the file again"),
            Err(problem) => {
                eprintln!("{problem}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

//! Everything Duck Forge Studio does, without its window.
//!
//! - [`studio`]: the Studio's state, and every action as one named
//!   [`Command`](studio::Command).
//! - [`folder`]: the games folder, where every path and command comes from.
//! - [`my_games`]: the person's own games folder, made from (and kept up to
//!   date with) the copy of Duck Forge that came with the Studio.
//! - [`console`]: every program the Studio runs, with its real output.
//! - [`build`]: finding Rust, building a game, watching its code.
//! - [`play`]: a game running inside the Studio: the link, the clock,
//!   freeze and resume, the session's recording.
//! - [`check`] and [`clip`]: Check and Clip.
//! - [`level`]: a game's level as Build mode edits it.
//! - [`script`]: running a list of commands with no window.
//! - [`update`]: is there a newer Studio? (Only when the person presses
//!   Update is anything downloaded.)
//!
//! Nothing here opens a window, so all of it can be tested headless, and
//! driven by a script or (later) an agent exactly as the app drives it.

pub mod assets;
pub mod build;
pub mod check;
pub mod clip;
pub mod console;
pub mod feel;
pub mod folder;
pub mod game_files;
pub mod history;
pub mod level;
pub mod manage;
pub mod my_games;
pub mod play;
pub mod problem;
pub mod recycle;
pub mod script;
pub mod settings;
pub mod studio;
#[doc(hidden)]
pub mod test_folder;
pub mod update;

pub use problem::Problem;
pub use studio::{Command, Studio};

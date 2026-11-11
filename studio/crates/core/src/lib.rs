//! Everything Duck Forge Studio does, without its window.
//!
//! - [`studio`]: the Studio's state, and every action as one named
//!   [`Command`](studio::Command).
//! - [`folder`]: the games folder, where every path and command comes from.
//! - [`console`]: every program the Studio runs, with its real output.
//! - [`build`]: finding Rust, building a game, watching its code.
//! - [`play`]: a game running inside the Studio: the link, the clock,
//!   freeze and resume, the session's recording.
//! - [`check`] and [`clip`]: Check and Clip.
//! - [`level`]: a game's level as Build mode edits it.
//! - [`script`]: running a list of commands with no window.
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
pub mod history;
pub mod level;
pub mod play;
pub mod problem;
pub mod script;
pub mod settings;
pub mod studio;
#[doc(hidden)]
pub mod test_folder;

pub use problem::Problem;
pub use studio::{Command, Studio};

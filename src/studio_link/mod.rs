//! The link between a game and Duck Forge Studio.
//!
//! The Studio starts the game with `--studio ADDRESS`. The game then opens
//! no window: it connects to the Studio and plays when the Studio says so,
//! one "tick" at a time, sending back each picture. The Studio keeps the
//! time, so it can freeze the game on any frame just by not sending the
//! next tick, and carry on from exactly there.
//!
//! [`protocol`] is the messages, used by both sides. The rest is the game's
//! side, and nothing a game calls itself: [`run`](crate::run) switches to
//! it when it sees `--studio`.

pub mod protocol;
mod runner;

pub(crate) use runner::run;

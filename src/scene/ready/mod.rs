//! The ready-made kinds: scripts that come with Duck Forge, written just
//! as a game's own are (each file uses only `duckforge::prelude`), so
//! "Turn into code" can give a game its own copy of one.
//!
//! An object whose `"kind"` is one of these runs it in any scene, with no
//! `scripts.rs` and no code: `{"id": 3, "name": "duck", "kind":
//! "platformer_player", "jump_height": 80, ...}`. A game's own script with
//! the same name would take its place, but the Studio won't make one.
//!
//! **Adding the next one** is written out step by step in
//! `docs/decisions.md` ("How to add a ready-made behaviour"): a file here,
//! one line in [`ALL`], a test, and a page in `docs/AI_GUIDE.md`.

pub mod platformer_player;

use super::Scripts;

/// One ready-made kind: its name in scene files, what the Studio calls it,
/// one sentence about it, its type's name and its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyMade {
    /// Its `"kind"` in scene files: `"platformer_player"`.
    pub name: &'static str,
    /// What the Studio calls it: "Platformer player".
    pub label: &'static str,
    /// What it does, in one short sentence.
    pub about: &'static str,
    /// Its type in its file: `PlatformerPlayer`.
    pub type_name: &'static str,
}

/// Every ready-made kind, in the order the Studio lists them.
pub const ALL: [ReadyMade; 1] = [ReadyMade {
    name: "platformer_player",
    label: "Platformer player",
    about: "runs, jumps, falls, and lands on one-way planks",
    type_name: "PlatformerPlayer",
}];

/// The ready-made kinds as a game's scripts are kept: what a scene falls
/// back on for a kind the game's own scripts don't have.
pub(crate) fn scripts() -> Scripts {
    Scripts::new().add::<platformer_player::PlatformerPlayer>("platformer_player")
}

/// The ready-made kind called `name`, if there is one.
pub fn get(name: &str) -> Option<&'static ReadyMade> {
    ALL.iter().find(|ready| ready.name == name)
}

/// A ready-made kind's whole file, as it is in Duck Forge: what "Turn into
/// code" starts from. (Only the Studio asks, so only it carries the text.)
pub fn source(name: &str) -> Option<&'static str> {
    match name {
        "platformer_player" => Some(include_str!("platformer_player.rs")),
        _ => None,
    }
}

/// A game's own copy of a ready-made kind's file, as "Turn into code"
/// writes it: its type is `type_name`, its first line names the script
/// `name`, and the rest is the same, line for line.
pub fn copy_for_a_game(ready: &ReadyMade, name: &str, type_name: &str) -> Option<String> {
    // One kind of line ending all through (a Windows checkout of Duck
    // Forge has "\r\n" in the engine's own file).
    let text = source(ready.name)?.replace("\r\n", "\n");
    // The file's opening words, up to the first line of code.
    let body = text
        .split_once("\nuse ")
        .map(|(_, rest)| format!("use {rest}"))?;
    let header = format!(
        "//! {name}: {about}.\n\
         //! It started as Duck Forge's ready-made \"{label}\", and is this\n\
         //! game's own code now: change anything. Its settings are the boxes\n\
         //! the Inspector shows for it.\n\n",
        about = ready.about,
        label = ready.label,
    );
    Some(header + &replace_word(&body, ready.type_name, type_name))
}

/// `text` with every whole-word `from` made `to` (`PlatformerPlayer`, and
/// not `PlatformerPlayers`).
fn replace_word(text: &str, from: &str, to: &str) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + from.len()..].chars().next();
        out.push_str(&rest[..at]);
        if before.is_some_and(word) || after.is_some_and(word) {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

// One test file per ready-made kind, in tests/, named as its file is.
#[cfg(test)]
mod tests;

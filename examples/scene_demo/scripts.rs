// Duck Forge Studio keeps this file: it lists this game's scripts.
// New script, Rename and Delete in Files change it. Writing it by hand
// is fine too, one line each in the same shape.

use duckforge::prelude::Scripts;

#[path = "scripts/bob.rs"]
mod bob;

#[rustfmt::skip]
pub fn all() -> Scripts {
    Scripts::new()
        .add::<bob::Bob>("bob")
}

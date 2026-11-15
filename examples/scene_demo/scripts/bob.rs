//! bob: a script. Drag it from Files onto an object to give that object
//! this script. Its settings are the boxes the Inspector shows for it.
//!
//! An object with this script bobs gently up and down where it starts,
//! like a coin waiting to be taken. No object in pond.json has it yet: try
//! dragging it onto the coin icon, or onto the sign.

use duckforge::prelude::*;

#[derive(Default)]
pub struct Bob {
    // Each setting is a box in the Inspector.
    how_far: f32,
    how_fast: f32,
    // Things it remembers while the game runs. These have no box.
    home_y: f32,
    time: f32,
}

impl Script for Bob {
    /// The settings: a name, where it goes, and its value at the start.
    fn settings(&mut self, s: &mut Settings) {
        s.number("how_far", &mut self.how_far, 3.0)
            .range(0.0, 32.0)
            .label("Bobs up and down by")
            .help("Pixels above and below where it starts");
        s.number("how_fast", &mut self.how_fast, 1.0)
            .range(0.0, 8.0)
            .label("Bobs a second");
    }

    /// Once, when the object starts: it remembers where that is.
    fn start(&mut self, me: Id, scene: &mut Scene, _ctx: &mut Context) {
        self.home_y = scene[me].at.y;
    }

    /// Sixty times a second, while the game plays.
    fn update(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {
        self.time += ctx.dt();
        // A whole turn of a circle (TAU) is one bob.
        let wave = (self.time * self.how_fast * std::f32::consts::TAU).sin();
        scene[me].at.y = self.home_y + wave * self.how_far;
    }
}

//! How long each stage of making the pictures took, for `summary.txt`:
//! updating the game, adding models, shadow maps, drawing the world,
//! bloom, the overlay drawn on top (text, sprites, the game's own drawing
//! code) and encoding (video, PNG files).
//!
//! Each stage is timed on the thread that asks for it, around the whole
//! stage (its own threads included), so the times add up to the run's.

use std::cell::Cell;
use std::time::Instant;

/// One stage. `NAMES` has their names, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Update,
    Models,
    Shadows,
    World,
    Bloom,
    Overlay,
    Encode,
}

/// What each stage is called in the summary.
pub(crate) const NAMES: [&str; 7] = [
    "updates", "models", "shadows", "world", "bloom", "overlay", "encode",
];

/// The stages that are part of drawing 3D.
const THREE_D: [Stage; 4] = [Stage::Models, Stage::Shadows, Stage::World, Stage::Bloom];

thread_local! {
    static SPENT: Cell<[f64; 7]> = const { Cell::new([0.0; 7]) };
}

/// Adds `seconds` to `stage`.
pub(crate) fn add(stage: Stage, seconds: f64) {
    SPENT.with(|spent| {
        let mut all = spent.get();
        all[stage as usize] += seconds;
        spent.set(all);
    });
}

/// Runs `work`, timing it as `stage`.
pub(crate) fn time<R>(stage: Stage, work: impl FnOnce() -> R) -> R {
    let start = Instant::now();
    let result = work();
    add(stage, start.elapsed().as_secs_f64());
    result
}

/// Seconds so far in each stage on this thread.
pub(crate) fn so_far() -> [f64; 7] {
    SPENT.with(Cell::get)
}

/// Seconds so far in each stage, starting again from nothing.
pub(crate) fn take() -> [f64; 7] {
    SPENT.with(|spent| spent.replace([0.0; 7]))
}

/// Runs the game's `draw` (`work`): what isn't 3D in it is the overlay.
pub(crate) fn drawing<R>(work: impl FnOnce() -> R) -> R {
    let before = so_far();
    let start = Instant::now();
    let result = work();
    let total = start.elapsed().as_secs_f64();
    let after = so_far();
    let three_d: f64 = THREE_D
        .iter()
        .map(|&s| after[s as usize] - before[s as usize])
        .sum();
    add(Stage::Overlay, (total - three_d).max(0.0));
    result
}

/// The stages as one line for the summary: "updates 0.40 s, ...".
pub(crate) fn words(spent: &[f64; 7]) -> String {
    NAMES
        .iter()
        .zip(spent)
        .filter(|(_, s)| **s > 0.0)
        .map(|(name, s)| format!("{name} {s:.2} s"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_add_up_and_the_overlay_is_what_is_left() {
        take();
        time(Stage::Update, || {
            std::thread::sleep(std::time::Duration::from_millis(5))
        });
        drawing(|| {
            time(Stage::World, || {
                std::thread::sleep(std::time::Duration::from_millis(20))
            });
            std::thread::sleep(std::time::Duration::from_millis(10)); // 2D on top
        });
        let spent = take();
        assert!(spent[Stage::Update as usize] >= 0.005);
        assert!(spent[Stage::World as usize] >= 0.02);
        let overlay = spent[Stage::Overlay as usize];
        assert!((0.01..0.019).contains(&overlay), "{overlay}");
        assert_eq!(so_far(), [0.0; 7], "taken");
        let line = words(&spent);
        assert!(
            line.starts_with("updates 0.0")
                && line.contains(", world 0.0")
                && !line.contains("bloom"),
            "{line}"
        );
    }
}

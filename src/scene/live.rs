//! Changes to objects as they are now, made from outside the game (Duck
//! Forge Studio dragging an object "for this run only"), on their way to
//! the scene they're for.
//!
//! A scene is a value the game holds, so the engine can't reach it. It
//! leaves the change here, then calls the game's `reload` with the scene's
//! file; [`Scene::reload`](super::Scene::reload) picks up what's waiting
//! for its file. What's waiting belongs to the thread that left it: the
//! engine runs a game, and its tests, one thread each.

use std::cell::RefCell;

use serde_json::{Map, Value};

thread_local! {
    static WAITING: RefCell<Vec<(String, u64, Map<String, Value>)>> = const { RefCell::new(Vec::new()) };
}

/// Leaves a change for the object `id` of the scene read from `file`: the
/// keys of its line to set, as they are now.
pub(crate) fn leave(file: &str, id: u64, set: &Map<String, Value>) {
    WAITING.with(|waiting| {
        waiting
            .borrow_mut()
            .push((file.to_string(), id, set.clone()));
    });
}

/// Takes the changes waiting for the scene read from `file`.
pub(crate) fn take_for(file: &str) -> Vec<(u64, Map<String, Value>)> {
    WAITING.with(|waiting| {
        let mut waiting = waiting.borrow_mut();
        let mut mine = Vec::new();
        waiting.retain(|(for_file, id, set)| {
            if for_file == file {
                mine.push((*id, set.clone()));
                false
            } else {
                true
            }
        });
        mine
    })
}

/// Forgets what nobody took (a game with no such scene).
pub(crate) fn forget() {
    WAITING.with(|waiting| waiting.borrow_mut().clear());
}

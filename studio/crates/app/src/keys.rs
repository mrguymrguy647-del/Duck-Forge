//! Turning what egui saw (keys, the mouse) into the game's inputs: the
//! same ones an input script holds.

use duckforge::InputAction;
use duckforge::prelude::{Key, MouseButton, vec2};
use eframe::egui;

/// The game's key for one of egui's, if the game has it. Tab isn't here:
/// it's the Studio's own key (Build and Play).
pub fn key(key: egui::Key) -> Option<Key> {
    use egui::Key as E;
    match key {
        E::Tab => None,
        E::ArrowLeft => Some(Key::Left),
        E::ArrowRight => Some(Key::Right),
        E::ArrowUp => Some(Key::Up),
        E::ArrowDown => Some(Key::Down),
        E::Space => Some(Key::Space),
        E::Enter => Some(Key::Enter),
        E::Escape => Some(Key::Escape),
        E::Backspace => Some(Key::Backspace),
        other => {
            // Letters are "A", number keys "0", function keys "F5".
            let name = other.name();
            if name.len() == 1 && name.as_bytes()[0].is_ascii_digit() {
                Key::from_name(&format!("Digit{name}"))
            } else if name.len() == 1 || name.starts_with('F') {
                Key::from_name(name)
            } else {
                None
            }
        }
    }
}

/// The game's mouse button for one of egui's.
pub fn button(button: egui::PointerButton) -> Option<MouseButton> {
    match button {
        egui::PointerButton::Primary => Some(MouseButton::Left),
        egui::PointerButton::Secondary => Some(MouseButton::Right),
        egui::PointerButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Where the game is on screen, for turning the pointer's place into game
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The game's picture, in egui's points.
    pub rect: egui::Rect,
    /// Screen pixels per game pixel (a whole number).
    pub scale: f32,
    /// Screen pixels per egui point.
    pub pixels_per_point: f32,
}

impl Placement {
    /// A place on screen (in points) as a place in the game (in game pixels).
    pub fn to_game(&self, place: egui::Pos2) -> duckforge::prelude::Vec2 {
        let offset = (place - self.rect.min) * self.pixels_per_point / self.scale;
        vec2(offset.x, offset.y)
    }
}

/// What changed in the Shift, Ctrl and Alt keys. egui reports them as a
/// state rather than as key presses, so the game gets the left one of each.
pub fn modifier_changes(before: egui::Modifiers, now: egui::Modifiers) -> Vec<InputAction> {
    let mut actions = Vec::new();
    for (was, is, key) in [
        (before.shift, now.shift, Key::LeftShift),
        (before.ctrl, now.ctrl, Key::LeftCtrl),
        (before.alt, now.alt, Key::LeftAlt),
    ] {
        if is && !was {
            actions.push(InputAction::Press(key));
        } else if was && !is {
            actions.push(InputAction::Release(key));
        }
    }
    actions
}

/// This frame's events as inputs for the game. `keyboard` is false while
/// a text box has the keyboard; `held` is true while the Studio holds the
/// pointer for the game (mouse look).
pub fn game_inputs(
    events: &[egui::Event],
    placement: Option<Placement>,
    pointer: Option<egui::Pos2>,
    keyboard: bool,
    held: bool,
) -> Vec<InputAction> {
    let mut actions = Vec::new();
    for event in events {
        match event {
            egui::Event::Key {
                key: logical,
                physical_key,
                pressed,
                repeat: false,
                ..
            } if keyboard => {
                // The key's place on the keyboard, so WASD is WASD on
                // every layout, as in the game's own window.
                if let Some(key) = key(physical_key.unwrap_or(*logical)) {
                    actions.push(if *pressed {
                        InputAction::Press(key)
                    } else {
                        InputAction::Release(key)
                    });
                }
            }
            egui::Event::PointerMoved(place) if !held => {
                if let Some(placement) = placement.filter(|p| p.rect.contains(*place)) {
                    actions.push(InputAction::MouseMove(placement.to_game(*place)));
                }
            }
            egui::Event::MouseMoved(amount) if held => {
                if let Some(placement) = placement {
                    let amount = *amount / placement.scale;
                    actions.push(InputAction::MouseMotion(vec2(amount.x, amount.y)));
                }
            }
            egui::Event::PointerButton {
                pos,
                button: which,
                pressed,
                ..
            } => {
                let Some(which) = button(*which) else {
                    continue;
                };
                let inside = held || placement.is_some_and(|p| p.rect.contains(*pos));
                if *pressed && inside {
                    actions.push(InputAction::MousePress(which));
                } else if !*pressed {
                    actions.push(InputAction::MouseRelease(which));
                }
            }
            egui::Event::MouseWheel { unit, delta, .. } => {
                // The wheel only counts over the game.
                let inside = held
                    || placement.is_some_and(|p| pointer.is_some_and(|at| p.rect.contains(at)));
                let notches = match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / 50.0,
                    egui::MouseWheelUnit::Page => delta.y * 10.0,
                };
                if inside && notches != 0.0 {
                    actions.push(InputAction::Scroll(notches));
                }
            }
            _ => {}
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_the_games_keys_and_tab_stays_the_studios() {
        assert_eq!(key(egui::Key::A), Some(Key::A));
        assert_eq!(key(egui::Key::Num7), Some(Key::Digit7));
        assert_eq!(key(egui::Key::F5), Some(Key::F5));
        assert_eq!(key(egui::Key::ArrowUp), Some(Key::Up));
        assert_eq!(key(egui::Key::Space), Some(Key::Space));
        assert_eq!(key(egui::Key::Escape), Some(Key::Escape));
        assert_eq!(key(egui::Key::Tab), None);
        assert_eq!(key(egui::Key::Home), None, "a key the games don't have");
    }

    #[test]
    fn the_pointer_is_given_in_game_pixels() {
        // A 320 x 240 game shown 3 times bigger on a 150% screen: each
        // game pixel is 3 screen pixels, which is 2 points.
        let placement = Placement {
            rect: egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(640.0, 480.0)),
            scale: 3.0,
            pixels_per_point: 1.5,
        };
        assert_eq!(placement.to_game(egui::pos2(100.0, 50.0)), vec2(0.0, 0.0));
        assert_eq!(
            placement.to_game(egui::pos2(420.0, 290.0)),
            vec2(160.0, 120.0)
        );
        let moved = [egui::Event::PointerMoved(egui::pos2(420.0, 290.0))];
        assert_eq!(
            game_inputs(&moved, Some(placement), None, true, false),
            [InputAction::MouseMove(vec2(160.0, 120.0))]
        );
        // Off the game: nothing.
        let off = [egui::Event::PointerMoved(egui::pos2(10.0, 10.0))];
        assert!(game_inputs(&off, Some(placement), None, true, false).is_empty());
        // Held for mouse look: raw movement, in game pixels.
        let raw = [egui::Event::MouseMoved(egui::vec2(6.0, -3.0))];
        assert_eq!(
            game_inputs(&raw, Some(placement), None, true, true),
            [InputAction::MouseMotion(vec2(2.0, -1.0))]
        );
        assert!(game_inputs(&raw, Some(placement), None, true, false).is_empty());
    }

    #[test]
    fn shift_ctrl_and_alt_become_the_left_keys() {
        let none = egui::Modifiers::NONE;
        let shift = egui::Modifiers::SHIFT;
        assert_eq!(
            modifier_changes(none, shift),
            [InputAction::Press(Key::LeftShift)]
        );
        assert_eq!(
            modifier_changes(shift, none),
            [InputAction::Release(Key::LeftShift)]
        );
        assert!(modifier_changes(shift, shift).is_empty());
    }
}

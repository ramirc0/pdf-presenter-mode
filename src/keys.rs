//! Key bindings, shared by every window.

use eframe::egui::{self, Key};

use crate::talk::Action;

/// The actions for the keys pressed this frame, in order.
pub fn actions(input: &egui::InputState) -> Vec<Action> {
    input
        .events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Key {
                key, pressed: true, ..
            } => action_for(*key),
            _ => None,
        })
        .collect()
}

fn action_for(key: Key) -> Option<Action> {
    match key {
        Key::ArrowRight | Key::ArrowDown | Key::Space | Key::PageDown | Key::L | Key::J => {
            Some(Action::Next)
        }
        Key::ArrowLeft | Key::ArrowUp | Key::PageUp | Key::H | Key::K => Some(Action::Previous),
        Key::Home => Some(Action::First),
        Key::End => Some(Action::Last),
        Key::X => Some(Action::Swap),
        Key::Q => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicker_keys_navigate() {
        assert_eq!(action_for(Key::PageDown), Some(Action::Next));
        assert_eq!(action_for(Key::PageUp), Some(Action::Previous));
    }

    #[test]
    fn vim_keys_navigate() {
        for key in [Key::J, Key::L, Key::ArrowDown] {
            assert_eq!(action_for(key), Some(Action::Next), "{key:?}");
        }
        for key in [Key::H, Key::K, Key::ArrowUp] {
            assert_eq!(action_for(key), Some(Action::Previous), "{key:?}");
        }
    }

    #[test]
    fn x_swaps_the_windows() {
        assert_eq!(action_for(Key::X), Some(Action::Swap));
    }

    #[test]
    fn escape_does_nothing() {
        assert_eq!(action_for(Key::Escape), None);
    }
}

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
        Key::ArrowRight | Key::Space | Key::PageDown => Some(Action::Next),
        Key::ArrowLeft | Key::PageUp => Some(Action::Previous),
        Key::Home => Some(Action::First),
        Key::End => Some(Action::Last),
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
    fn escape_does_nothing() {
        assert_eq!(action_for(Key::Escape), None);
    }
}

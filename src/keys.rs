//! Key bindings, shared by every window, and the shortcut hint built from them.

use eframe::egui::{self, Key};

use crate::talk::Action;

/// Keys that trigger one action, with or without Alt held.
pub struct Binding {
    /// Any of these keys triggers the action.
    pub keys: &'static [Key],
    /// Whether Alt must be held. Keys without Alt are for navigation and core app controls.
    pub alt: bool,
    /// What the keys do.
    pub action: Action,
    /// What the shortcut hint says the keys do.
    pub does: &'static str,
}

/// Every binding, in the order the shortcut hint lists them.
pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: &[
            Key::ArrowRight,
            Key::ArrowDown,
            Key::Space,
            Key::PageDown,
            Key::L,
            Key::J,
        ],
        alt: false,
        action: Action::Next,
        does: "next slide",
    },
    Binding {
        keys: &[Key::ArrowLeft, Key::ArrowUp, Key::PageUp, Key::H, Key::K],
        alt: false,
        action: Action::Previous,
        does: "previous slide",
    },
    Binding {
        keys: &[Key::Home],
        alt: false,
        action: Action::First,
        does: "first slide",
    },
    Binding {
        keys: &[Key::End],
        alt: false,
        action: Action::Last,
        does: "last slide",
    },
    Binding {
        keys: &[Key::G],
        alt: false,
        action: Action::GoTo,
        does: "go to a slide: type its number, then Enter",
    },
    Binding {
        keys: &[Key::Enter],
        alt: false,
        action: Action::Confirm,
        does: "confirm",
    },
    Binding {
        keys: &[Key::Backspace],
        alt: false,
        action: Action::Backspace,
        does: "delete a digit",
    },
    Binding {
        keys: &[Key::B, Key::Period],
        alt: false,
        action: Action::Blank,
        does: "blank the audience, or show it again",
    },
    Binding {
        keys: &[Key::F],
        alt: false,
        action: Action::Fullscreen,
        does: "toggle fullscreen",
    },
    Binding {
        keys: &[Key::Questionmark],
        alt: false,
        action: Action::Help,
        does: "show or hide these shortcuts",
    },
    Binding {
        keys: &[Key::Escape],
        alt: false,
        action: Action::Cancel,
        does: "close",
    },
    Binding {
        keys: &[Key::Q],
        alt: false,
        action: Action::Quit,
        does: "quit, when pressed twice",
    },
    Binding {
        keys: &[Key::F],
        alt: true,
        action: Action::Freeze,
        does: "freeze the audience, or let it follow again",
    },
    Binding {
        keys: &[Key::S],
        alt: true,
        action: Action::StartTimer,
        does: "start the timer",
    },
    Binding {
        keys: &[Key::P],
        alt: true,
        action: Action::PauseTimer,
        does: "pause or resume the timer",
    },
    Binding {
        keys: &[Key::R],
        alt: true,
        action: Action::ResetTimer,
        does: "reset the timer",
    },
    Binding {
        keys: &[Key::V],
        alt: true,
        action: Action::Layout,
        does: "switch the presenter layout",
    },
    Binding {
        keys: &[Key::Plus, Key::Equals],
        alt: true,
        action: Action::BiggerNotes,
        does: "larger notes",
    },
    Binding {
        keys: &[Key::Minus],
        alt: true,
        action: Action::SmallerNotes,
        does: "smaller notes",
    },
    Binding {
        keys: &[Key::X],
        alt: true,
        action: Action::Swap,
        does: "swap the two windows' views",
    },
];

/// The digit keys, in order from 0. They type a slide number after `g`.
const DIGITS: [Key; 10] = [
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

/// The actions for the keys pressed this frame, in order.
pub fn actions(input: &egui::InputState) -> Vec<Action> {
    input
        .events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => action_for(*key, *modifiers),
            _ => None,
        })
        .collect()
}

/// The action bound to `key` under `modifiers`. Shift is ignored, since `?` and `+` need it.
/// Ctrl combinations belong to the system.
fn action_for(key: Key, modifiers: egui::Modifiers) -> Option<Action> {
    if modifiers.ctrl || modifiers.command || modifiers.mac_cmd {
        return None;
    }
    if !modifiers.alt
        && let Some(digit) = DIGITS.iter().position(|&d| d == key)
    {
        return Some(Action::Digit(digit as u8));
    }
    BINDINGS
        .iter()
        .find(|binding| binding.alt == modifiers.alt && binding.keys.contains(&key))
        .map(|binding| binding.action)
}

/// How the shortcut hint names `key`: `l` for a letter, `Right` for an arrow, `?` for a sign.
pub fn label(key: Key) -> String {
    let name = key.symbol_or_name();
    match key {
        Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp => key.name().into(),
        _ if name.len() == 1 => name.to_lowercase(),
        _ => name.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALT: egui::Modifiers = egui::Modifiers::ALT;
    const NONE: egui::Modifiers = egui::Modifiers::NONE;

    #[test]
    fn clicker_keys_navigate() {
        assert_eq!(action_for(Key::PageDown, NONE), Some(Action::Next));
        assert_eq!(action_for(Key::PageUp, NONE), Some(Action::Previous));
    }

    #[test]
    fn vim_keys_navigate() {
        for key in [Key::J, Key::L, Key::ArrowDown] {
            assert_eq!(action_for(key, NONE), Some(Action::Next), "{key:?}");
        }
        for key in [Key::H, Key::K, Key::ArrowUp] {
            assert_eq!(action_for(key, NONE), Some(Action::Previous), "{key:?}");
        }
    }

    #[test]
    fn digits_type_numbers() {
        assert_eq!(action_for(Key::Num7, NONE), Some(Action::Digit(7)));
        assert_eq!(action_for(Key::Num7, ALT), None);
    }

    #[test]
    fn swap_needs_alt() {
        assert_eq!(action_for(Key::X, NONE), None);
        assert_eq!(action_for(Key::X, ALT), Some(Action::Swap));
    }

    #[test]
    fn alt_does_not_reach_the_plain_layer() {
        assert_eq!(action_for(Key::F, NONE), Some(Action::Fullscreen));
        assert_eq!(action_for(Key::Space, ALT), None);
    }

    #[test]
    fn shift_is_ignored_and_ctrl_is_not() {
        assert_eq!(
            action_for(Key::Questionmark, egui::Modifiers::SHIFT),
            Some(Action::Help)
        );
        assert_eq!(action_for(Key::Q, egui::Modifiers::CTRL), None);
    }

    #[test]
    fn no_key_is_bound_twice_in_one_layer() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in &BINDINGS[i + 1..] {
                let shared = a.keys.iter().find(|key| b.keys.contains(key));
                assert!(a.alt != b.alt || shared.is_none(), "{shared:?}");
            }
        }
    }

    #[test]
    fn labels_read_like_the_keyboard() {
        assert_eq!(label(Key::L), "l");
        assert_eq!(label(Key::ArrowRight), "Right");
        assert_eq!(label(Key::Questionmark), "?");
        assert_eq!(label(Key::PageDown), "PageDown");
    }
}

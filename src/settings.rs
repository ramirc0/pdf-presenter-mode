//! Presenter preferences that persist between runs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How the presenter view arranges its panels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// The current slide is largest. The next slide and the notes share a side column.
    #[default]
    SlidesFirst,
    /// The notes are largest. The current and next slides share a side column.
    NotesFirst,
}

impl Layout {
    /// The other layout.
    pub fn other(self) -> Self {
        match self {
            Layout::SlidesFirst => Layout::NotesFirst,
            Layout::NotesFirst => Layout::SlidesFirst,
        }
    }
}

/// Where a layout divides the window, as shares, so it fits any window size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Splits {
    /// The left column's share of the width.
    pub columns: f32,
    /// The top panel's share of the side column's height.
    pub rows: f32,
}

/// Preferences saved with eframe's storage.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The presenter layout.
    pub layout: Layout,
    /// How much larger notes are than egui's default text.
    pub notes_scale: f32,
    /// The splits of the slides-first layout.
    pub slides_first: Splits,
    /// The splits of the notes-first layout.
    pub notes_first: Splits,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            notes_scale: 1.75,
            slides_first: Splits {
                columns: 0.65,
                rows: 0.45,
            },
            notes_first: Splits {
                columns: 0.4,
                rows: 0.55,
            },
        }
    }
}

const KEY: &str = "settings";

impl Settings {
    /// The saved settings, or the defaults if there are none.
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|storage| eframe::get_value(storage, KEY))
            .unwrap_or_default()
    }

    /// Saves the settings to `storage`.
    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, KEY, self);
    }

    /// The splits of the current layout.
    pub fn splits_mut(&mut self) -> &mut Splits {
        match self.layout {
            Layout::SlidesFirst => &mut self.slides_first,
            Layout::NotesFirst => &mut self.notes_first,
        }
    }

    /// The notes one step larger, up to 5 times egui's default text.
    pub fn bigger_notes(&mut self) {
        self.notes_scale = (self.notes_scale * 1.1).min(5.0);
    }

    /// The notes one step smaller, down to 0.75 times egui's default text.
    pub fn smaller_notes(&mut self) {
        self.notes_scale = (self.notes_scale / 1.1).max(0.75);
    }
}

/// The decks presented last, newest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recent(Vec<PathBuf>);

const RECENT: &str = "recent";

/// How many decks [`Recent`] keeps.
const RECENT_DECKS: usize = 5;

impl Recent {
    /// The saved decks that still exist, or none.
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        let Self(mut decks) = storage
            .and_then(|storage| eframe::get_value(storage, RECENT))
            .unwrap_or_default();
        decks.retain(|deck| deck.exists());
        Self(decks)
    }

    /// Saves the decks to `storage`.
    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, RECENT, self);
    }

    /// Puts `deck` first, once, and forgets the oldest beyond five.
    pub fn add(&mut self, deck: &Path) {
        let deck = std::fs::canonicalize(deck).unwrap_or_else(|_| deck.to_path_buf());
        self.0.retain(|known| *known != deck);
        self.0.insert(0, deck);
        self.0.truncate(RECENT_DECKS);
    }

    /// The decks, newest first.
    pub fn decks(&self) -> &[PathBuf] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_decks_come_newest_first_once_and_five_at_most() {
        let dir = std::env::temp_dir().join(format!("ppm-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut recent = Recent::default();
        for n in 0..7 {
            recent.add(&dir.join(format!("{n}.pdf")));
        }
        recent.add(&dir.join("4.pdf"));
        let names: Vec<_> = recent
            .decks()
            .iter()
            .map(|deck| deck.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["4.pdf", "6.pdf", "5.pdf", "3.pdf", "2.pdf"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recent_deck_is_kept_by_its_full_path() {
        let mut recent = Recent::default();
        recent.add(Path::new("fixtures/basic.pdf"));
        recent.add(&std::env::current_dir().unwrap().join("fixtures/basic.pdf"));
        assert_eq!(recent.decks().len(), 1);
        assert!(recent.decks()[0].is_absolute());
    }

    #[test]
    fn missing_recent_decks_are_forgotten_on_load() {
        let mut storage = Memory::default();
        let mut recent = Recent::default();
        recent.add(Path::new("fixtures/basic.pdf"));
        recent.add(Path::new("fixtures/gone.pdf"));
        recent.save(&mut storage);
        let loaded = Recent::load(Some(&storage));
        assert_eq!(loaded.decks().len(), 1);
        assert!(loaded.decks()[0].ends_with("fixtures/basic.pdf"));
    }

    /// Storage in memory, for tests.
    #[derive(Default)]
    struct Memory(std::collections::HashMap<String, String>);

    impl eframe::Storage for Memory {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }

        fn flush(&mut self) {}
    }

    #[test]
    fn notes_size_stays_in_range() {
        let mut settings = Settings::default();
        for _ in 0..100 {
            settings.bigger_notes();
        }
        assert_eq!(settings.notes_scale, 5.0);
        for _ in 0..100 {
            settings.smaller_notes();
        }
        assert_eq!(settings.notes_scale, 0.75);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"layout": "NotesFirst"}"#).unwrap();
        assert_eq!(settings.layout, Layout::NotesFirst);
        assert_eq!(settings.notes_scale, Settings::default().notes_scale);
    }
}

//! Presenter preferences that persist between runs.

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

#[cfg(test)]
mod tests {
    use super::*;

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

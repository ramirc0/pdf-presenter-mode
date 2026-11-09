//! The UI font.

use std::sync::Arc;

use eframe::egui;

/// ProggyForever by Omar Cornut, under the MIT license in `assets/fonts/ProggyForever-LICENSE.txt`.
const PROGGY_FOREVER: &[u8] = include_bytes!("../assets/fonts/ProggyForever-Regular.ttf");

/// egui's fonts with ProggyForever first in every family. The defaults remain as fallbacks
/// for glyphs ProggyForever lacks.
fn definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "ProggyForever".into(),
        Arc::new(egui::FontData::from_static(PROGGY_FOREVER)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "ProggyForever".into());
    }
    fonts
}

/// Makes ProggyForever the UI font of `ctx`.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proggy_forever_comes_first_with_the_defaults_behind_it() {
        let fonts = definitions();
        let defaults = egui::FontDefinitions::default();
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let names = &fonts.families[&family];
            assert_eq!(names[0], "ProggyForever");
            assert_eq!(&names[1..], &defaults.families[&family][..]);
        }
    }

    #[test]
    fn the_embedded_font_is_a_truetype_file() {
        assert_eq!(&PROGGY_FOREVER[..4], &[0, 1, 0, 0]);
    }
}

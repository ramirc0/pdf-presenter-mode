//! The speaker's window.

use eframe::egui;

use crate::deck::Deck;
use crate::keys;
use crate::render::{Renderer, place};
use crate::talk::{Outcome, Talk};

/// The presenter window: for now, the current slide, letterboxed on black.
pub struct Presenter {
    deck: Deck,
    talk: Talk,
    renderer: Renderer,
}

impl Presenter {
    /// Presents `deck` from its first slide, rendering through `renderer`.
    pub fn new(deck: Deck, renderer: Renderer) -> Self {
        let talk = Talk::new(deck.pages.len());
        Self {
            deck,
            talk,
            renderer,
        }
    }
}

impl eframe::App for Presenter {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        for action in ui.input(keys::actions) {
            if self.talk.apply(action) == Outcome::Quit {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        let area = ui.max_rect();
        let painter = ui.painter();
        painter.rect_filled(area, 0.0, egui::Color32::BLACK);
        let pixels_per_point = ui.ctx().pixels_per_point();
        let placed = |index: usize| place(index, self.deck.pages[index], area, pixels_per_point);

        let current = (self.talk.slides() > 0)
            .then(|| placed(self.talk.slide()))
            .flatten();
        let next = self.talk.next_slide().and_then(placed);
        self.renderer
            .request(current.iter().chain(&next).map(|p| p.target).collect());

        if let Some(current) = current
            && let Some(texture) = self.renderer.texture(current.target)
        {
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
            painter.image(texture.id(), current.rect, uv, egui::Color32::WHITE);
        }
        if self.talk.quit_armed() {
            painter.text(
                area.left_bottom() + egui::vec2(16.0, -16.0),
                egui::Align2::LEFT_BOTTOM,
                "Press q again to quit",
                egui::FontId::proportional(18.0),
                egui::Color32::LIGHT_GRAY,
            );
        }
    }
}

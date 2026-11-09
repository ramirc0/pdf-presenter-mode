//! What both windows share: the talk, the renderer and where each window draws.

use std::sync::Arc;

use eframe::egui;

use crate::deck::PageSize;
use crate::keys;
use crate::pdfpc::Notes;
use crate::render::{Renderer, Target, place};
use crate::talk::{Outcome, Talk};

/// One of the app's two windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// The root window, on the laptop by default.
    Main,
    /// The second window, fullscreen on the projector by default.
    Projector,
}

/// What a window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The speaker's view.
    Presenter,
    /// The slide alone, for the room.
    Audience,
}

/// Where a window draws, in points, and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    /// The drawable rect in points.
    pub rect: egui::Rect,
    /// Physical pixels per point.
    pub pixels_per_point: f32,
}

/// Repaints every window. The render worker and key handling share it.
pub type Repaint = Arc<dyn Fn() + Send + Sync>;

/// Shared state of both windows.
pub struct Stage {
    pages: Vec<PageSize>,
    notes: Notes,
    talk: Talk,
    renderer: Renderer,
    areas: [Option<Area>; 2],
    repaint: Repaint,
}

impl Stage {
    /// Presents `pages` from the first slide, with `notes` in the presenter view.
    /// `repaint` must repaint both windows.
    pub fn new(pages: Vec<PageSize>, notes: Notes, renderer: Renderer, repaint: Repaint) -> Self {
        let talk = Talk::new(pages.len());
        Self {
            pages,
            notes,
            talk,
            renderer,
            areas: [None, None],
            repaint,
        }
    }

    /// Applies the keys pressed in the window `ui` belongs to.
    pub fn handle_keys(&mut self, ui: &egui::Ui) -> Outcome {
        let actions = ui.input(keys::actions);
        if actions.is_empty() {
            return Outcome::Continue;
        }
        (self.repaint)();
        for action in actions {
            if self.talk.apply(action) == Outcome::Quit {
                return Outcome::Quit;
            }
        }
        Outcome::Continue
    }

    /// Stops rendering for `window`, which has closed.
    pub fn close(&mut self, window: Window) {
        self.areas[window as usize] = None;
    }

    /// Draws `window`'s view into `ui`.
    pub fn draw(&mut self, ui: &mut egui::Ui, window: Window) {
        let view = view_of(window, self.talk.swapped());
        let window_rect = ui.max_rect();
        let (slide_rect, notes_rect) = if view == View::Presenter && !self.notes.is_empty() {
            let (slide, notes) = window_rect.split_top_bottom_at_fraction(0.6);
            (slide, Some(notes))
        } else {
            (window_rect, None)
        };
        let area = Area {
            rect: slide_rect,
            pixels_per_point: ui.ctx().pixels_per_point(),
        };
        self.areas[window as usize] = Some(area);
        let wanted = plan(&self.pages, &self.talk, self.areas.iter().flatten());
        self.renderer.request(wanted);

        ui.painter()
            .rect_filled(window_rect, 0.0, egui::Color32::BLACK);
        if let Some(rect) = notes_rect
            && let Some(note) = self.notes.on_page(self.talk.slide())
        {
            let mut notes_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(16.0)));
            egui::ScrollArea::vertical().show(&mut notes_ui, |ui| {
                ui.label(
                    egui::RichText::new(note)
                        .size(22.0)
                        .color(egui::Color32::WHITE),
                );
            });
        }
        let painter = ui.painter();
        if view == View::Audience {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }
        let current = (self.talk.slides() > 0)
            .then(|| place_in(&self.pages, self.talk.slide(), area))
            .flatten();
        if let Some(current) = current
            && let Some(texture) = self.renderer.texture(current.target)
        {
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
            painter.image(texture.id(), current.rect, uv, egui::Color32::WHITE);
        }
        if view == View::Presenter && self.talk.quit_armed() {
            // A dark backing keeps the hint readable over a white slide.
            let galley = painter.layout_no_wrap(
                "Press q again to quit".into(),
                egui::FontId::proportional(18.0),
                egui::Color32::WHITE,
            );
            let origin = area.rect.left_bottom() + egui::vec2(16.0, -16.0 - galley.size().y);
            let backing = egui::Rect::from_min_size(origin, galley.size()).expand(6.0);
            painter.rect_filled(backing, 4.0, egui::Color32::from_black_alpha(200));
            painter.galley(origin, galley, egui::Color32::WHITE);
        }
    }
}

/// The view `window` shows. A swap exchanges the two.
fn view_of(window: Window, swapped: bool) -> View {
    match (window, swapped) {
        (Window::Main, false) | (Window::Projector, true) => View::Presenter,
        (Window::Main, true) | (Window::Projector, false) => View::Audience,
    }
}

fn place_in(pages: &[PageSize], index: usize, area: Area) -> Option<crate::render::Placement> {
    place(index, pages[index], area.rect, area.pixels_per_point)
}

/// The renders the windows need, most urgent first: every on-screen slide,
/// then the next slide at each window's size.
fn plan<'a>(
    pages: &[PageSize],
    talk: &Talk,
    areas: impl Iterator<Item = &'a Area> + Clone,
) -> Vec<Target> {
    let mut wanted = Vec::new();
    let slides = [Some(talk.slide()), talk.next_slide()];
    for slide in slides.into_iter().flatten().filter(|&s| s < pages.len()) {
        for area in areas.clone() {
            if let Some(placed) = place_in(pages, slide, *area)
                && !wanted.contains(&placed.target)
            {
                wanted.push(placed.target);
            }
        }
    }
    wanted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::talk::Action;

    fn area(width: f32, height: f32) -> Area {
        Area {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height)),
            pixels_per_point: 1.0,
        }
    }

    fn deck(slides: usize) -> Vec<PageSize> {
        vec![
            PageSize {
                width: 160.0,
                height: 90.0
            };
            slides
        ]
    }

    #[test]
    fn on_screen_slides_come_before_the_next_slide() {
        let talk = Talk::new(3);
        let areas = [area(1920.0, 1080.0), area(640.0, 360.0)];
        let wanted = plan(&deck(3), &talk, areas.iter());
        let summary: Vec<_> = wanted.iter().map(|t| (t.page, t.width)).collect();
        assert_eq!(summary, [(0, 1920), (0, 640), (1, 1920), (1, 640)]);
    }

    #[test]
    fn the_last_slide_has_nothing_to_prefetch() {
        let mut talk = Talk::new(2);
        talk.apply(Action::Last);
        let areas = [area(1920.0, 1080.0)];
        let wanted = plan(&deck(2), &talk, areas.iter());
        assert_eq!(wanted.len(), 1);
        assert_eq!(wanted[0].page, 1);
    }

    #[test]
    fn same_sized_windows_share_renders() {
        let talk = Talk::new(2);
        let areas = [area(800.0, 450.0), area(800.0, 450.0)];
        assert_eq!(plan(&deck(2), &talk, areas.iter()).len(), 2);
    }

    #[test]
    fn an_empty_deck_needs_nothing() {
        let talk = Talk::new(0);
        let areas = [area(800.0, 450.0)];
        assert!(plan(&[], &talk, areas.iter()).is_empty());
    }

    #[test]
    fn swap_exchanges_the_views() {
        assert_eq!(view_of(Window::Main, false), View::Presenter);
        assert_eq!(view_of(Window::Projector, false), View::Audience);
        assert_eq!(view_of(Window::Main, true), View::Audience);
        assert_eq!(view_of(Window::Projector, true), View::Presenter);
    }
}

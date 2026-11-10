//! What both windows share: the talk, the renderer and where each window draws.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::deck::{Deck, PageSize};
use crate::keys;
use crate::pdfpc::Notes;
use crate::render::{Placement, Renderer, Target, place};
use crate::settings::{Layout, Settings};
use crate::talk::{Action, Outcome, Talk};
use crate::timer::{Urgency, clock_face};

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

/// Repaints every window. The render worker and key handling share it.
pub type Repaint = Arc<dyn Fn() + Send + Sync>;

/// Shared state of both windows.
pub struct Stage {
    pages: Vec<PageSize>,
    notes: Notes,
    markdown: CommonMarkCache,
    talk: Talk,
    renderer: Renderer,
    /// Decks reopened after their files changed.
    reloads: Receiver<Deck>,
    /// The renders each window drew in its last frame, most important first.
    shown: [Vec<Target>; 2],
    /// The overview thumbnails each window drew in its last frame. They render last.
    thumbnails: [Vec<Target>; 2],
    repaint: Repaint,
}

impl Stage {
    /// Presents `deck` from the first slide, with the presenter view arranged by `settings`.
    /// Each deck from `reloads` replaces the current one. Textures go to `ctx`, and
    /// `repaint` must repaint both windows.
    pub fn new(
        deck: Deck,
        settings: Settings,
        reloads: Receiver<Deck>,
        ctx: egui::Context,
        repaint: Repaint,
    ) -> Self {
        let talk = Talk::new(deck.pages.len(), settings);
        Self {
            pages: deck.pages,
            notes: deck.notes,
            markdown: CommonMarkCache::default(),
            talk,
            renderer: Renderer::new(ctx, deck.pdf, repaint.clone()),
            reloads,
            shown: [Vec::new(), Vec::new()],
            thumbnails: [Vec::new(), Vec::new()],
            repaint,
        }
    }

    /// The presenter preferences, to save on exit.
    pub fn settings(&self) -> Settings {
        self.talk.settings()
    }

    /// Applies the keys pressed in the window `ui` belongs to.
    pub fn handle_keys(&mut self, ui: &egui::Ui) -> Outcome {
        let actions = ui.input(keys::actions);
        if actions.is_empty() {
            return Outcome::Continue;
        }
        (self.repaint)();
        for action in actions {
            if action == Action::Fullscreen {
                let fullscreen = ui.input(|input| input.viewport().fullscreen.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            if self.talk.apply(action, Instant::now()) == Outcome::Quit {
                return Outcome::Quit;
            }
        }
        Outcome::Continue
    }

    /// Stops rendering for `window`, which has closed.
    pub fn close(&mut self, window: Window) {
        self.shown[window as usize].clear();
        self.thumbnails[window as usize].clear();
    }

    /// Draws `window`'s view into `ui`.
    pub fn draw(&mut self, ui: &mut egui::Ui, window: Window) {
        if let Some(deck) = self.reloads.try_iter().last() {
            self.talk.reload(deck.pages.len());
            self.pages = deck.pages;
            self.notes = deck.notes;
            // A new renderer has no textures, so no page of the old deck can show.
            self.renderer = Renderer::new(ui.ctx().clone(), deck.pdf, self.repaint.clone());
        }
        self.close(window);
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::BLACK);
        match view_of(window, self.talk.swapped()) {
            View::Audience => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                self.show_slide(ui, window, self.talk.audience_slide(), ui.max_rect());
            }
            View::Presenter => self.draw_presenter(ui, window),
        }
        let wanted = plan(
            self.pages.len(),
            self.shown.iter().flatten(),
            self.thumbnails.iter().flatten(),
        );
        self.renderer.request(wanted);
    }

    fn draw_presenter(&mut self, ui: &mut egui::Ui, window: Window) {
        egui::Panel::bottom("status")
            .frame(egui::Frame::NONE.inner_margin(8))
            .show(ui, |ui| self.show_status(ui));
        let body = ui.available_rect_before_wrap();
        if let Some(selected) = self.talk.overview() {
            self.show_overview(ui, window, body, selected);
        } else {
            self.show_panels(ui, window, body);
        }
        if self.talk.help() {
            show_shortcuts(ui);
        }
    }

    /// The current and next slides and the notes, arranged by the layout.
    fn show_panels(&mut self, ui: &mut egui::Ui, window: Window, body: egui::Rect) {
        let layout = self.talk.settings().layout;
        let splits = self.talk.settings_mut().splits_mut();
        let (left, right) = split(ui, "columns", body, &mut splits.columns, true);
        let side = match layout {
            Layout::SlidesFirst => right,
            Layout::NotesFirst => left,
        };
        let (top, bottom) = split(ui, "rows", side, &mut splits.rows, false);
        let [left, right, top, bottom] = [left, right, top, bottom].map(|r| r.shrink(MARGIN));
        let (current, next) = (Some(self.talk.slide()), self.talk.next_slide());
        match layout {
            Layout::SlidesFirst => {
                self.show_slide(ui, window, current, left);
                self.show_slide(ui, window, next, top);
                self.show_notes(ui, bottom);
            }
            Layout::NotesFirst => {
                self.show_slide(ui, window, current, top);
                self.show_slide(ui, window, next, bottom);
                self.show_notes(ui, right);
            }
        }
    }

    /// Every slide in a grid. The selected one is outlined, and a click jumps to a slide.
    fn show_overview(&mut self, ui: &egui::Ui, window: Window, body: egui::Rect, selected: usize) {
        let slides = self.talk.slides();
        let columns = self.talk.overview_columns();
        let cell = egui::vec2(
            body.width() / columns as f32,
            body.height() / slides.div_ceil(columns) as f32,
        );
        let mut picked = None;
        for page in 0..slides {
            let offset = egui::vec2((page % columns) as f32, (page / columns) as f32) * cell;
            let rect = egui::Rect::from_min_size(body.min + offset, cell).shrink(MARGIN);
            let Some(placed) = self.paint(ui, Some(page), rect) else {
                continue;
            };
            self.thumbnails[window as usize].push(placed.target);
            let id = ui.scope_id().with(("thumbnail", page));
            if ui.interact(placed.rect, id, egui::Sense::click()).clicked() {
                picked = Some(page);
            }
            let stroke = if page == selected {
                egui::Stroke::new(3.0, ui.visuals().warn_fg_color)
            } else if page == self.talk.slide() {
                egui::Stroke::new(2.0, ui.visuals().selection.bg_fill)
            } else {
                continue;
            };
            ui.painter()
                .rect_stroke(placed.rect, 0.0, stroke, egui::StrokeKind::Outside);
        }
        if let Some(page) = picked {
            self.talk.apply(Action::Pick(page), Instant::now());
            (self.repaint)();
        }
    }

    /// Draws `page`, if the deck has it, fitted into `rect`, and records the render it needs.
    fn show_slide(&mut self, ui: &egui::Ui, window: Window, page: Option<usize>, rect: egui::Rect) {
        if let Some(placed) = self.paint(ui, page, rect) {
            self.shown[window as usize].push(placed.target);
        }
    }

    /// Draws `page`, if the deck has it, fitted into `rect`, and says where it went.
    /// Until its render arrives, a render of the same page at another size stands in.
    fn paint(&self, ui: &egui::Ui, page: Option<usize>, rect: egui::Rect) -> Option<Placement> {
        let page = page.filter(|&page| page < self.pages.len())?;
        let placed = place(page, self.pages[page], rect, ui.ctx().pixels_per_point())?;
        if let Some(texture) = self.renderer.texture(placed.target) {
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
            ui.painter()
                .image(texture.id(), placed.rect, uv, egui::Color32::WHITE);
        }
        Some(placed)
    }

    fn show_notes(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(note) = self.notes.on_page(self.talk.slide()) else {
            return;
        };
        let mut notes_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let style = notes_ui.style_mut();
        enlarge_text(style, self.talk.settings().notes_scale);
        // Brighter than egui's dark gray, while bold stays white.
        style.visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_gray(210);
        egui::ScrollArea::vertical().show(&mut notes_ui, |ui| {
            if self.notes.plain_text {
                ui.label(note);
            } else {
                CommonMarkViewer::new().show(ui, &mut self.markdown, note);
            }
        });
    }

    fn show_status(&self, ui: &mut egui::Ui) {
        enlarge_text(ui.style_mut(), 1.4);
        let wall = jiff::Zoned::now();
        // The clock and timers tick once per second, in this window only.
        let to_next_second = 1_000_000_000 - wall.subsec_nanosecond().unsigned_abs();
        ui.ctx()
            .request_repaint_after(Duration::from_nanos(to_next_second.into()));
        // Whole seconds, so elapsed and left always add up to the goal.
        let elapsed = Duration::from_secs(self.talk.timer().elapsed(Instant::now()).as_secs());
        let schedule = self.notes.schedule;
        ui.horizontal(|ui| {
            // One gap between every item, whichever of them show.
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.strong(wall.strftime("%H:%M").to_string());
            let paused = if self.talk.timer().paused() {
                " (paused)"
            } else {
                ""
            };
            ui.label(format!(
                "elapsed {}{paused}",
                clock_face(elapsed.try_into().unwrap_or_default())
            ));
            if let Some(left) = schedule.remaining(elapsed, wall.time()) {
                let color = match schedule.urgency(left) {
                    Urgency::Calm => ui.visuals().text_color(),
                    Urgency::Closing => ui.visuals().warn_fg_color,
                    Urgency::Over => ui.visuals().error_fg_color,
                };
                ui.colored_label(color, format!("left {}", clock_face(left)));
            }
            ui.label(format!(
                "slide {} / {}",
                self.talk.slide() + 1,
                self.talk.slides()
            ));
            if let Some(typed) = self.talk.goto() {
                let typed = if typed == 0 {
                    String::new()
                } else {
                    typed.to_string()
                };
                ui.strong(format!("go to slide: {typed}_"));
            }
            let badge = |ui: &mut egui::Ui, text: &str| {
                ui.label(
                    egui::RichText::new(text)
                        .strong()
                        .color(egui::Color32::BLACK)
                        .background_color(ui.visuals().warn_fg_color),
                );
            };
            if self.talk.blanked() {
                badge(ui, " BLANK ");
            }
            if self.talk.frozen() {
                badge(ui, " FROZEN ");
            }
            if self.talk.quit_armed() {
                ui.colored_label(ui.visuals().warn_fg_color, "Press q again to quit");
            }
        });
    }
}

/// The gap in points between a panel's edge and its content.
const MARGIN: f32 = 8.0;

/// Splits `rect` side by side or top to bottom, giving the first part `share` of it.
/// Dragging the seam changes `share`, within 0.1 to 0.9.
fn split(
    ui: &egui::Ui,
    name: &str,
    rect: egui::Rect,
    share: &mut f32,
    side_by_side: bool,
) -> (egui::Rect, egui::Rect) {
    let axis = usize::from(!side_by_side);
    let length = rect.size()[axis];
    let seam = |share: f32| rect.min[axis] + length * share;
    let at = seam(*share);
    let handle = if side_by_side {
        egui::Rect::from_x_y_ranges(at - MARGIN..=at + MARGIN, rect.y_range())
    } else {
        egui::Rect::from_x_y_ranges(rect.x_range(), at - MARGIN..=at + MARGIN)
    };
    let response = ui.interact(handle, ui.scope_id().with(name), egui::Sense::drag());
    if response.dragged() && length > 0.0 {
        *share = (*share + response.drag_delta()[axis] / length).clamp(0.1, 0.9);
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(if side_by_side {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        });
    }
    let at = seam(*share);
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    if side_by_side {
        ui.painter().vline(at, rect.y_range(), stroke);
        rect.split_left_right_at_x(at)
    } else {
        ui.painter().hline(rect.x_range(), at, stroke);
        rect.split_top_bottom_at_y(at)
    }
}

/// Scales every text style of `style` by `factor`.
fn enlarge_text(style: &mut egui::Style, factor: f32) {
    for font in style.text_styles.values_mut() {
        font.size *= factor;
    }
}

/// The shortcut hint, built from the key table: keys without a modifier, then the Alt layer.
fn show_shortcuts(ui: &egui::Ui) {
    egui::Area::new(egui::Id::unique("shortcuts"))
        .fade_in(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            enlarge_text(ui.style_mut(), 1.4);
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for alt in [false, true] {
                    ui.strong(if alt { "With Alt held" } else { "Keys" });
                    egui::Grid::new(("shortcuts", alt))
                        .num_columns(2)
                        .spacing([24.0, 4.0])
                        .show(ui, |ui| {
                            for binding in keys::BINDINGS.iter().filter(|b| b.alt == alt) {
                                let keys: Vec<String> =
                                    binding.keys.iter().map(|&key| keys::label(key)).collect();
                                ui.label(keys.join(", "));
                                ui.label(binding.does);
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                }
            });
        });
}

/// The view `window` shows. A swap exchanges the two.
fn view_of(window: Window, swapped: bool) -> View {
    match (window, swapped) {
        (Window::Main, false) | (Window::Projector, true) => View::Presenter,
        (Window::Main, true) | (Window::Projector, false) => View::Audience,
    }
}

/// The renders the windows need, most urgent first: everything on screen, then for each
/// page on screen the page after it at the same size, then the overview thumbnails.
/// That covers the next slide at the audience and current sizes, and the one after it
/// at the next-slide panel's size.
fn plan<'a>(
    pages: usize,
    shown: impl Iterator<Item = &'a Target> + Clone,
    thumbnails: impl Iterator<Item = &'a Target>,
) -> Vec<Target> {
    let ahead = shown.clone().map(|target| Target {
        page: target.page + 1,
        ..*target
    });
    let mut wanted: Vec<Target> = Vec::new();
    for target in shown.copied().chain(ahead).chain(thumbnails.copied()) {
        if target.page < pages && !wanted.contains(&target) {
            wanted.push(target);
        }
    }
    wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(page: usize, width: u16) -> Target {
        Target {
            page,
            width,
            height: width / 2,
        }
    }

    #[test]
    fn on_screen_pages_come_before_the_pages_after_them() {
        let shown = [at(0, 1920), at(0, 640), at(1, 400)];
        let wanted = plan(3, shown.iter(), [].iter());
        let summary: Vec<_> = wanted.iter().map(|t| (t.page, t.width)).collect();
        assert_eq!(
            summary,
            [(0, 1920), (0, 640), (1, 400), (1, 1920), (1, 640), (2, 400)]
        );
    }

    #[test]
    fn the_last_page_has_nothing_after_it() {
        let shown = [at(1, 1920)];
        assert_eq!(plan(2, shown.iter(), [].iter()), [at(1, 1920)]);
    }

    #[test]
    fn same_sized_windows_share_renders() {
        let shown = [at(0, 800), at(0, 800)];
        assert_eq!(plan(2, shown.iter(), [].iter()).len(), 2);
    }

    #[test]
    fn nothing_on_screen_needs_nothing() {
        assert!(plan(3, [].iter(), [].iter()).is_empty());
    }

    #[test]
    fn thumbnails_come_last() {
        let shown = [at(1, 1920)];
        let thumbnails = [at(0, 160), at(1, 160), at(2, 160)];
        let wanted = plan(3, shown.iter(), thumbnails.iter());
        assert_eq!(wanted[..2], [at(1, 1920), at(2, 1920)]);
        assert_eq!(wanted[2..], thumbnails);
    }

    #[test]
    fn swap_exchanges_the_views() {
        assert_eq!(view_of(Window::Main, false), View::Presenter);
        assert_eq!(view_of(Window::Projector, false), View::Audience);
        assert_eq!(view_of(Window::Main, true), View::Audience);
        assert_eq!(view_of(Window::Projector, true), View::Presenter);
    }
}

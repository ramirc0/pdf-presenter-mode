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
    /// Where the mouse points on the current slide while the laser is on,
    /// as fractions of the slide's width and height.
    laser: Option<egui::Pos2>,
    /// Lines drawn with the laser, oldest first. The last one grows while its drag lasts.
    ink: Vec<Stroke>,
    repaint: Repaint,
}

/// A line of ink on one slide.
struct Stroke {
    /// The slide it was drawn on.
    slide: usize,
    /// Where the line runs, as fractions of the slide's width and height.
    points: Vec<egui::Pos2>,
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
            laser: None,
            ink: Vec::new(),
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
        let mut outcome = Outcome::Continue;
        for action in actions {
            if action == Action::Fullscreen {
                let fullscreen = ui.input(|input| input.viewport().fullscreen.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            if action == Action::Erase {
                let slide = self.talk.slide();
                self.ink.retain(|stroke| stroke.slide != slide);
            }
            match self.talk.apply(action, Instant::now()) {
                Outcome::Quit => return Outcome::Quit,
                Outcome::MoveAudience => outcome = Outcome::MoveAudience,
                Outcome::Continue => {}
            }
        }
        outcome
    }

    /// Stops rendering for `window`, which has closed.
    pub fn close(&mut self, window: Window) {
        self.shown[window as usize].clear();
        self.thumbnails[window as usize].clear();
    }

    /// Draws `window`'s view into `ui`.
    /// The presenter view's status bar names `audience`, the monitor the audience is on.
    pub fn draw(&mut self, ui: &mut egui::Ui, window: Window, audience: Option<&str>) {
        if let Some(deck) = self.reloads.try_iter().last() {
            self.talk.reload(deck.pages.len());
            self.pages = deck.pages;
            self.notes = deck.notes;
            // The ink marks spots on the old pages, which may have moved.
            self.ink.clear();
            // A new renderer has no textures, so no page of the old deck can show.
            self.renderer = Renderer::new(ui.ctx().clone(), deck.pdf, self.repaint.clone());
        }
        self.close(window);
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::BLACK);
        match view_of(window, self.talk.swapped()) {
            View::Audience => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                let slide = self.talk.audience_slide();
                let shown = self.show_slide(ui, window, slide, ui.max_rect());
                // Frozen on another slide, the audience cannot see what the speaker points at.
                if slide == Some(self.talk.slide()) {
                    self.show_laser(ui, shown);
                }
            }
            View::Presenter => self.draw_presenter(ui, window, audience),
        }
        let wanted = plan(
            self.pages.len(),
            self.shown.iter().flatten(),
            self.thumbnails.iter().flatten(),
        );
        self.renderer.request(wanted);
    }

    fn draw_presenter(&mut self, ui: &mut egui::Ui, window: Window, audience: Option<&str>) {
        egui::Panel::bottom("status")
            .frame(egui::Frame::NONE.inner_margin(8))
            .show(ui, |ui| self.show_status(ui, audience));
        let body = ui.available_rect_before_wrap();
        if let Some(selected) = self.talk.overview() {
            self.show_overview(ui, window, body, selected);
            self.aim(ui, None);
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
        let [current, next, notes] = match layout {
            Layout::SlidesFirst => [left, top, bottom],
            Layout::NotesFirst => [top, bottom, right],
        }
        .map(|r| r.shrink(MARGIN));
        let shown = self.show_slide(ui, window, Some(self.talk.slide()), current);
        self.aim(ui, shown);
        self.scribble(ui, shown);
        self.show_laser(ui, shown);
        self.show_slide(ui, window, self.talk.next_slide(), next);
        self.show_notes(ui, notes);
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

    /// Draws `page`, if the deck has it, fitted into `rect`, records the render it needs,
    /// and says where the page went.
    fn show_slide(
        &mut self,
        ui: &egui::Ui,
        window: Window,
        page: Option<usize>,
        rect: egui::Rect,
    ) -> Option<egui::Rect> {
        let placed = self.paint(ui, page, rect)?;
        self.shown[window as usize].push(placed.target);
        Some(placed.rect)
    }

    /// Points the laser where the mouse is on `slide`, the current slide's rect, while the laser is on.
    /// A move repaints both windows, so the audience follows.
    fn aim(&mut self, ui: &egui::Ui, slide: Option<egui::Rect>) {
        let aimed = slide.filter(|_| self.talk.laser()).and_then(|rect| {
            let pos = ui
                .input(|input| input.pointer.hover_pos())
                .filter(|_| ui.rect_contains_pointer(rect))?;
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            Some(fraction_of(rect, pos))
        });
        if aimed != self.laser {
            self.laser = aimed;
            (self.repaint)();
        }
    }

    /// While the laser is on, draws ink where the left button drags on `slide`, the current slide's rect.
    /// Only a drag that starts on the slide draws, so resizing the panels never does.
    fn scribble(&mut self, ui: &egui::Ui, slide: Option<egui::Rect>) {
        let Some(rect) = slide.filter(|_| self.talk.laser()) else {
            return;
        };
        let drag = ui.interact(rect, ui.scope_id().with("ink"), egui::Sense::drag());
        let page = self.talk.slide();
        if drag.drag_started_by(egui::PointerButton::Primary) {
            self.ink.push(Stroke {
                slide: page,
                points: Vec::new(),
            });
        }
        let Some(pos) = drag
            .interact_pointer_pos()
            .filter(|_| drag.dragged_by(egui::PointerButton::Primary))
        else {
            return;
        };
        // An erase or a slide change mid-drag can leave the last line on another slide.
        let Some(stroke) = self.ink.last_mut().filter(|stroke| stroke.slide == page) else {
            return;
        };
        let at = fraction_of(rect, pos);
        // Each repaint runs this again, so a repaint for a button held still would never stop.
        if stroke.points.last() != Some(&at) {
            stroke.points.push(at);
            (self.repaint)();
        }
    }

    /// Draws the laser dot on `slide`, the rect of a picture of the current slide.
    fn show_laser(&self, ui: &egui::Ui, slide: Option<egui::Rect>) {
        // A window that stopped drawing leaves its last aim behind. Turning the laser off still hides it.
        let (Some(rect), Some(at)) = (slide, self.laser.filter(|_| self.talk.laser())) else {
            return;
        };
        let center = rect.lerp_inside(at.to_vec2());
        let radius = rect.height() * LASER_RADIUS;
        let painter = ui.painter().with_clip_rect(rect);
        painter.circle_filled(center, 2.0 * radius, LASER.gamma_multiply(0.3));
        painter.circle_filled(center, radius, LASER);
    }

    /// Draws `page` with its ink, if the deck has it, fitted into `rect`, and says where it went.
    /// Until its render arrives, a render of the same page at another size stands in.
    fn paint(&self, ui: &egui::Ui, page: Option<usize>, rect: egui::Rect) -> Option<Placement> {
        let page = page.filter(|&page| page < self.pages.len())?;
        let placed = place(page, self.pages[page], rect, ui.ctx().pixels_per_point())?;
        if let Some(texture) = self.renderer.texture(placed.target) {
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
            ui.painter()
                .image(texture.id(), placed.rect, uv, egui::Color32::WHITE);
        }
        self.show_ink(ui, page, placed.rect);
        Some(placed)
    }

    /// Draws the ink on `page` over its picture at `rect`.
    fn show_ink(&self, ui: &egui::Ui, page: usize, rect: egui::Rect) {
        let painter = ui.painter().with_clip_rect(rect);
        let pen = egui::Stroke::new(rect.height() * INK_WIDTH, LASER);
        for stroke in self.ink.iter().filter(|stroke| stroke.slide == page) {
            let points = stroke
                .points
                .iter()
                .map(|&at| rect.lerp_inside(at.to_vec2()));
            painter.line(points.collect(), pen);
        }
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

    fn show_status(&self, ui: &mut egui::Ui, audience: Option<&str>) {
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
            if let Some(monitor) = audience {
                ui.label(format!("audience {monitor}"));
            }
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
            if self.talk.laser() {
                badge(ui, " LASER ");
            }
            if self.talk.quit_armed() {
                ui.colored_label(ui.visuals().warn_fg_color, "Press q again to quit");
            }
        });
    }
}

/// The gap in points between a panel's edge and its content.
const MARGIN: f32 = 8.0;

/// The color of the laser dot.
const LASER: egui::Color32 = egui::Color32::from_rgb(255, 32, 32);

/// The laser dot's radius, as a fraction of the slide's height.
const LASER_RADIUS: f32 = 0.008;

/// The ink's width, as a fraction of the slide's height.
const INK_WIDTH: f32 = 0.006;

/// Where `pos` falls on `rect`, as fractions of its width and height.
fn fraction_of(rect: egui::Rect, pos: egui::Pos2) -> egui::Pos2 {
    ((pos - rect.min) / rect.size()).to_pos2()
}

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
pub fn enlarge_text(style: &mut egui::Style, factor: f32) {
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

    /// The window size of the headless frames.
    const SCREEN: egui::Rect =
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1920.0, 1200.0));

    /// A spot inside the current slide of the slides-first layout on `SCREEN`.
    const ON_SLIDE: egui::Pos2 = egui::pos2(576.0, 540.0);

    /// Another spot on the current slide, where the test drags end.
    const DRAG_TO: egui::Pos2 = egui::pos2(700.0, 600.0);

    /// A spot on the next-slide panel of the slides-first layout on `SCREEN`.
    const OFF_SLIDE: egui::Pos2 = egui::pos2(1600.0, 250.0);

    /// The deck the tests present.
    fn fixture_deck() -> Deck {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/basic.pdf");
        Deck::open(&path, None).unwrap()
    }

    /// A stage presenting the fixture deck, with textures going to `ctx`.
    fn fixture_stage(ctx: &egui::Context) -> Stage {
        let (_, reloads) = std::sync::mpsc::channel();
        Stage::new(
            fixture_deck(),
            Settings::default(),
            reloads,
            ctx.clone(),
            Arc::new(|| {}),
        )
    }

    /// Runs one frame of `window` on `events`, as `presenter.rs` does, and returns its shapes.
    fn shapes(
        ctx: &egui::Context,
        stage: &mut Stage,
        window: Window,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Shape> {
        let input = egui::RawInput {
            screen_rect: Some(SCREEN),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            stage.handle_keys(ui);
            stage.draw(ui, window, None);
        });
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    /// Draws one frame of `window` with the mouse at `mouse`, or outside the window,
    /// and returns the centers of the laser dots drawn.
    fn frame(
        ctx: &egui::Context,
        stage: &mut Stage,
        window: Window,
        mouse: Option<egui::Pos2>,
    ) -> Vec<egui::Pos2> {
        let event = mouse.map_or(egui::Event::PointerGone, egui::Event::PointerMoved);
        shapes(ctx, stage, window, vec![event])
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Circle(circle) if circle.fill == LASER => Some(circle.center),
                _ => None,
            })
            .collect()
    }

    /// Draws one frame of `window` and returns the points of its ink lines.
    fn ink(ctx: &egui::Context, stage: &mut Stage, window: Window) -> Vec<Vec<egui::Pos2>> {
        let red = egui::epaint::ColorMode::Solid(LASER);
        shapes(ctx, stage, window, Vec::new())
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Path(path) if path.stroke.color == red => Some(path.points),
                _ => None,
            })
            .collect()
    }

    /// The left button going down, or up, at `pos`.
    fn left_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Drags with the left button from `from` to `to` in the presenter view.
    fn drag(ctx: &egui::Context, stage: &mut Stage, from: egui::Pos2, to: egui::Pos2) {
        for event in [
            egui::Event::PointerMoved(from),
            left_button(from, true),
            egui::Event::PointerMoved(to),
            left_button(to, false),
        ] {
            shapes(ctx, stage, Window::Main, vec![event]);
        }
    }

    /// Asserts that the lines `got` run through the points `want`, give or take rounding.
    fn assert_lines(got: &[Vec<egui::Pos2>], want: &[Vec<egui::Pos2>]) {
        let near = |a: &Vec<egui::Pos2>, b: &Vec<egui::Pos2>| {
            a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (*p - *q).length() < 1e-3)
        };
        let same = got.len() == want.len() && got.iter().zip(want).all(|(a, b)| near(a, b));
        assert!(same, "{got:?} != {want:?}");
    }

    /// Turns the laser on and points at `ON_SLIDE` in the presenter view.
    fn point(ctx: &egui::Context, stage: &mut Stage) -> Vec<egui::Pos2> {
        stage.talk.apply(Action::Laser, Instant::now());
        frame(ctx, stage, Window::Main, Some(ON_SLIDE))
    }

    #[test]
    fn the_laser_starts_off() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        assert!(frame(&ctx, &mut stage, Window::Main, Some(ON_SLIDE)).is_empty());
        assert!(frame(&ctx, &mut stage, Window::Projector, None).is_empty());
    }

    #[test]
    fn the_audience_sees_the_dot_where_the_speaker_points() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        let presenter = point(&ctx, &mut stage);
        assert_eq!(presenter.len(), 1);
        assert!((presenter[0] - ON_SLIDE).length() < 1e-3, "{presenter:?}");
        let at = stage.laser.unwrap();
        let slide = place(0, stage.pages[0], SCREEN, 1.0).unwrap().rect;
        let audience = frame(&ctx, &mut stage, Window::Projector, None);
        assert_eq!(audience.len(), 1);
        let want = slide.min + at.to_vec2() * slide.size();
        assert!(
            (audience[0] - want).length() < 1e-3,
            "{audience:?} != {want:?}"
        );
    }

    #[test]
    fn the_dot_goes_when_the_mouse_leaves() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        point(&ctx, &mut stage);
        assert!(frame(&ctx, &mut stage, Window::Main, None).is_empty());
        assert!(frame(&ctx, &mut stage, Window::Projector, None).is_empty());
    }

    #[test]
    fn turning_the_laser_off_hides_the_dot_from_the_audience_at_once() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        point(&ctx, &mut stage);
        stage.talk.apply(Action::Laser, Instant::now());
        assert!(frame(&ctx, &mut stage, Window::Projector, None).is_empty());
    }

    #[test]
    fn a_frozen_audience_does_not_see_the_dot_on_another_slide() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        stage.talk.apply(Action::Freeze, Instant::now());
        stage.talk.apply(Action::Next, Instant::now());
        assert_eq!(point(&ctx, &mut stage).len(), 1);
        assert!(frame(&ctx, &mut stage, Window::Projector, None).is_empty());
    }

    #[test]
    fn the_overview_hides_the_dot() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        point(&ctx, &mut stage);
        stage.talk.apply(Action::Overview, Instant::now());
        assert!(frame(&ctx, &mut stage, Window::Main, Some(ON_SLIDE)).is_empty());
        assert!(frame(&ctx, &mut stage, Window::Projector, None).is_empty());
    }

    #[test]
    fn a_drag_on_the_slide_draws_ink_on_both_views() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        stage.talk.apply(Action::Laser, Instant::now());
        drag(&ctx, &mut stage, ON_SLIDE, DRAG_TO);
        assert_lines(
            &ink(&ctx, &mut stage, Window::Main),
            &[vec![ON_SLIDE, DRAG_TO]],
        );
        let slide = place(0, stage.pages[0], SCREEN, 1.0).unwrap().rect;
        let points = stage.ink[0].points.iter();
        let want = points.map(|&at| slide.lerp_inside(at.to_vec2())).collect();
        assert_lines(&ink(&ctx, &mut stage, Window::Projector), &[want]);
    }

    #[test]
    fn a_drag_draws_nothing_with_the_laser_off() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        drag(&ctx, &mut stage, ON_SLIDE, DRAG_TO);
        assert!(ink(&ctx, &mut stage, Window::Main).is_empty());
    }

    #[test]
    fn a_drag_from_off_the_slide_draws_nothing() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        stage.talk.apply(Action::Laser, Instant::now());
        drag(&ctx, &mut stage, OFF_SLIDE, ON_SLIDE);
        assert!(ink(&ctx, &mut stage, Window::Main).is_empty());
    }

    #[test]
    fn the_ink_stays_with_its_slide() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        stage.talk.apply(Action::Laser, Instant::now());
        drag(&ctx, &mut stage, ON_SLIDE, DRAG_TO);
        stage.talk.apply(Action::Next, Instant::now());
        assert!(ink(&ctx, &mut stage, Window::Main).is_empty());
        assert!(ink(&ctx, &mut stage, Window::Projector).is_empty());
        stage.talk.apply(Action::Previous, Instant::now());
        assert_eq!(ink(&ctx, &mut stage, Window::Main).len(), 1);
    }

    #[test]
    fn alt_e_erases_the_ink_on_the_current_slide_only() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        stage.talk.apply(Action::Laser, Instant::now());
        drag(&ctx, &mut stage, ON_SLIDE, DRAG_TO);
        stage.talk.apply(Action::Next, Instant::now());
        drag(&ctx, &mut stage, ON_SLIDE, DRAG_TO);
        let alt_e = egui::Event::Key {
            key: egui::Key::E,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::ALT,
        };
        shapes(&ctx, &mut stage, Window::Main, vec![alt_e]);
        assert!(ink(&ctx, &mut stage, Window::Main).is_empty());
        stage.talk.apply(Action::Previous, Instant::now());
        assert_eq!(ink(&ctx, &mut stage, Window::Main).len(), 1);
    }

    #[test]
    fn a_reload_erases_the_ink() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        stage.talk.apply(Action::Laser, Instant::now());
        drag(&ctx, &mut stage, ON_SLIDE, DRAG_TO);
        let (reload, reloads) = std::sync::mpsc::channel();
        stage.reloads = reloads;
        reload.send(fixture_deck()).unwrap();
        assert!(ink(&ctx, &mut stage, Window::Main).is_empty());
    }

    #[test]
    fn a_held_button_adds_no_points() {
        let ctx = egui::Context::default();
        let mut stage = fixture_stage(&ctx);
        stage.talk.apply(Action::Laser, Instant::now());
        frame(&ctx, &mut stage, Window::Main, Some(ON_SLIDE));
        shapes(
            &ctx,
            &mut stage,
            Window::Main,
            vec![left_button(ON_SLIDE, true)],
        );
        for _ in 0..3 {
            shapes(&ctx, &mut stage, Window::Main, Vec::new());
        }
        assert_eq!(stage.ink.len(), 1);
        assert_eq!(stage.ink[0].points.len(), 1);
    }

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

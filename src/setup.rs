//! The start page: pick a deck, place the two windows, set the countdown, and start.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui;

use crate::deck::Deck;
use crate::launch::Launch;
use crate::monitors::{Monitor, audience_monitor, names_of};
use crate::presenter::Projector;
use crate::stage::enlarge_text;
use crate::timer::Goal;

/// Where the start page sends a window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Spot {
    /// The audience takes the first external monitor and follows hotplug.
    /// The presenter goes fullscreen on the monitor the start page is on.
    #[default]
    Auto,
    /// Fullscreen on this monitor.
    Monitor(String),
    /// A normal window.
    Window,
}

/// The width of the path field and the monitor map, in points.
const WIDTH: f32 = 560.0;

/// The start page's choices, kept between frames.
#[derive(Default)]
pub struct Setup {
    /// The deck's path, as typed.
    path: String,
    /// The deck opened from `path`, or why it did not open. `None` until a path is entered.
    opened: Option<Result<Deck, String>>,
    audience: Spot,
    presenter: Spot,
    /// The countdown in minutes, as typed.
    minutes: String,
    /// The end time, as typed, such as `14:30`.
    ends_at: String,
    /// Whether the path field has had its first focus.
    focused: bool,
    /// The file chooser's answer, while it is open.
    chooser: Option<Receiver<Option<PathBuf>>>,
}

impl Setup {
    /// Draws the page into `ui`, with `monitors` connected and the window on the monitor
    /// named `here`. Returns the talk and its opened deck once the speaker starts it.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        monitors: &[Monitor],
        here: Option<&str>,
    ) -> Option<(Launch, Deck)> {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
        enlarge_text(ui.style_mut(), 1.4);
        let names = names_of(monitors);
        let presenter = self.presenter_on(here);
        let audience = self.audience_on(&names, presenter.as_deref());
        let mut start = false;
        egui::Frame::NONE.inner_margin(24).show(ui, |ui| {
            egui::Grid::new("setup")
                .num_columns(2)
                .spacing([24.0, 12.0])
                .show(ui, |ui| {
                    ui.label("Deck");
                    ui.vertical(|ui| {
                        start |= self.show_path(ui);
                        self.show_check(ui);
                    });
                    ui.end_row();
                    ui.label("Screens");
                    ui.vertical(|ui| {
                        ui.weak("Click a screen to put the audience there.");
                        self.show_map(ui, monitors, audience.as_deref(), presenter.as_deref());
                    });
                    ui.end_row();
                    ui.label("Audience");
                    self.choose_audience(ui, &names, audience.as_deref(), presenter.as_deref());
                    ui.end_row();
                    ui.label("Presenter");
                    self.choose_presenter(
                        ui,
                        &names,
                        here,
                        audience.as_deref(),
                        presenter.as_deref(),
                    );
                    ui.end_row();
                    ui.label("Timer");
                    self.show_timer(ui);
                    ui.end_row();
                    ui.label("");
                    let ready = matches!(self.opened, Some(Ok(_))) && self.goal().is_ok();
                    let button =
                        egui::Button::new("Start (Enter)").min_size(egui::vec2(160.0, 0.0));
                    start |= ui.add_enabled(ready, button).clicked();
                    ui.end_row();
                });
        });
        if start { self.launch(here) } else { None }
    }

    /// The path field and the button for the file chooser. Leaving the field opens the deck.
    /// Returns whether Enter left it.
    fn show_path(&mut self, ui: &mut egui::Ui) -> bool {
        self.take_choice();
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text("the PDF to present, such as ~/talks/talk.pdf")
                    .desired_width(WIDTH - 100.0),
            );
            if !self.focused {
                field.request_focus();
                self.focused = true;
            }
            if field.changed() {
                self.opened = None;
            }
            if field.lost_focus() && self.opened.is_none() && !self.path.trim().is_empty() {
                self.open();
            }
            let open = egui::Button::new("Open…").min_size(egui::vec2(88.0, 0.0));
            if ui.add_enabled(self.chooser.is_none(), open).clicked() {
                self.choose(ui.ctx());
            }
            field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
        })
        .inner
    }

    /// Opens the desktop's file chooser on its own thread, so the page keeps drawing.
    fn choose(&mut self, ctx: &egui::Context) {
        let (answer, chooser) = mpsc::channel();
        let folder = expand_home(self.path.trim())
            .parent()
            .filter(|folder| folder.is_dir())
            .map(Path::to_path_buf);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("chooser".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Open a deck")
                    .add_filter("PDF", &["pdf"]);
                if let Some(folder) = folder {
                    dialog = dialog.set_directory(folder);
                }
                // The talk may have started meanwhile and dropped the receiver.
                let _ = answer.send(dialog.pick_file());
                ctx.request_repaint();
            })
            .expect("cannot start the file chooser thread");
        self.chooser = Some(chooser);
    }

    /// Opens the deck the file chooser picked, once it answers.
    fn take_choice(&mut self) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        match chooser.try_recv() {
            Ok(Some(path)) => {
                self.path = path.display().to_string();
                self.open();
                self.chooser = None;
            }
            Ok(None) | Err(TryRecvError::Disconnected) => self.chooser = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Opens the deck at the typed path, and fills the timer from its notes.
    fn open(&mut self) {
        let opened = Deck::open(&expand_home(self.path.trim()), None);
        if let Ok(deck) = &opened {
            (self.minutes, self.ends_at) = match deck.notes.schedule.goal {
                Some(Goal::Length(length)) => ((length.as_secs() / 60).to_string(), String::new()),
                Some(Goal::EndsAt(time)) => (String::new(), time.strftime("%H:%M").to_string()),
                None => (String::new(), String::new()),
            };
        }
        self.opened = Some(opened.map_err(|err| format!("{err:#}")));
    }

    /// The opened deck's slide and note counts, or why it did not open.
    fn show_check(&self, ui: &mut egui::Ui) {
        match &self.opened {
            Some(Ok(deck)) => {
                let slides = deck.pages.len();
                let noted = (0..slides)
                    .filter(|&page| deck.notes.on_page(page).is_some())
                    .count();
                ui.weak(format!("{slides} slides · notes on {noted}"));
            }
            Some(Err(why)) => {
                ui.colored_label(ui.visuals().error_fg_color, why);
            }
            None => {}
        }
    }

    /// The monitors as the compositor arranges them, with the role of each.
    /// A click sends the audience to a monitor.
    fn show_map(
        &mut self,
        ui: &mut egui::Ui,
        monitors: &[Monitor],
        audience: Option<&str>,
        presenter: Option<&str>,
    ) {
        let (frame, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 200.0), egui::Sense::hover());
        let named: Vec<&Monitor> = monitors.iter().filter(|m| !m.name.is_empty()).collect();
        let Some(bounds) = named.iter().map(|m| m.area).reduce(|a, b| a.union(b)) else {
            return;
        };
        let scale = (frame.width() / bounds.width()).min(frame.height() / bounds.height());
        let place = |pos: egui::Pos2| frame.min + (pos - bounds.min) * scale;
        let font = egui::TextStyle::Body.resolve(ui.style());
        for monitor in named {
            let tile = egui::Rect::from_min_max(place(monitor.area.min), place(monitor.area.max))
                .shrink(2.0);
            let response = ui.interact(
                tile,
                egui::Id::unique("screen").with(monitor.name.as_str()),
                egui::Sense::click(),
            );
            let look = ui.style().interact(&response);
            let painter = ui.painter();
            painter.rect(
                tile,
                4.0,
                look.bg_fill,
                look.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let [width, height] = monitor.pixels;
            painter.text(
                tile.left_top() + egui::vec2(8.0, 6.0),
                egui::Align2::LEFT_TOP,
                format!("{}\n{width}×{height}", monitor.name),
                font.clone(),
                look.text_color(),
            );
            let role = if audience == Some(monitor.name.as_str()) {
                "AUDIENCE"
            } else if presenter == Some(monitor.name.as_str()) {
                "PRESENTER"
            } else {
                ""
            };
            painter.text(
                tile.center_bottom() - egui::vec2(0.0, 8.0),
                egui::Align2::CENTER_BOTTOM,
                role,
                font.clone(),
                ui.visuals().strong_text_color(),
            );
            if response.clicked() {
                self.send_audience_to(&monitor.name, audience, presenter);
            }
        }
    }

    fn choose_audience(
        &mut self,
        ui: &mut egui::Ui,
        names: &[String],
        audience: Option<&str>,
        presenter: Option<&str>,
    ) {
        let auto = match audience_monitor(names, None, presenter) {
            Some(index) => format!("Auto: first external, now {}", names[index]),
            None => String::from("Auto: first external, none yet"),
        };
        let label = |spot: &Spot| match spot {
            Spot::Auto => auto.clone(),
            Spot::Monitor(monitor) => monitor.clone(),
            Spot::Window => String::from("A window"),
        };
        let choices = spots(names, None);
        egui::ComboBox::from_id_salt("audience")
            .width(WIDTH)
            .selected_text(label(&self.audience))
            .show_ui(ui, |ui| {
                for spot in choices {
                    if ui
                        .selectable_label(self.audience == spot, label(&spot))
                        .clicked()
                    {
                        match &spot {
                            Spot::Monitor(monitor) => {
                                self.send_audience_to(monitor, audience, presenter);
                            }
                            _ => self.audience = spot,
                        }
                    }
                }
            });
    }

    fn choose_presenter(
        &mut self,
        ui: &mut egui::Ui,
        names: &[String],
        here: Option<&str>,
        audience: Option<&str>,
        presenter: Option<&str>,
    ) {
        let label = |spot: &Spot| match spot {
            Spot::Auto => match here {
                Some(monitor) => format!("Fullscreen on this screen, {monitor}"),
                None => String::from("Fullscreen on this screen"),
            },
            Spot::Monitor(monitor) => format!("Fullscreen on {monitor}"),
            Spot::Window => String::from("A window"),
        };
        let choices = spots(names, here);
        egui::ComboBox::from_id_salt("presenter")
            .width(WIDTH)
            .selected_text(label(&self.presenter))
            .show_ui(ui, |ui| {
                for spot in choices {
                    if ui
                        .selectable_label(self.presenter == spot, label(&spot))
                        .clicked()
                    {
                        match &spot {
                            Spot::Monitor(monitor) => {
                                self.send_presenter_to(monitor, audience, presenter);
                            }
                            _ => self.presenter = spot,
                        }
                    }
                }
            });
    }

    fn show_timer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.minutes).desired_width(48.0));
            ui.label("min, or ends at");
            ui.add(
                egui::TextEdit::singleline(&mut self.ends_at)
                    .hint_text("HH:MM")
                    .desired_width(80.0),
            );
            if let Err(why) = self.goal() {
                ui.colored_label(ui.visuals().error_fg_color, why);
            }
        });
    }

    /// Sends the audience to `monitor`. A presenter there moves to where the audience was.
    fn send_audience_to(&mut self, monitor: &str, audience: Option<&str>, presenter: Option<&str>) {
        if presenter == Some(monitor) {
            self.presenter = audience.map_or(Spot::Window, |m| Spot::Monitor(m.into()));
        }
        self.audience = Spot::Monitor(monitor.into());
    }

    /// Sends the presenter to `monitor`. An audience there moves to where the presenter was.
    fn send_presenter_to(
        &mut self,
        monitor: &str,
        audience: Option<&str>,
        presenter: Option<&str>,
    ) {
        if audience == Some(monitor) {
            self.audience = presenter.map_or(Spot::Auto, |m| Spot::Monitor(m.into()));
        }
        self.presenter = Spot::Monitor(monitor.into());
    }

    /// The monitor the presenter goes fullscreen on, with the start page on `here`.
    fn presenter_on(&self, here: Option<&str>) -> Option<String> {
        match &self.presenter {
            Spot::Auto => here.map(Into::into),
            Spot::Monitor(monitor) => Some(monitor.clone()),
            Spot::Window => None,
        }
    }

    /// The monitor the audience goes to among `names`, with the presenter on `presenter`.
    fn audience_on(&self, names: &[String], presenter: Option<&str>) -> Option<String> {
        let requested = match &self.audience {
            Spot::Auto => None,
            Spot::Monitor(monitor) => Some(monitor.as_str()),
            Spot::Window => return None,
        };
        audience_monitor(names, requested, presenter).map(|index| names[index].clone())
    }

    /// The countdown typed into the timer fields.
    fn goal(&self) -> Result<Option<Goal>, &'static str> {
        match (self.minutes.trim(), self.ends_at.trim()) {
            ("", "") => Ok(None),
            (minutes, "") => minutes
                .parse::<u64>()
                .ok()
                .filter(|&minutes| minutes > 0)
                .map(|minutes| Some(Goal::Length(Duration::from_secs(minutes * 60))))
                .ok_or("minutes must be a whole number"),
            ("", time) => time
                .parse()
                .map(|time| Some(Goal::EndsAt(time)))
                .map_err(|_| "the end time must look like 14:30"),
            _ => Err("set minutes or an end time, not both"),
        }
    }

    /// The talk the choices describe, with the window on `here`, once a deck is open.
    fn launch(&mut self, here: Option<&str>) -> Option<(Launch, Deck)> {
        let goal = self.goal().ok()?;
        let Some(Ok(_)) = &self.opened else {
            return None;
        };
        let Some(Ok(mut deck)) = self.opened.take() else {
            return None;
        };
        // Only a countdown other than the notes file's overrides it, on reloads too.
        let goal = goal.filter(|&goal| Some(goal) != deck.notes.schedule.goal);
        if goal.is_some() {
            deck.notes.schedule.goal = goal;
        }
        let audience = match &self.audience {
            Spot::Auto => Projector::Monitor { requested: None },
            Spot::Monitor(monitor) => Projector::Monitor {
                requested: Some(monitor.clone()),
            },
            Spot::Window => Projector::Windowed,
        };
        let launch = Launch {
            deck: expand_home(self.path.trim()),
            notes: None,
            audience,
            presenter: self.presenter_on(here),
            goal,
        };
        Some((launch, deck))
    }
}

/// The choices for a window: the default, each monitor in `names` but `except`, and a window.
fn spots(names: &[String], except: Option<&str>) -> Vec<Spot> {
    let monitors = names
        .iter()
        .filter(|name| !name.is_empty() && Some(name.as_str()) != except)
        .map(|name| Spot::Monitor(name.clone()));
    std::iter::once(Spot::Auto)
        .chain(monitors)
        .chain([Spot::Window])
        .collect()
}

/// `path` with a leading `~/` replaced by the home directory.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    /// A laptop panel with an external monitor to its right.
    fn desk() -> Vec<Monitor> {
        let monitor = |name: &str, x: f32, width: f32, height: f32| Monitor {
            name: name.into(),
            pixels: [width as u32, height as u32],
            area: egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(width, height)),
        };
        vec![
            monitor("eDP-1", 0.0, 1920.0, 1200.0),
            monitor("DP-6", 1920.0, 3072.0, 1728.0),
        ]
    }

    /// Runs one frame of the page on `events`, with the window on eDP-1.
    fn frame(
        ctx: &egui::Context,
        setup: &mut Setup,
        events: Vec<egui::Event>,
    ) -> (Option<(Launch, Deck)>, Vec<egui::Shape>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            events,
            ..Default::default()
        };
        let mut started = None;
        let mut output = ctx.run_ui(input, |ui| started = setup.show(ui, &desk(), Some("eDP-1")));
        output.textures_delta.clear();
        let shapes = output
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect();
        (started, shapes)
    }

    /// Where the map writes `text` at the top of a tile.
    fn label_of(shapes: &[egui::Shape], text: &str) -> egui::Pos2 {
        shapes
            .iter()
            .find_map(|shape| match shape {
                egui::Shape::Text(label) if label.galley.text().starts_with(text) => {
                    Some(label.pos)
                }
                _ => None,
            })
            .unwrap()
    }

    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    fn opened(path: &str) -> Setup {
        let mut setup = Setup {
            path: path.into(),
            ..Default::default()
        };
        setup.open();
        setup
    }

    #[test]
    fn the_defaults_send_the_audience_out_and_keep_the_presenter_here() {
        let mut setup = opened(&fixture("basic.pdf"));
        let (launch, deck) = setup.launch(Some("eDP-1")).unwrap();
        assert!(matches!(
            launch.audience,
            Projector::Monitor { requested: None }
        ));
        assert_eq!(launch.presenter.as_deref(), Some("eDP-1"));
        assert_eq!(launch.deck, PathBuf::from(fixture("basic.pdf")));
        assert!(!deck.pages.is_empty());
    }

    #[test]
    fn a_window_choice_becomes_a_window() {
        let mut setup = opened(&fixture("basic.pdf"));
        setup.audience = Spot::Window;
        setup.presenter = Spot::Window;
        let (launch, _) = setup.launch(Some("eDP-1")).unwrap();
        assert!(matches!(launch.audience, Projector::Windowed));
        assert_eq!(launch.presenter, None);
    }

    #[test]
    fn a_bad_deck_says_why_and_cannot_start() {
        let mut setup = opened(&fixture("truncated.pdf"));
        assert!(matches!(&setup.opened, Some(Err(why)) if why.contains("%%EOF")));
        assert!(setup.launch(None).is_none());
    }

    #[test]
    fn the_timer_reads_minutes_or_an_end_time() {
        let mut setup = Setup::default();
        assert_eq!(setup.goal(), Ok(None));
        setup.minutes = "20".into();
        assert_eq!(
            setup.goal(),
            Ok(Some(Goal::Length(Duration::from_secs(20 * 60))))
        );
        setup.ends_at = "14:30".into();
        assert!(setup.goal().is_err());
        setup.minutes.clear();
        assert_eq!(
            setup.goal(),
            Ok(Some(Goal::EndsAt(jiff::civil::time(14, 30, 0, 0))))
        );
        setup.ends_at = "soon".into();
        assert!(setup.goal().is_err());
    }

    #[test]
    fn a_click_on_a_screen_sends_the_audience_there() {
        let ctx = egui::Context::default();
        let mut setup = Setup {
            presenter: Spot::Window,
            ..Default::default()
        };
        // egui hit-tests a press against the previous frame, so draw one first.
        let (_, shapes) = frame(&ctx, &mut setup, Vec::new());
        let laptop = label_of(&shapes, "eDP-1") + egui::vec2(4.0, 4.0);
        frame(&ctx, &mut setup, click(laptop));
        assert_eq!(setup.audience, Spot::Monitor("eDP-1".into()));
    }

    #[test]
    fn sending_the_audience_to_the_presenter_swaps_them() {
        let ctx = egui::Context::default();
        let mut setup = Setup::default();
        let (_, shapes) = frame(&ctx, &mut setup, Vec::new());
        let laptop = label_of(&shapes, "eDP-1") + egui::vec2(4.0, 4.0);
        frame(&ctx, &mut setup, click(laptop));
        assert_eq!(setup.audience, Spot::Monitor("eDP-1".into()));
        assert_eq!(setup.presenter, Spot::Monitor("DP-6".into()));
    }

    #[test]
    fn the_presenter_takes_the_audience_monitor_by_swapping() {
        let mut setup = Setup::default();
        setup.send_presenter_to("DP-6", Some("DP-6"), Some("eDP-1"));
        assert_eq!(setup.presenter, Spot::Monitor("DP-6".into()));
        assert_eq!(setup.audience, Spot::Monitor("eDP-1".into()));
    }

    #[test]
    fn a_home_path_expands() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            expand_home("~/talk.pdf"),
            PathBuf::from(home).join("talk.pdf")
        );
        assert_eq!(expand_home("/tmp/talk.pdf"), PathBuf::from("/tmp/talk.pdf"));
    }
}

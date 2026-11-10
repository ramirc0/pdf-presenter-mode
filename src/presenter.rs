//! The root window, from the start page to the presenter, and the projector window it opens, moves and closes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::deck::Deck;
use crate::launch::Launch;
use crate::monitors::{Monitors, audience_monitor, names_of, next_audience_monitor};
use crate::settings::{Recent, Settings};
use crate::setup::Setup;
use crate::stage::{Repaint, Stage, Window};
use crate::talk::Outcome;
use crate::watch;

/// Where the projector window goes.
#[derive(Clone)]
pub enum Projector {
    /// Fullscreen on a monitor: the requested connector, or else the first external one.
    Monitor {
        /// A connector name such as `DP-6`, from `--audience`.
        requested: Option<String>,
    },
    /// A normal window, for rehearsal and screen sharing.
    Windowed,
}

impl Projector {
    /// Whether the projector window has somewhere to go among `names`, with the presenter
    /// fullscreen on `presenter`.
    fn has_place(&self, names: &[String], presenter: Option<&str>) -> bool {
        match self {
            Projector::Windowed => true,
            Projector::Monitor { requested } => {
                audience_monitor(names, requested.as_deref(), presenter).is_some()
            }
        }
    }

    /// The connector the audience asks for by name, if any.
    fn requested(&self) -> Option<&str> {
        match self {
            Projector::Monitor { requested } => requested.as_deref(),
            Projector::Windowed => None,
        }
    }
}

/// The viewport of the projector window.
pub fn projector_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("projector")
}

/// How often to look for a changed deck or notes file.
const RELOAD_POLL: Duration = Duration::from_secs(1);

/// The root window: the start page, then the presenter. During the talk it also decides
/// where the projector window goes.
pub struct Console {
    phase: Phase,
    monitors: Monitors,
    placement: Arc<Mutex<Placement>>,
    /// The monitor the presenter was last put on, or `None` while its monitor is missing.
    presenter_on: Option<String>,
    /// The monitor list last checked for missing requested monitors.
    checked: Vec<String>,
    /// The decks presented last, which the start page offers.
    recent: Recent,
}

/// What the root window shows.
enum Phase {
    /// The start page, until it starts the talk.
    Setup(Box<Setup>),
    /// The talk, shared with the projector window.
    Talk(Arc<Mutex<Stage>>),
}

/// Where the two windows go. Both windows share it, because a covered window stops running
/// its ui, and either window may get Alt+m.
struct Placement {
    projector: Projector,
    /// The monitor the presenter goes fullscreen on, or `None` for a normal window.
    presenter: Option<String>,
    /// The monitor name the projector window is on, `window` when windowed, or `None` while it is closed.
    shown_on: Option<String>,
    /// The monitor the presenter window is on, as winit last reported it.
    presenter_at: Option<String>,
}

impl Placement {
    /// The monitor the audience is fullscreen on, if any.
    fn audience_monitor(&self) -> Option<&str> {
        match self.projector {
            Projector::Monitor { .. } => self.shown_on.as_deref(),
            Projector::Windowed => None,
        }
    }

    /// Sends a fullscreen audience to the next monitor among `names` that the presenter is
    /// not on, and returns its index. From then on the audience asks for that monitor by name.
    fn move_audience(&mut self, names: &[String]) -> Option<usize> {
        let Projector::Monitor { requested } = &mut self.projector else {
            return None;
        };
        let next = next_audience_monitor(
            names,
            self.shown_on.as_deref(),
            self.presenter_at.as_deref(),
        )?;
        *requested = Some(names[next].clone());
        self.shown_on = Some(names[next].clone());
        Some(next)
    }
}

/// Applies the keys pressed in `ui`'s window to `stage`, then quits or moves the audience
/// among the monitors `names` if asked.
fn handle_keys(ui: &egui::Ui, stage: &mut Stage, placement: &Mutex<Placement>, names: &[String]) {
    match stage.handle_keys(ui) {
        Outcome::Quit => {
            ui.ctx()
                .send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
        Outcome::MoveAudience => {
            let moved = placement
                .lock()
                .expect("placement poisoned")
                .move_audience(names);
            if let Some(index) = moved {
                ui.ctx()
                    .send_viewport_cmd_to(projector_id(), egui::ViewportCommand::SetMonitor(index));
            }
        }
        Outcome::Continue => {}
    }
}

impl Console {
    /// The start page, for a talk on `monitors` that offers the `recent` decks.
    pub fn new(monitors: Monitors, recent: Recent) -> Self {
        Self {
            phase: Phase::Setup(Box::default()),
            monitors,
            placement: Arc::new(Mutex::new(Placement {
                projector: Projector::Monitor { requested: None },
                presenter: None,
                shown_on: None,
                presenter_at: None,
            })),
            presenter_on: None,
            checked: Vec::new(),
            recent,
        }
    }

    /// Starts the talk `launch` describes, on its opened `deck`, with textures going to `ctx`
    /// and the presenter preferences from `storage`.
    pub fn start(
        &mut self,
        launch: Launch,
        deck: Deck,
        ctx: &egui::Context,
        storage: Option<&dyn eframe::Storage>,
    ) {
        let repaint: Repaint = Arc::new({
            let ctx = ctx.clone();
            move || {
                ctx.request_repaint_of(egui::ViewportId::ROOT);
                ctx.request_repaint_of(projector_id());
            }
        });
        let reloads = watch::watch(
            launch.watched(),
            RELOAD_POLL,
            {
                let launch = launch.clone();
                move || launch.open()
            },
            {
                let repaint = repaint.clone();
                move || repaint()
            },
        );
        let stage = Stage::new(deck, Settings::load(storage), reloads, ctx.clone(), repaint);
        self.recent.add(&launch.deck);
        let mut placement = self.placement.lock().expect("placement poisoned");
        placement.projector = launch.audience;
        placement.presenter = launch.presenter;
        drop(placement);
        self.phase = Phase::Talk(Arc::new(Mutex::new(stage)));
    }

    /// Warns about each requested monitor that is not connected, once per monitor list.
    fn report_missing(&mut self, names: &[String]) {
        if names == self.checked {
            return;
        }
        let placement = self.placement.lock().expect("placement poisoned");
        let missing = [
            (
                placement.projector.requested(),
                "there is no audience window",
            ),
            (
                placement.presenter.as_deref(),
                "the presenter stays a normal window",
            ),
        ];
        for (monitor, so) in missing {
            if let Some(monitor) = monitor
                && !names.iter().any(|name| name == monitor)
            {
                log::warn!(
                    "monitor {monitor} is not connected, so {so}. Connected: {}",
                    names.join(", ")
                );
            }
        }
        self.checked = names.to_vec();
    }

    /// Puts the presenter fullscreen on its monitor whenever that monitor comes back.
    fn place_presenter(&mut self, ctx: &egui::Context, names: &[String]) {
        let Some(monitor) = self
            .placement
            .lock()
            .expect("placement poisoned")
            .presenter
            .clone()
        else {
            return;
        };
        match names.iter().position(|name| *name == monitor) {
            Some(index) if self.presenter_on.as_ref() != Some(&monitor) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::SetMonitor(index));
                self.presenter_on = Some(monitor);
            }
            Some(_) => {}
            None => self.presenter_on = None,
        }
    }

    fn show_projector(&mut self, ctx: &egui::Context, stage: &Arc<Mutex<Stage>>, names: &[String]) {
        let builder = egui::ViewportBuilder::default()
            .with_title("ppm audience")
            .with_app_id("ppm");
        let mut placement = self.placement.lock().expect("placement poisoned");
        let placed = match &placement.projector {
            Projector::Windowed => Some((builder, String::from("window"))),
            Projector::Monitor { requested } => {
                audience_monitor(names, requested.as_deref(), placement.presenter.as_deref()).map(
                    |index| {
                        // egui only moves the window when the index changes. A replug can
                        // change the monitor behind the same index.
                        if placement
                            .shown_on
                            .as_ref()
                            .is_some_and(|on| *on != names[index])
                        {
                            ctx.send_viewport_cmd_to(
                                projector_id(),
                                egui::ViewportCommand::SetMonitor(index),
                            );
                        }
                        (builder.with_monitor(index), names[index].clone())
                    },
                )
            }
        };
        let Some((builder, monitor)) = placed else {
            // Not showing the viewport this frame closes it, so it cannot land on the laptop panel.
            if placement.shown_on.take().is_some() {
                stage
                    .lock()
                    .expect("stage poisoned")
                    .close(Window::Projector);
            }
            return;
        };
        placement.shown_on = Some(monitor);
        drop(placement);
        let stage = stage.clone();
        let monitors = self.monitors.clone();
        let placement = self.placement.clone();
        ctx.show_viewport_deferred(projector_id(), builder, move |ui, _class| {
            // After an unplug the compositor can move this window over the presenter. A covered
            // Wayland window gets no frame callbacks, so the presenter's ui, which closes this
            // window, stops running. Leaving fullscreen uncovers it.
            let names = names_of(&monitors.lock().expect("monitor list poisoned"));
            let (has_place, audience) = {
                let placement = placement.lock().expect("placement poisoned");
                let has_place = placement
                    .projector
                    .has_place(&names, placement.presenter.as_deref());
                (has_place, placement.audience_monitor().map(String::from))
            };
            if !has_place {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            let mut stage = stage.lock().expect("stage poisoned");
            handle_keys(ui, &mut stage, &placement, &names);
            stage.draw(ui, Window::Projector, audience.as_deref());
        });
    }
}

impl eframe::App for Console {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let list = self.monitors.lock().expect("monitor list poisoned").clone();
        let stage = match &mut self.phase {
            Phase::Setup(setup) => {
                let here = frame
                    .winit_window()
                    .and_then(|window| window.current_monitor())
                    .and_then(|monitor| monitor.name());
                // Wayland names the window's monitor after it first shows, and no event repaints then.
                if here.is_none() {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                if let Some((launch, deck)) = setup.show(ui, &list, here.as_deref(), &self.recent) {
                    self.start(launch, deck, ui.ctx(), frame.storage());
                    ui.ctx().request_repaint();
                }
                return;
            }
            Phase::Talk(stage) => stage.clone(),
        };
        let names = names_of(&list);
        self.report_missing(&names);
        self.place_presenter(ui.ctx(), &names);
        let audience = {
            let mut placement = self.placement.lock().expect("placement poisoned");
            placement.presenter_at = frame
                .winit_window()
                .and_then(|window| window.current_monitor())
                .and_then(|monitor| monitor.name());
            placement.audience_monitor().map(String::from)
        };
        {
            let mut stage = stage.lock().expect("stage poisoned");
            handle_keys(ui, &mut stage, &self.placement, &names);
            stage.draw(ui, Window::Main, audience.as_deref());
        }
        self.show_projector(ui.ctx(), &stage, &names);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.recent.save(storage);
        if let Phase::Talk(stage) = &self.phase {
            stage
                .lock()
                .expect("stage poisoned")
                .settings()
                .save(storage);
        }
    }
}

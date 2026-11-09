//! The root window, and the projector window it opens, moves and closes.

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::monitors::{Monitors, audience_monitor};
use crate::stage::{Stage, Window};
use crate::talk::Outcome;

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
    /// Whether the projector window has somewhere to go among `names`.
    fn has_place(&self, names: &[String]) -> bool {
        match self {
            Projector::Windowed => true,
            Projector::Monitor { requested } => {
                audience_monitor(names, requested.as_deref()).is_some()
            }
        }
    }
}

/// The viewport of the projector window.
pub fn projector_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("projector")
}

/// The root window. Each frame it also decides where the projector window goes.
pub struct Console {
    stage: Arc<Mutex<Stage>>,
    monitors: Monitors,
    projector: Projector,
    /// The monitor name the projector window is on, or `None` while it is closed.
    shown_on: Option<String>,
    /// The monitor list a missing `--audience` connector was last reported for.
    reported_missing: Vec<String>,
}

impl Console {
    /// A console over `stage`, placing the projector window by `projector` among `monitors`.
    pub fn new(stage: Arc<Mutex<Stage>>, monitors: Monitors, projector: Projector) -> Self {
        Self {
            stage,
            monitors,
            projector,
            shown_on: None,
            reported_missing: Vec::new(),
        }
    }

    fn show_projector(&mut self, ctx: &egui::Context) {
        let builder = egui::ViewportBuilder::default()
            .with_title("ppm audience")
            .with_app_id("ppm");
        let placed = match &self.projector {
            Projector::Windowed => Some((builder, String::from("window"))),
            Projector::Monitor { requested } => {
                let names = self.monitors.lock().expect("monitor list poisoned").clone();
                if let Some(requested) = requested
                    && !names.contains(requested)
                    && names != self.reported_missing
                {
                    log::warn!(
                        "monitor {requested} is not connected, so there is no audience window. Connected: {}",
                        names.join(", ")
                    );
                    self.reported_missing.clone_from(&names);
                }
                audience_monitor(&names, requested.as_deref()).map(|index| {
                    // egui only moves the window when the index changes. A replug can
                    // change the monitor behind the same index.
                    if self.shown_on.as_ref().is_some_and(|on| *on != names[index]) {
                        ctx.send_viewport_cmd_to(
                            projector_id(),
                            egui::ViewportCommand::SetMonitor(index),
                        );
                    }
                    (builder.with_monitor(index), names[index].clone())
                })
            }
        };
        let Some((builder, monitor)) = placed else {
            // Not showing the viewport this frame closes it, so it cannot land on the laptop panel.
            if self.shown_on.take().is_some() {
                self.stage
                    .lock()
                    .expect("stage poisoned")
                    .close(Window::Projector);
            }
            return;
        };
        self.shown_on = Some(monitor);
        let stage = self.stage.clone();
        let monitors = self.monitors.clone();
        let projector = self.projector.clone();
        ctx.show_viewport_deferred(projector_id(), builder, move |ui, _class| {
            // After an unplug the compositor can move this window over the presenter. A covered
            // Wayland window gets no frame callbacks, so the presenter's ui, which closes this
            // window, stops running. Leaving fullscreen uncovers it.
            let names = monitors.lock().expect("monitor list poisoned").clone();
            if !projector.has_place(&names) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            let mut stage = stage.lock().expect("stage poisoned");
            if stage.handle_keys(ui) == Outcome::Quit {
                ui.ctx()
                    .send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            }
            stage.draw(ui, Window::Projector);
        });
    }
}

impl eframe::App for Console {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        {
            let mut stage = self.stage.lock().expect("stage poisoned");
            if stage.handle_keys(ui) == Outcome::Quit {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            stage.draw(ui, Window::Main);
        }
        self.show_projector(ui.ctx());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.stage
            .lock()
            .expect("stage poisoned")
            .settings()
            .save(storage);
    }
}

//! `ppm`: a presenter console for PDF slide decks.

mod deck;
mod fonts;
mod keys;
mod logging;
mod monitors;
mod pdfpc;
mod presenter;
mod render;
mod settings;
mod stage;
mod talk;
mod timer;
mod watch;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use clap::Parser;
use eframe::{UserEvent, egui};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

use crate::deck::Deck;
use crate::logging::StderrLog;
use crate::monitors::Monitors;
use crate::presenter::{Console, Projector, projector_id};
use crate::settings::Settings;
use crate::stage::{Repaint, Stage};
use crate::timer::Goal;

/// Present a PDF slide deck.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The PDF to present.
    deck: PathBuf,
    /// Read speaker notes from this pdfpc file instead of the PDF's attachment or `<deck>.pdfpc`.
    #[arg(long, value_name = "FILE")]
    notes: Option<PathBuf>,
    /// Put the audience window on this connector, such as DP-6, instead of the first external monitor.
    #[arg(long, value_name = "CONNECTOR")]
    audience: Option<String>,
    /// Count down from this many minutes, instead of the notes file's duration.
    #[arg(long, value_name = "MINUTES", conflicts_with = "end_time")]
    duration: Option<u64>,
    /// Count down to this local time, such as 14:30, instead of the notes file's end time.
    #[arg(long, value_name = "HH:MM")]
    end_time: Option<jiff::civil::Time>,
    /// Put the presenter view fullscreen on this connector, such as eDP-1, instead of a normal window.
    #[arg(long, value_name = "CONNECTOR")]
    presenter: Option<String>,
    /// Show the audience view as a normal window, for rehearsal and screen sharing.
    #[arg(long, conflicts_with = "audience")]
    windowed_audience: bool,
}

/// How often to look for a new or missing monitor.
const MONITOR_POLL: Duration = Duration::from_secs(1);

/// How often to look for a changed deck or notes file.
const RELOAD_POLL: Duration = Duration::from_secs(1);

static LOG: StderrLog = StderrLog::new();

fn main() -> anyhow::Result<()> {
    log::set_logger(&LOG)?;
    log::set_max_level(log::LevelFilter::Warn);
    let cli = Cli::parse();
    if cli.presenter.is_some() && cli.presenter == cli.audience {
        anyhow::bail!("--audience and --presenter name the same monitor");
    }
    let goal = cli.end_time.map(Goal::EndsAt).or(cli
        .duration
        .map(|minutes| Goal::Length(Duration::from_secs(minutes * 60))));
    let open = {
        let (path, notes) = (cli.deck.clone(), cli.notes.clone());
        move || -> anyhow::Result<Deck> {
            let mut deck = Deck::open(&path, notes.as_deref())?;
            if goal.is_some() {
                deck.notes.schedule.goal = goal;
            }
            Ok(deck)
        }
    };
    let deck = open()?;
    let watched = vec![
        cli.deck.clone(),
        cli.notes.unwrap_or_else(|| deck::sidecar(&cli.deck)),
    ];
    let projector = if cli.windowed_audience {
        Projector::Windowed
    } else {
        Projector::Monitor {
            requested: cli.audience,
        }
    };
    let monitors = Monitors::default();
    let gui = Arc::new(OnceLock::new());

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ppm")
            .with_app_id("ppm"),
        // The monitor logic places the windows. A restored position would fight it.
        persist_window: false,
        ..Default::default()
    };
    let eframe = eframe::create_native(
        "ppm",
        options,
        Box::new({
            let monitors = monitors.clone();
            let gui = gui.clone();
            move |cc| {
                let ctx = cc.egui_ctx.clone();
                fonts::install(&ctx);
                // Both windows paint black, so text must be light whatever the system theme.
                ctx.set_theme(egui::Theme::Dark);
                gui.get_or_init(|| ctx.clone());
                let repaint: Repaint = Arc::new(move || {
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                    ctx.request_repaint_of(projector_id());
                });
                let reloads = watch::watch(watched, RELOAD_POLL, open, {
                    let repaint = repaint.clone();
                    move || repaint()
                });
                let stage = Stage::new(
                    deck,
                    Settings::load(cc.storage),
                    reloads,
                    cc.egui_ctx.clone(),
                    repaint,
                );
                Ok(Box::new(Console::new(
                    Arc::new(Mutex::new(stage)),
                    monitors,
                    projector,
                    cli.presenter,
                )))
            }
        }),
        &event_loop,
    );
    event_loop.run_app(&mut Windows {
        eframe,
        monitors,
        gui,
    })?;
    if LOG.failed() {
        anyhow::bail!(
            "ppm stopped after the error above. A Nix build outside NixOS needs nixGL for its GL driver: `nixGLIntel ppm DECK`"
        );
    }
    Ok(())
}

/// Runs eframe on our own winit loop, which gives access to the monitor list.
struct Windows<'a> {
    eframe: eframe::EframeWinitApplication<'a>,
    monitors: Monitors,
    /// The GUI context, once eframe has created it.
    gui: Arc<OnceLock<egui::Context>>,
}

impl Windows<'_> {
    /// Updates the monitor names. A change repaints both windows, since either may be covered.
    fn track_monitors(&self, event_loop: &ActiveEventLoop) {
        let names: Vec<String> = event_loop
            .available_monitors()
            .map(|monitor| monitor.name().unwrap_or_default())
            .collect();
        let mut current = self.monitors.lock().expect("monitor list poisoned");
        if *current != names {
            *current = names;
            if let Some(ctx) = self.gui.get() {
                ctx.request_repaint_of(egui::ViewportId::ROOT);
                ctx.request_repaint_of(projector_id());
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for Windows<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.track_monitors(event_loop);
        self.eframe.resumed(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.eframe.window_event(event_loop, id, event);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.eframe.new_events(event_loop, cause);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        self.eframe.user_event(event_loop, event);
    }

    /// Dropped on purpose: raw mouse motion anywhere on screen would repaint the app.
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, _: DeviceEvent) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.track_monitors(event_loop);
        self.eframe.about_to_wait(event_loop);
        // winit has no hotplug event, and a new output does not wake the loop. Poll.
        let poll = Instant::now() + MONITOR_POLL;
        match event_loop.control_flow() {
            ControlFlow::Wait => event_loop.set_control_flow(ControlFlow::WaitUntil(poll)),
            ControlFlow::WaitUntil(deadline) if deadline > poll => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(poll));
            }
            ControlFlow::WaitUntil(_) | ControlFlow::Poll => {}
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.suspended(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.memory_warning(event_loop);
    }
}

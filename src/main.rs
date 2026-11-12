//! `ppm`: a presenter console for PDF slide decks.

mod deck;
mod desktop;
mod fonts;
mod keys;
mod launch;
mod logging;
mod monitors;
mod pdfpc;
mod presenter;
mod render;
mod settings;
mod setup;
mod stage;
mod talk;
mod timer;
mod watch;
mod xdg;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use clap::Parser;
use eframe::{UserEvent, egui};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

use crate::launch::Launch;
use crate::monitors::{Monitor, Monitors};
use crate::presenter::{Console, Projector, projector_id};
use crate::settings::Recent;
use crate::timer::{Countdown, Goal};

/// Present a PDF slide deck.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The PDF to present. Without it, a start page asks for one.
    deck: Option<PathBuf>,
    /// Read speaker notes from this pdfpc file instead of the PDF's attachment or `<deck>.pdfpc`.
    #[arg(long, value_name = "FILE", requires = "deck")]
    notes: Option<PathBuf>,
    /// Put the audience window on this connector, such as DP-6, instead of the first external monitor.
    #[arg(long, value_name = "CONNECTOR", requires = "deck")]
    audience: Option<String>,
    /// Count down from this many minutes, instead of the notes file's duration.
    #[arg(
        long,
        value_name = "MINUTES",
        conflicts_with = "end_time",
        requires = "deck"
    )]
    duration: Option<u64>,
    /// Count down to this local time, such as 14:30, instead of the notes file's end time.
    #[arg(long, value_name = "HH:MM", requires = "deck")]
    end_time: Option<jiff::civil::Time>,
    /// Put the presenter view fullscreen on this connector, such as eDP-1, instead of a normal window.
    #[arg(long, value_name = "CONNECTOR", requires = "deck")]
    presenter: Option<String>,
    /// Show the audience view as a normal window, for rehearsal and screen sharing.
    #[arg(long, conflicts_with = "audience", requires = "deck")]
    windowed_audience: bool,
    /// List ppm in app launchers and "Open with" menus, by writing a desktop entry that starts
    /// this program to ~/.local/share/applications. For installs without a package manager.
    #[arg(long, exclusive = true)]
    install_desktop_entry: bool,
}

/// How often to look for a new or missing monitor.
const MONITOR_POLL: Duration = Duration::from_secs(1);

fn main() -> anyhow::Result<()> {
    logging::install()?;
    let cli = Cli::parse();
    if cli.install_desktop_entry {
        let path = desktop::install()?;
        println!("Wrote {}", path.display());
        return Ok(());
    }
    if cli.presenter.is_some() && cli.presenter == cli.audience {
        anyhow::bail!("--audience and --presenter name the same monitor");
    }
    logging::keep_file();
    log::info!(
        "ppm {}, arguments {:?}",
        env!("CARGO_PKG_VERSION"),
        std::env::args().skip(1).collect::<Vec<_>>()
    );
    let talk = match cli.deck {
        Some(deck) => {
            let countdown = cli
                .end_time
                .map(Goal::EndsAt)
                .or(cli
                    .duration
                    .map(|minutes| Goal::Length(Duration::from_secs(minutes * 60))))
                .map_or(Countdown::FromNotes, Countdown::Fixed);
            let audience = if cli.windowed_audience {
                Projector::Windowed
            } else {
                Projector::Monitor {
                    requested: cli.audience,
                }
            };
            let launch = Launch {
                deck,
                notes: cli.notes,
                audience,
                presenter: cli.presenter,
                countdown,
            };
            let deck = launch.open()?;
            Some((launch, deck))
        }
        None => None,
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
                fonts::install(&cc.egui_ctx);
                // Both windows paint black, so text must be light whatever the system theme.
                cc.egui_ctx.set_theme(egui::Theme::Dark);
                cc.egui_ctx
                    .all_styles_mut(|style| square_corners(&mut style.visuals));
                gui.get_or_init(|| cc.egui_ctx.clone());
                let mut console = Console::new(monitors, Recent::load(cc.storage));
                if let Some((launch, deck)) = talk {
                    console.start(launch, deck, &cc.egui_ctx, cc.storage);
                }
                Ok(Box::new(console))
            }
        }),
        &event_loop,
    );
    event_loop.run_app(&mut Windows {
        eframe,
        monitors,
        gui,
    })?;
    if logging::failed() {
        anyhow::bail!(
            "ppm stopped after the error above. A Nix build outside NixOS needs nixGL for its GL driver: `nixGLIntel ppm DECK`"
        );
    }
    Ok(())
}

/// Removes the rounding from every widget, window and menu.
fn square_corners(visuals: &mut egui::Visuals) {
    visuals.window_corner_radius = egui::CornerRadius::ZERO;
    visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    let widgets = &mut visuals.widgets;
    for look in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        look.corner_radius = egui::CornerRadius::ZERO;
    }
}

/// Runs eframe on our own winit loop, which gives access to the monitor list.
struct Windows<'a> {
    eframe: eframe::EframeWinitApplication<'a>,
    monitors: Monitors,
    /// The GUI context, once eframe has created it.
    gui: Arc<OnceLock<egui::Context>>,
}

impl Windows<'_> {
    /// Updates the monitor list. A change repaints both windows, since either may be covered.
    fn track_monitors(&self, event_loop: &ActiveEventLoop) {
        let list: Vec<Monitor> = event_loop
            .available_monitors()
            .map(|monitor| {
                let scale = monitor.scale_factor();
                let size = monitor.size();
                let corner = monitor.position().to_logical::<f32>(scale);
                let extent = size.to_logical::<f32>(scale);
                Monitor {
                    name: monitor.name().unwrap_or_default(),
                    pixels: [size.width, size.height],
                    area: egui::Rect::from_min_size(
                        egui::pos2(corner.x, corner.y),
                        egui::vec2(extent.width, extent.height),
                    ),
                }
            })
            .collect();
        let mut current = self.monitors.lock().expect("monitor list poisoned");
        if *current != list {
            let described: Vec<String> = list
                .iter()
                .map(|m| format!("{} {}x{}", m.name, m.pixels[0], m.pixels[1]))
                .collect();
            log::info!("monitors: {}", described.join(", "));
            *current = list;
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

//! `ppm`: a presenter console for PDF slide decks.

mod deck;
mod keys;
mod presenter;
mod render;
mod talk;

use std::path::PathBuf;

use clap::Parser;
use eframe::{UserEvent, egui};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use crate::deck::Deck;
use crate::presenter::Presenter;
use crate::render::Renderer;

/// Present a PDF slide deck.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The PDF to present.
    deck: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let deck = Deck::open(&cli.deck)?;

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ppm")
            .with_app_id("ppm"),
        ..Default::default()
    };
    let eframe = eframe::create_native(
        "ppm",
        options,
        Box::new(|cc| {
            let renderer = Renderer::new(cc.egui_ctx.clone(), deck.pdf.clone());
            Ok(Box::new(Presenter::new(deck, renderer)))
        }),
        &event_loop,
    );
    event_loop.run_app(&mut Windows { eframe })?;
    Ok(())
}

/// Runs eframe on our own winit loop, which gives access to the monitor list.
struct Windows<'a> {
    eframe: eframe::EframeWinitApplication<'a>,
}

impl ApplicationHandler<UserEvent> for Windows<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
        self.eframe.about_to_wait(event_loop);
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

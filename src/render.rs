//! Rendering pages off the GUI thread, each at an exact size in physical pixels.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::Affine;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings, render_into};

use crate::deck::PageSize;

/// A page at an exact size in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    /// Zero-based page index.
    pub page: usize,
    /// Width in physical pixels.
    pub width: u16,
    /// Height in physical pixels.
    pub height: u16,
}

/// Where a page goes on screen, and the render that fills that spot 1:1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The page's rect in points. Its corners sit on physical pixel boundaries.
    pub rect: egui::Rect,
    /// The render to draw in `rect`.
    pub target: Target,
}

/// Fits page `index` of size `page` inside `area`, centered, on whole physical pixels.
///
/// Returns `None` when the page or the area has no size.
pub fn place(
    index: usize,
    page: PageSize,
    area: egui::Rect,
    pixels_per_point: f32,
) -> Option<Placement> {
    let area_px = area.size() * pixels_per_point;
    if page.width <= 0.0 || page.height <= 0.0 || area_px.x < 1.0 || area_px.y < 1.0 {
        return None;
    }
    let scale = (area_px.x / page.width).min(area_px.y / page.height);
    // The tolerance keeps float error from costing an exact fit a pixel row.
    let size_px = (egui::vec2(page.width, page.height) * scale + egui::Vec2::splat(0.01))
        .floor()
        .min(area_px.floor())
        .clamp(
            egui::Vec2::splat(1.0),
            egui::Vec2::splat(f32::from(u16::MAX)),
        );
    let min_px = (area.min.to_vec2() * pixels_per_point + (area_px - size_px) / 2.0).round();
    Some(Placement {
        rect: egui::Rect::from_min_size(
            (min_px / pixels_per_point).to_pos2(),
            size_px / pixels_per_point,
        ),
        target: Target {
            page: index,
            width: size_px.x as u16,
            height: size_px.y as u16,
        },
    })
}

/// A finished render on its way to the GUI thread.
struct Rendered {
    target: Target,
    image: egui::ColorImage,
}

/// Renders pages on a worker thread and keeps the textures still in use.
pub struct Renderer {
    ctx: egui::Context,
    requests: Sender<Vec<Target>>,
    results: Receiver<Rendered>,
    wanted: Vec<Target>,
    textures: HashMap<Target, egui::TextureHandle>,
}

impl Renderer {
    /// Starts a worker that renders pages of `pdf` and calls `repaint` after each one.
    /// Textures go to `ctx`.
    ///
    /// # Panics
    ///
    /// Panics if the operating system cannot start a thread.
    pub fn new(ctx: egui::Context, pdf: Arc<Pdf>, repaint: impl Fn() + Send + 'static) -> Self {
        let (requests, jobs) = mpsc::channel();
        let (done, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("render".into())
            .spawn(move || work(&pdf, &jobs, &done, &repaint))
            .expect("cannot start the render thread");
        Self {
            ctx,
            requests,
            results,
            wanted: Vec::new(),
            textures: HashMap::new(),
        }
    }

    /// Asks for `wanted`, most urgent first. This replaces the previous request,
    /// so the worker drops targets that are no longer wanted.
    pub fn request(&mut self, wanted: Vec<Target>) {
        self.receive();
        if wanted == self.wanted {
            return;
        }
        let missing = wanted
            .iter()
            .copied()
            .filter(|target| !self.textures.contains_key(target))
            .collect();
        self.wanted = wanted;
        self.evict();
        // The worker only stops when this renderer is dropped.
        let _ = self.requests.send(missing);
    }

    /// The texture for `target`. While it renders, an older render of the same page stands in.
    pub fn texture(&self, target: Target) -> Option<&egui::TextureHandle> {
        self.textures.get(&target).or_else(|| {
            self.textures
                .iter()
                .find(|(other, _)| other.page == target.page)
                .map(|(_, texture)| texture)
        })
    }

    fn receive(&mut self) {
        while let Ok(Rendered { target, image }) = self.results.try_recv() {
            if self.wanted.contains(&target) {
                let name = format!("page {} at {}x{}", target.page, target.width, target.height);
                let texture = self
                    .ctx
                    .load_texture(name, image, egui::TextureOptions::NEAREST);
                self.textures.insert(target, texture);
            }
        }
        self.evict();
    }

    /// Keeps wanted textures, plus stand-ins for wanted pages that are not ready yet.
    fn evict(&mut self) {
        let wanted = &self.wanted;
        let ready: Vec<usize> = wanted
            .iter()
            .filter(|target| self.textures.contains_key(target))
            .map(|target| target.page)
            .collect();
        self.textures.retain(|target, _| {
            wanted.contains(target)
                || (wanted.iter().any(|w| w.page == target.page) && !ready.contains(&target.page))
        });
    }
}

/// The worker loop: always serve the newest request, most urgent target first.
fn work(pdf: &Pdf, jobs: &Receiver<Vec<Target>>, done: &Sender<Rendered>, repaint: &impl Fn()) {
    let pages = pdf.pages();
    let mut painter = Painter::new();
    let mut queue = VecDeque::new();
    loop {
        if queue.is_empty() {
            match jobs.recv() {
                Ok(job) => queue = VecDeque::from(job),
                Err(_) => return,
            }
        }
        while let Ok(job) = jobs.try_recv() {
            queue = VecDeque::from(job);
        }
        let Some(target) = queue.pop_front() else {
            continue;
        };
        let Some(page) = pages.get(target.page) else {
            continue;
        };
        let image = painter.paint(page, target);
        if done.send(Rendered { target, image }).is_err() {
            return;
        }
        repaint();
    }
}

/// hayro's caches and buffers, reused across pages of one document.
struct Painter<'a> {
    cache: RenderCache<'a>,
    interpreter: InterpreterSettings,
    settings: RenderSettings,
    context: RenderContext,
    pixmap: Pixmap,
    resources: Resources,
}

impl<'a> Painter<'a> {
    fn new() -> Self {
        Self {
            cache: RenderCache::new(),
            interpreter: InterpreterSettings::default(),
            settings: RenderSettings::default(),
            context: RenderContext::new(1, 1),
            pixmap: Pixmap::new(1, 1),
            resources: Resources::default(),
        }
    }

    /// Renders `page` to fill `target` exactly, composited onto white.
    fn paint(&mut self, page: &'a Page<'a>, target: Target) -> egui::ColorImage {
        let (width, height) = page.render_dimensions();
        let transform = Affine::scale_non_uniform(
            f64::from(target.width) / f64::from(width),
            f64::from(target.height) / f64::from(height),
        ) * page.initial_transform(true).to_kurbo();
        self.context.reset_and_resize(target.width, target.height);
        render_into(
            page,
            &self.cache,
            &self.interpreter,
            &self.settings,
            &mut self.context,
            transform,
        );
        self.context.flush();
        self.pixmap.resize(target.width, target.height);
        // Typst pages have no background fill. Clearing to white keeps them off black.
        self.context.render_with(
            &mut self.pixmap,
            &mut self.resources,
            RasterizerSettings {
                target_init: TargetInit::Clear(WHITE),
                ..Default::default()
            },
        );
        egui::ColorImage::from_rgba_premultiplied(
            [usize::from(target.width), usize::from(target.height)],
            self.pixmap.data_as_u8_slice(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::Deck;

    fn page(width: f32, height: f32) -> PageSize {
        PageSize { width, height }
    }

    #[test]
    fn placement_snaps_to_physical_pixels() {
        let area = egui::Rect::from_min_size(egui::pos2(10.3, 20.7), egui::vec2(1000.0, 700.0));
        for ppp in [1.0, 1.25, 1.5, 5.0 / 3.0, 2.0] {
            let placed = place(0, page(841.89, 473.563), area, ppp).unwrap();
            let min = placed.rect.min.to_vec2() * ppp;
            let size = placed.rect.size() * ppp;
            assert!(
                (min - min.round()).length() < 1e-3,
                "ppp {ppp}: min {min:?}"
            );
            assert!(
                (size.x - f32::from(placed.target.width)).abs() < 1e-3,
                "ppp {ppp}"
            );
            assert!(
                (size.y - f32::from(placed.target.height)).abs() < 1e-3,
                "ppp {ppp}"
            );
            assert!(
                area.expand(1.0 / ppp).contains_rect(placed.rect),
                "ppp {ppp}"
            );
        }
    }

    #[test]
    fn placement_keeps_the_aspect_ratio_and_centers() {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1200.0));
        let placed = place(3, page(841.89, 473.563), area, 1.0).unwrap();
        assert_eq!(
            placed.target,
            Target {
                page: 3,
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(placed.rect.min, egui::pos2(0.0, 60.0));
    }

    #[test]
    fn placement_needs_a_size() {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(0.0, 100.0));
        assert!(place(0, page(10.0, 10.0), area, 1.0).is_none());
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        assert!(place(0, page(0.0, 10.0), area, 1.0).is_none());
    }

    #[test]
    fn every_fixture_page_renders_at_the_requested_size_onto_white() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/basic.pdf");
        let deck = Deck::open(&path, None).unwrap();
        let pages = deck.pdf.pages();
        let mut painter = Painter::new();
        for (index, page) in pages.iter().enumerate() {
            let target = Target {
                page: index,
                width: 320,
                height: 180,
            };
            let image = painter.paint(page, target);
            assert_eq!(image.size, [320, 180]);
            let first = image.pixels[0];
            assert!(
                image.pixels.iter().any(|&p| p != first),
                "page {index} is one flat color"
            );
            let corner = image.pixels[image.pixels.len() - 1];
            assert_eq!(
                corner,
                egui::Color32::WHITE,
                "page {index} bottom-right corner"
            );
        }
    }
}

//! Loading a slide deck from a PDF file, with its speaker notes.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, bail};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::dict::keys::{EF, EMBEDDED_FILES, F, KIDS, NAMES};
use hayro::hayro_syntax::object::{Array, Dict, Stream};

use crate::pdfpc::Notes;

/// The size of a page in PDF points, after its rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

/// A parsed PDF, the size of each of its pages, and its speaker notes.
pub struct Deck {
    /// The parsed document, shared with the render worker.
    pub pdf: Arc<Pdf>,
    /// One entry per page, in page order.
    pub pages: Vec<PageSize>,
    /// The speaker notes, empty if the deck has none.
    pub notes: Notes,
    /// Why the notes beside the deck were ignored, if they did not parse.
    pub ignored_notes: Option<String>,
}

impl Deck {
    /// Reads and parses the PDF at `path`, and the notes for it.
    ///
    /// The notes come from the pdfpc file `notes` if given. Otherwise they come from an
    /// attached file whose name ends in `.pdfpc`, then from `<deck>.pdfpc` beside the PDF.
    /// A broken attachment or sidecar only logs a warning and fills `ignored_notes`,
    /// so the talk can go on.
    ///
    /// # Errors
    ///
    /// Fails if the PDF cannot be read, does not end in `%%EOF`, or does not parse,
    /// or if `notes` is given and cannot be read or parsed.
    /// The `%%EOF` check matters because hayro rebuilds a broken cross-reference
    /// table, so a half-written file could otherwise open with pages missing.
    pub fn open(path: &Path, notes: Option<&Path>) -> anyhow::Result<Self> {
        let data =
            std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        if !data.trim_ascii_end().ends_with(b"%%EOF") {
            bail!("{} is incomplete: it does not end in %%EOF", path.display());
        }
        let pdf = Pdf::new(data)
            .map_err(|err| anyhow::anyhow!("cannot parse {}: {err:?}", path.display()))?;
        let pages = pdf
            .pages()
            .iter()
            .map(|page| {
                let (width, height) = page.render_dimensions();
                PageSize { width, height }
            })
            .collect();
        let (notes, ignored_notes) = match notes {
            Some(notes) => (read_notes(notes)?, None),
            None => notes_beside(&pdf, path),
        };
        let deck = Self {
            pdf: Arc::new(pdf),
            pages,
            notes,
            ignored_notes,
        };
        log::info!(
            "opened {}: {} slides, notes on {}",
            path.display(),
            deck.pages.len(),
            deck.noted()
        );
        Ok(deck)
    }

    /// How many slides have a note.
    pub fn noted(&self) -> usize {
        (0..self.pages.len())
            .filter(|&page| self.notes.on_page(page).is_some())
            .count()
    }
}

/// Where the notes for the deck at `path` sit beside it: `<deck>.pdfpc`.
pub fn sidecar(path: &Path) -> std::path::PathBuf {
    path.with_extension("pdfpc")
}

fn read_notes(path: &Path) -> anyhow::Result<Notes> {
    let json = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Notes::from_pdfpc(&json).with_context(|| format!("cannot parse {}", path.display()))
}

/// The attached notes, else the sidecar notes, else none. Notes that do not parse come back
/// empty, with the reason.
fn notes_beside(pdf: &Pdf, path: &Path) -> (Notes, Option<String>) {
    let notes = match attached_pdfpc(pdf) {
        Some(json) => Notes::from_pdfpc(&json)
            .with_context(|| format!("cannot parse the notes attached to {}", path.display())),
        None => {
            let sidecar = sidecar(path);
            if !sidecar.exists() {
                return (Notes::default(), None);
            }
            read_notes(&sidecar)
        }
    };
    match notes {
        Ok(notes) => (notes, None),
        Err(err) => {
            log::warn!("ignoring the notes: {err:#}");
            (Notes::default(), Some(format!("{err:#}")))
        }
    }
}

/// The contents of the first attached file whose name ends in `.pdfpc`.
/// Typst names an attachment by its path relative to `--root`, so only the end is fixed.
fn attached_pdfpc(pdf: &Pdf) -> Option<Vec<u8>> {
    let root: Dict = pdf.xref().get(pdf.xref().root_id())?;
    let files: Dict = root.get::<Dict>(NAMES)?.get(EMBEDDED_FILES)?;
    pdfpc_in(&files)
}

/// Searches a node of the `/EmbeddedFiles` name tree and its kids.
fn pdfpc_in(node: &Dict) -> Option<Vec<u8>> {
    if let Some(names) = node.get::<Array>(NAMES) {
        // The array alternates names and file specifications.
        let mut entries = names.flex_iter();
        while let (Some(name), Some(spec)) = (
            entries.next::<hayro::hayro_syntax::object::String>(),
            entries.next::<Dict>(),
        ) {
            if name.as_bytes().ends_with(b".pdfpc") {
                let file: Stream = spec.get::<Dict>(EF)?.get(F)?;
                return file.decoded().ok().map(|data| data.into_owned());
            }
        }
    }
    node.get::<Array>(KIDS)?
        .iter::<Dict>()
        .find_map(|kid| pdfpc_in(&kid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn opens_a_typslides_deck() {
        let deck = Deck::open(&fixture("basic.pdf"), None).unwrap();
        assert!(deck.pages.len() >= 3);
        for page in &deck.pages {
            assert!((page.width / page.height - 16.0 / 9.0).abs() < 0.01);
        }
    }

    #[test]
    fn rejects_a_truncated_pdf() {
        let err = Deck::open(&fixture("truncated.pdf"), None).err().unwrap();
        assert!(err.to_string().contains("%%EOF"), "{err}");
    }

    #[test]
    fn reports_a_missing_file() {
        assert!(Deck::open(&fixture("missing.pdf"), None).is_err());
    }

    #[test]
    fn reads_attached_notes_before_a_sidecar() {
        let deck = Deck::open(&fixture("notes.pdf"), None).unwrap();
        assert_eq!(deck.notes.on_page(0), None);
        assert_eq!(
            deck.notes.on_page(1),
            Some("Page 2, first note.\n\nPage 2, second note.")
        );
        let expected = [
            (2, "Page 3, before the break."),
            (3, "Page 4, after the break."),
            (4, "Page 5, inside cols."),
            (5, "Page 6, inside framed."),
            (6, "Page 7, inside focus-slide."),
        ];
        for (page, note) in expected {
            assert_eq!(deck.notes.on_page(page), Some(note), "page {page}");
        }
        assert!(deck.notes.on_page(7).unwrap().contains("**bold**"));
        assert_eq!(deck.notes.on_page(8), None);
    }

    #[test]
    fn reads_a_sidecar_beside_the_deck() {
        let deck = Deck::open(&fixture("basic.pdf"), None).unwrap();
        assert_eq!(deck.notes.on_page(0), Some("Sidecar note."));
    }

    #[test]
    fn a_notes_file_wins_over_the_attachment() {
        let deck = Deck::open(&fixture("notes.pdf"), Some(&fixture("basic.pdfpc"))).unwrap();
        assert_eq!(deck.notes.on_page(0), Some("Sidecar note."));
        assert_eq!(deck.notes.on_page(1), None);
    }

    #[test]
    fn a_broken_sidecar_is_ignored_with_its_reason() {
        let dir = std::env::temp_dir().join(format!("ppm-deck-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(fixture("basic.pdf"), dir.join("talk.pdf")).unwrap();
        std::fs::write(dir.join("talk.pdfpc"), "{broken").unwrap();
        let deck = Deck::open(&dir.join("talk.pdf"), None).unwrap();
        assert_eq!(deck.notes, Notes::default());
        let why = deck.ignored_notes.unwrap();
        assert!(why.contains("talk.pdfpc"), "{why}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reports_a_missing_notes_file() {
        let err = Deck::open(&fixture("basic.pdf"), Some(&fixture("missing.pdfpc")))
            .err()
            .unwrap();
        assert!(err.to_string().contains("missing.pdfpc"), "{err}");
    }
}

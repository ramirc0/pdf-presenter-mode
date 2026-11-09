//! Loading a slide deck from a PDF file.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, bail};
use hayro::hayro_syntax::Pdf;

/// The size of a page in PDF points, after its rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

/// A parsed PDF and the size of each of its pages.
pub struct Deck {
    /// The parsed document, shared with the render worker.
    pub pdf: Arc<Pdf>,
    /// One entry per page, in page order.
    pub pages: Vec<PageSize>,
}

impl Deck {
    /// Reads and parses the PDF at `path`.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read, does not end in `%%EOF`, or does not parse.
    /// The `%%EOF` check matters because hayro rebuilds a broken cross-reference
    /// table, so a half-written file could otherwise open with pages missing.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
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
        Ok(Self {
            pdf: Arc::new(pdf),
            pages,
        })
    }
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
        let deck = Deck::open(&fixture("basic.pdf")).unwrap();
        assert!(deck.pages.len() >= 3);
        for page in &deck.pages {
            assert!((page.width / page.height - 16.0 / 9.0).abs() < 0.01);
        }
    }

    #[test]
    fn rejects_a_truncated_pdf() {
        let err = Deck::open(&fixture("truncated.pdf")).err().unwrap();
        assert!(err.to_string().contains("%%EOF"), "{err}");
    }

    #[test]
    fn reports_a_missing_file() {
        assert!(Deck::open(&fixture("missing.pdf")).is_err());
    }
}

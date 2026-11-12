//! The links on a deck's pages: where each one sits and where it goes.
//!
//! hayro does not read annotations, so this reads the PDF's objects itself.

use std::collections::HashMap;

use hayro::hayro_interpret::util::{RectExt as _, TransformExt as _};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::dict::keys::{
    A, ANNOTS, D, DEST, DESTS, F, KIDS, NAMES, RECT, S, SUBTYPE, URI,
};
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, Rect};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo;

/// Where a link goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Destination {
    /// The slide at this index, counting from 0.
    Slide(usize),
    /// A URI, such as a web page. ppm opens only some, see [`browsable`].
    Web(String),
}

/// A link on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// Where the link sits, in fractions of the page's width and height from its top left corner.
    pub area: kurbo::Rect,
    /// Where it goes.
    pub to: Destination,
}

/// The annotation flags Hidden and NoView. hayro does not draw such links, so they must not react either.
const UNSEEN: u32 = 1 << 1 | 1 << 5;

/// The links on each page of `pdf`, by page.
pub fn links_of(pdf: &Pdf) -> Vec<Vec<Link>> {
    let xref = pdf.xref();
    let catalog = xref.get::<Dict>(xref.root_id());
    // Destinations name a page by its object.
    let pages: HashMap<ObjectIdentifier, usize> = pdf
        .pages()
        .iter()
        .enumerate()
        .filter_map(|(index, page)| Some((page.raw().obj_id()?, index)))
        .collect();
    pdf.pages()
        .iter()
        .map(|page| links_on(page, &pages, catalog.as_ref()))
        .collect()
}

/// The links on `page`. `pages` maps page objects to indexes, and `catalog` holds named destinations.
fn links_on(
    page: &Page,
    pages: &HashMap<ObjectIdentifier, usize>,
    catalog: Option<&Dict>,
) -> Vec<Link> {
    let Some(annots) = page.raw().get::<Array>(ANNOTS) else {
        return Vec::new();
    };
    let (width, height) = page.render_dimensions();
    let (width, height) = (f64::from(width), f64::from(height));
    // The transform the page renders with: from PDF user space to the page, top left first.
    let to_page = page.initial_transform(true).to_kurbo();
    // `iter` would stop at the first entry that does not resolve.
    annots
        .raw_iter()
        .filter_map(|annot| {
            let annot = match annot {
                MaybeRef::Ref(id) => page.xref().get::<Dict>(id.into()),
                MaybeRef::NotRef(object) => object.into_dict(),
            }?;
            let link = annot
                .get::<Name>(SUBTYPE)
                .is_some_and(|subtype| &*subtype == b"Link".as_slice());
            let seen = annot.get::<u32>(F).unwrap_or(0) & UNSEEN == 0;
            if !link || !seen {
                return None;
            }
            let rect = to_page.transform_rect_bbox(annot.get::<Rect>(RECT)?.to_kurbo());
            let area = kurbo::Rect::new(
                rect.x0 / width,
                rect.y0 / height,
                rect.x1 / width,
                rect.y1 / height,
            );
            let to = destination(&annot, pages, catalog)?;
            Some(Link { area, to })
        })
        .collect()
}

/// Where the link annotation `annot` goes: its `/Dest`, or the target of its `/A` action.
fn destination(
    annot: &Dict,
    pages: &HashMap<ObjectIdentifier, usize>,
    catalog: Option<&Dict>,
) -> Option<Destination> {
    if let Some(dest) = annot.get::<Object>(DEST) {
        return slide_of(dest, pages, catalog).map(Destination::Slide);
    }
    let action = annot.get::<Dict>(A)?;
    match &*action.get::<Name>(S)? {
        b"GoTo" => slide_of(action.get::<Object>(D)?, pages, catalog).map(Destination::Slide),
        b"URI" => {
            let uri = action.get::<hayro::hayro_syntax::object::String>(URI)?;
            Some(Destination::Web(
                String::from_utf8_lossy(uri.as_bytes()).into_owned(),
            ))
        }
        _ => None,
    }
}

/// The address to open for the link target `uri`, if ppm opens it: `http`, `https` and `mailto`
/// links, and addresses that start with `www.`, which get `https://`. Other schemes, such as
/// `file:` or an app's own, could start any program, so they stay closed.
pub fn browsable(uri: &str) -> Option<String> {
    let uri = uri.trim();
    if uri.starts_with("www.") {
        return Some(format!("https://{uri}"));
    }
    let (scheme, rest) = uri.split_once(':')?;
    let opens = ["http", "https", "mailto"]
        .iter()
        .any(|open| scheme.eq_ignore_ascii_case(open));
    (opens && !rest.is_empty()).then(|| uri.to_owned())
}

/// The slide a destination names: an array that starts with the page, or a string naming such
/// an array in the catalog's `/Names /Dests` tree.
fn slide_of(
    dest: Object,
    pages: &HashMap<ObjectIdentifier, usize>,
    catalog: Option<&Dict>,
) -> Option<usize> {
    let array = match dest {
        Object::Array(array) => array,
        Object::String(name) => {
            let tree = catalog?.get::<Dict>(NAMES)?.get::<Dict>(DESTS)?;
            match named(&tree, name.as_bytes())? {
                Object::Array(array) => array,
                // The array may also sit in a dictionary's `/D`.
                Object::Dict(dict) => dict.get::<Array>(D)?,
                _ => return None,
            }
        }
        _ => return None,
    };
    let page = array.raw_iter().next()?.as_obj_ref()?;
    pages.get(&ObjectIdentifier::from(page)).copied()
}

/// The value under `key` in the name tree at `node`, searching its kids too.
fn named<'a>(node: &Dict<'a>, key: &[u8]) -> Option<Object<'a>> {
    if let Some(names) = node.get::<Array>(NAMES) {
        // The array alternates keys and values.
        let mut entries = names.flex_iter();
        while let (Some(name), Some(value)) = (
            entries.next::<hayro::hayro_syntax::object::String>(),
            entries.next::<Object>(),
        ) {
            if name.as_bytes() == key {
                return Some(value);
            }
        }
    }
    node.get::<Array>(KIDS)?
        .iter::<Dict>()
        .find_map(|kid| named(&kid, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Pdf {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        Pdf::new(std::fs::read(path).unwrap()).unwrap()
    }

    /// A PDF of `objects`, numbered from 1, with a correct cross-reference table.
    fn pdf_of(objects: &[&str]) -> Pdf {
        let mut pdf = String::from("%PDF-1.7\n");
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf += &format!("{} 0 obj\n{object}\nendobj\n", index + 1);
        }
        let xref = pdf.len();
        pdf += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for offset in offsets {
            pdf += &format!("{offset:010} 00000 n \n");
        }
        pdf += &format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        );
        Pdf::new(pdf.into_bytes()).unwrap()
    }

    fn near(a: kurbo::Rect, b: kurbo::Rect) -> bool {
        [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1]
            .iter()
            .all(|d| d.abs() < 1e-6)
    }

    #[test]
    fn typst_links_go_to_their_slides_in_reading_order() {
        let links = links_of(&fixture("links.pdf"));
        assert_eq!(links.len(), 3);
        let first: Vec<&Destination> = links[0].iter().map(|link| &link.to).collect();
        assert_eq!(
            first,
            [
                &Destination::Slide(2),
                &Destination::Slide(1),
                &Destination::Web("https://typst.app".into()),
                &Destination::Web("www.example.com".into()),
                &Destination::Web("file:///etc/hostname".into()),
            ]
        );
        for link in &links[0] {
            let area = link.area;
            assert!(
                0.0 <= area.x0 && area.x0 < area.x1 && area.x1 <= 1.0,
                "{area:?}"
            );
            assert!(
                0.0 <= area.y0 && area.y0 < area.y1 && area.y1 <= 1.0,
                "{area:?}"
            );
        }
        assert!(links[0][0].area.y1 <= links[0][1].area.y0);
        assert!(links[1].is_empty() && links[2].is_empty());
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert_eq!(
            browsable("https://typst.app").as_deref(),
            Some("https://typst.app")
        );
        assert_eq!(
            browsable("HTTP://example.com").as_deref(),
            Some("HTTP://example.com")
        );
        assert_eq!(
            browsable("mailto:me@example.com").as_deref(),
            Some("mailto:me@example.com")
        );
        assert_eq!(
            browsable("www.example.com").as_deref(),
            Some("https://www.example.com")
        );
        for closed in [
            "file:///etc/hostname",
            "javascript:alert(1)",
            "example.com",
            "https:",
        ] {
            assert_eq!(browsable(closed), None, "{closed}");
        }
    }

    #[test]
    fn a_goto_action_finds_its_named_destination() {
        let pdf = pdf_of(&[
            "<< /Type /Catalog /Pages 2 0 R /Names << /Dests << /Names [(end) [4 0 R /Fit]] >> >> >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 160 90] >>",
            "<< /Type /Page /Parent 2 0 R /Annots [5 0 R 6 0 R] >>",
            "<< /Type /Page /Parent 2 0 R >>",
            "<< /Type /Annot /Subtype /Link /Rect [10 10 50 30] /A << /S /GoTo /D (end) >> >>",
            "<< /Type /Annot /Subtype /Link /F 2 /Rect [60 10 90 30] /Dest [4 0 R /Fit] >>",
        ]);
        let links = links_of(&pdf);
        // The hidden second link does not count.
        assert_eq!(links[0].len(), 1);
        assert_eq!(links[0][0].to, Destination::Slide(1));
        // The page is 90 points high, so 10 to 30 from the bottom is 60 to 80 from the top.
        let area = kurbo::Rect::new(10.0 / 160.0, 60.0 / 90.0, 50.0 / 160.0, 80.0 / 90.0);
        assert!(near(links[0][0].area, area), "{:?}", links[0][0].area);
    }
}

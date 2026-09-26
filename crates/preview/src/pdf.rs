//! PDF, rendered by PDFium — the engine behind Chrome's viewer.
//!
//! The library is loaded **dynamically** at first use rather than linked. That is what lets a
//! build without the binary still compile, launch and run: the tier reports that it cannot help
//! and the ladder falls through to an icon, instead of the whole application failing to start
//! over a file format most collections do not even contain.
//!
//! No pure-Rust renderer is a real option here. The parsers (`lopdf`, `pdf-rs`) do not rasterise,
//! and the young rasterisers draw text as grey boxes. The alternatives that do work are licensed
//! wrong for a distributed application — MuPDF is AGPL, and the `pdfium` crate (not this one) is
//! GPL. PDFium itself is Apache-2.0 or BSD-3, which is clean.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use components::ComponentId;
use image::DynamicImage;
use pdfium_render::prelude::{
    PdfPageTextChars, PdfRenderConfig, PdfSearchDirection, PdfSearchOptions, Pdfium,
};

/// Illustrator files are, in practice, PDFs with extra private data, so PDFium opens them. It is
/// not guaranteed for very old `.ai` files — those fall through to the OS tier.
pub fn handles(extension: &str) -> bool {
    matches!(extension, "pdf" | "ai")
}

/// Where to look for the library, in order: beside the executable (a full install), the copy qrate
/// installed on demand, then the system's own, which the empty path stands for.
fn candidates(exe_dir: Option<&Path>, installed: Option<&Path>) -> Vec<PathBuf> {
    exe_dir
        .into_iter()
        .chain(installed)
        .map(Pdfium::pdfium_platform_library_name_at_path)
        .chain([PathBuf::new()])
        .collect()
}

fn bind() -> Option<Pdfium> {
    let exe = std::env::current_exe().ok();
    let installed = components::store().and_then(|store| store.locate(ComponentId::Pdfium));
    let bindings = candidates(exe.as_deref().and_then(Path::parent), installed.as_deref())
        .into_iter()
        .find_map(|library| {
            if library.as_os_str().is_empty() {
                Pdfium::bind_to_system_library().ok()
            } else if library.is_file() {
                Pdfium::bind_to_library(&library)
                    .map_err(|err| {
                        log::warn!("could not load PDFium from {}: {err}", library.display())
                    })
                    .ok()
            } else {
                None
            }
        });
    if bindings.is_none() {
        log::info!(
            "PDFium is not installed, so PDFs will show an icon instead of their first page"
        );
    }
    Some(Pdfium::new(bindings?))
}

/// The one PDFium instance, behind the lock that makes it safe to reach.
///
/// Both parts are load-bearing. PDFium initialises global state, so binding it more than once in
/// a process aborts — not a Rust panic that a test could catch, an immediate process death. And
/// the library is not re-entrant, so two threads rendering at once corrupts it. Previews are
/// decoded on a background pool and a gallery draws many cards, so both happen readily.
///
/// The cost is that PDF rendering is serialised. That is what the library allows, and each result
/// is cached, so it is paid once per page anybody actually looks at.
///
/// A miss is looked at again only after a component install or removal: retrying a failed load
/// on every card would be a stutter per row.
fn pdfium() -> Option<&'static Mutex<Pdfium>> {
    static INSTANCE: OnceLock<Mutex<Pdfium>> = OnceLock::new();
    static TRIED: Mutex<Option<(u64, bool)>> = Mutex::new(None);
    if let Some(pdfium) = INSTANCE.get() {
        return Some(pdfium);
    }
    components::remember(&TRIED, components::generation(), || {
        INSTANCE.get().is_some()
            || bind().is_some_and(|pdfium| INSTANCE.set(Mutex::new(pdfium)).is_ok())
    });
    INSTANCE.get()
}

/// Take the lock, recovering from a previous render having panicked while holding it: a poisoned
/// lock would otherwise disable PDF previews for the rest of the session.
fn locked() -> Option<std::sync::MutexGuard<'static, Pdfium>> {
    Some(pdfium()?.lock().unwrap_or_else(|err| err.into_inner()))
}

/// How many pages the document has, for the viewer's page controls. `None` when PDFium is absent
/// or the file will not open, which the caller reads as "no paging".
pub fn page_count(path: &Path) -> Option<usize> {
    let pdfium = locked()?;
    let document = pdfium.load_pdf_from_file(path, None).ok()?;
    usize::try_from(document.pages().len()).ok()
}

/// The text layer of every page, one page per line break. Empty for a scan that was never OCR'd.
///
/// ponytail: holds the PDFium lock for the whole document, so a 500-page PDF stalls gallery
/// thumbnails until it is read. Per-page locking if that shows up in a real collection.
pub fn text(path: &Path) -> Option<String> {
    let pdfium = locked()?;
    let document = pdfium.load_pdf_from_file(path, None).ok()?;
    Some(
        document
            .pages()
            .iter()
            .filter_map(|page| page.text().ok().map(|text| text.all()))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Whether the document carries a text layer at all.
///
/// A scan that was never OCR'd has none, and "no matches" is a misleading answer to every search
/// over it — the words are on the page, they are just not text. Short-circuits on the first page
/// with any characters, so it is cheap whenever the answer is yes and only walks the whole file
/// to prove a no.
pub fn has_text_layer(path: &Path) -> Option<bool> {
    let pdfium = locked()?;
    let document = pdfium.load_pdf_from_file(path, None).ok()?;
    Some(
        document
            .pages()
            .iter()
            .any(|page| page.text().is_ok_and(|text| !text.chars().is_empty())),
    )
}

/// How much of the surrounding line a hit carries with it, in characters either side.
///
/// Deliberately more than a row shows. How much fits depends on how wide the reader has dragged
/// the find panel, which changes without the document changing — so the generous window is cut
/// here once and trimmed to the panel at render, rather than re-searching on every drag pixel.
const CONTEXT: usize = 120;

/// One search hit: which page it is on, where on that page, and the words around it.
#[derive(Clone)]
pub struct Match {
    /// Zero-based, so it indexes the viewer's pages directly.
    pub page: usize,
    /// The hit's box as fractions of the page, measured from its **top-left** — PDF counts up from
    /// the bottom, and converting here means the viewer never has to know that.
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    /// The page's width over its height. The viewer needs it to work out where inside its
    /// letterbox the page was actually drawn before it can place the box above.
    pub aspect: f32,
    /// The hit's surroundings — see [`CONTEXT`] — and where the hit itself sits inside them. More
    /// than a row displays; the caller trims it to whatever it has room for.
    pub line: String,
    pub at: Range<usize>,
}

/// Every hit for `needle`, in document order.
///
/// PDFium does the matching rather than a scan over extracted strings: it is the only way to learn
/// *where* on the page a hit is, and it compares using the document's own text ordering rather
/// than a flattened copy of it.
pub fn search(path: &Path, needle: &str) -> Option<Vec<Match>> {
    let pdfium = locked()?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|err| log::warn!("could not open {}: {err}", path.display()))
        .ok()?;
    let options = PdfSearchOptions::new();
    let mut hits = Vec::new();

    for (index, page) in document.pages().iter().enumerate() {
        let (Ok(text), width, height) = (page.text(), page.width(), page.height()) else {
            continue;
        };
        if width.value <= 0.0 || height.value <= 0.0 {
            continue;
        }
        let Ok(found) = text.search(needle, &options) else {
            continue;
        };
        let chars = text.chars();
        let length = needle.chars().count();
        for segments in found.iter(PdfSearchDirection::SearchForward) {
            // ponytail: the first segment only, so a hit broken across a line wrap highlights its
            // first half. Widen to every segment if wrapped hits turn out to be common.
            let Ok(segment) = segments.first() else {
                continue;
            };
            let bounds = segment.bounds();
            let (line, at) = match segment.chars().ok().and_then(|c| c.first_char_index()) {
                Some(start) => context(&chars, start, length),
                None => (String::new(), 0..0),
            };
            hits.push(Match {
                page: index,
                left: bounds.left().value / width.value,
                top: 1.0 - bounds.top().value / height.value,
                width: bounds.width().value / width.value,
                height: bounds.height().value / height.value,
                aspect: width.value / height.value,
                line,
                at,
            });
        }
    }
    Some(hits)
}

/// The [`CONTEXT`] characters either side of a hit, read straight out of PDFium's character list,
/// plus where the hit itself sits in the result.
///
/// Reading characters by index rather than slicing a rectangle out of the page is what keeps this
/// in reading order. A band across the page also catches whatever sits *beside* the line — another
/// column, a table cell, a marginal note — and hands that back interleaved and unreadable.
fn context(chars: &PdfPageTextChars, start: usize, length: usize) -> (String, Range<usize>) {
    // Control characters become spaces so a row cannot turn into two lines.
    let read = |range: Range<usize>| -> String {
        range
            .filter_map(|index| chars.get(index).ok()?.unicode_char())
            .map(|char| if char.is_control() { ' ' } else { char })
            .collect()
    };
    let end = (start + length).min(chars.len());
    let head = read(start.saturating_sub(CONTEXT)..start);
    let word = read(start..end);
    let tail = read(end..(end + CONTEXT).min(chars.len()));

    // Built in three pieces so the highlight's byte range falls out of their lengths — searching
    // the joined string for the match again would reintroduce every case-folding trap.
    let at = head.len()..head.len() + word.len();
    (format!("{head}{word}{tail}"), at)
}
/// One page, rendered to fit `max_edge`.
///
/// Page-per-item is deliberately *not* how this works. Tropy splits a multi-page PDF into one item
/// per page, which suits a tool whose rows *are* files; qrate's rows come from the spreadsheet, so
/// a page is something the viewer navigates rather than something the data model knows about.
pub fn decode(path: &Path, max_edge: u32, page: usize) -> Option<DynamicImage> {
    let pdfium = locked()?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|err| log::warn!("could not open {}: {err}", path.display()))
        .ok()?;
    let index = i32::try_from(page).ok()?;
    let page = document.pages().get(index).ok()?;

    // Sized by the longer edge so a landscape page is not rendered as a sliver, and never
    // enlarged past what the caller asked for.
    let config = PdfRenderConfig::new()
        .set_maximum_width(max_edge.try_into().ok()?)
        .set_maximum_height(max_edge.try_into().ok()?);
    page.render_with_config(&config)
        .map_err(|err| log::warn!("could not render {}: {err}", path.display()))
        .ok()
        .and_then(|bitmap| bitmap.as_image().ok())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::pdf;

    #[test]
    fn looks_beside_the_executable_then_in_components_then_the_system() {
        let exe = Path::new("/qrate");
        let installed = Path::new("/data/components/pdfium/chromium-7881");
        let found = pdf::candidates(Some(exe), Some(installed));
        assert_eq!(found.len(), 3);
        assert!(found[0].starts_with(exe));
        assert!(found[1].starts_with(installed));
        assert_eq!(found[2], PathBuf::new(), "the system library comes last");
        assert_eq!(pdf::candidates(None, None), [PathBuf::new()]);
    }

    #[test]
    fn claims_pdf_and_illustrator_only() {
        assert!(pdf::handles("pdf") && pdf::handles("ai"));
        assert!(!pdf::handles("jpg") && !pdf::handles("eps"));
    }

    /// The tier has to be inert without the library rather than panicking or aborting — that is
    /// the whole reason for binding dynamically, and it is the state most development checkouts
    /// are in. With PDFium present this instead proves a real page renders.
    #[test]
    fn renders_a_page_or_declines_cleanly_without_the_library() {
        // A minimal one-page PDF, written by hand so the test needs no binary fixture.
        let pdf = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]>>endobj\n\
trailer<</Root 1 0 R>>";
        let path = std::env::temp_dir().join("qrate-pdf-probe.pdf");
        std::fs::write(&path, pdf).unwrap();

        let rendered = pdf::decode(&path, 128, 0);
        match (super::pdfium().is_some(), rendered) {
            (true, Some(page)) => {
                assert!(
                    page.width() <= 128 && page.height() <= 128,
                    "size cap applied"
                );
                assert!(page.width() > 0 && page.height() > 0, "a page with area");
                // The MediaBox is 200x100, so the cap has to land on the long edge.
                assert!(
                    page.width() > page.height(),
                    "landscape page stayed landscape"
                );
            }
            (true, None) => panic!("PDFium is installed but a valid one-page PDF did not render"),
            // The state a checkout without `scripts/fetch-binaries.sh` is in, and the one the
            // dynamic binding exists to survive: no library, no panic, no preview.
            (false, rendered) => {
                assert!(rendered.is_none(), "no library can only mean no page");
                eprintln!("skipping the render check: PDFium is not installed");
            }
        }

        let _ = std::fs::remove_file(&path);
    }

    /// A PDF carrying a real text layer, built rather than committed as a fixture. Two pages with
    /// different words, so an extraction that quietly returned page one twice is visible — and in
    /// opposite orientations, so is a hit measured against the wrong page.
    fn document_with_text(name: &str) -> std::path::PathBuf {
        let draw = |text: &str| format!("BT /F1 12 Tf 20 50 Td ({text}) Tj ET");
        let (one, two) = (draw("Aderman family"), draw("Kitsilano beach"));
        let pdf = format!(
            "%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R 5 0 R]/Count 2>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Resources<</Font<</F1 4 0 R>>>>/Contents 6 0 R>>endobj\n\
4 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n\
5 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 200]/Resources<</Font<</F1 4 0 R>>>>/Contents 7 0 R>>endobj\n\
6 0 obj<</Length {}>>stream\n{one}\nendstream endobj\n\
7 0 obj<</Length {}>>stream\n{two}\nendstream endobj\n\
trailer<</Root 1 0 R>>",
            one.len(),
            two.len()
        );
        let path = std::env::temp_dir().join(format!("qrate-pdf-text-{name}.pdf"));
        std::fs::write(&path, pdf).unwrap();
        path
    }

    /// A hit on the second page must report page 1, not page 0 — the viewer navigates by that
    /// number, so an off-by-one sends the reader to the wrong page with a highlight on nothing.
    #[test]
    fn hits_report_the_page_they_are_actually_on() {
        let path = document_with_text("context");

        let Some(hits) = pdf::search(&path, "Kitsilano") else {
            eprintln!("skipping the page check: PDFium is not installed");
            let _ = std::fs::remove_file(&path);
            return;
        };
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page, 1, "the word is only on the second page");
        // That page is 100x200, the opposite orientation to the first, so a hit that had silently
        // been measured against page one would carry the wrong aspect.
        assert!((hits[0].aspect - 0.5).abs() < 0.01);

        let _ = std::fs::remove_file(&path);
    }

    /// A hit has to come back with readable context and a highlight range that lands on the match.
    ///
    /// The context used to be sliced out of a rectangle spanning the page, which also caught
    /// whatever sat beside the line and returned it interleaved — the row rendered as punctuation
    /// soup. Asserting the surrounding words are intact is what pins that down.
    #[test]
    fn a_hit_carries_readable_context_and_a_range_that_lands_on_it() {
        let path = document_with_text("pages");

        let Some(hits) = pdf::search(&path, "family") else {
            eprintln!("skipping the search check: PDFium is not installed");
            let _ = std::fs::remove_file(&path);
            return;
        };

        assert_eq!(hits.len(), 1, "one hit, on the page that has the word");
        let hit = &hits[0];
        assert_eq!(hit.page, 0);

        // The range must slice the match itself out of the line — off by a byte and the panel
        // highlights the wrong word, or panics on a char boundary.
        assert_eq!(&hit.line[hit.at.clone()], "family");
        // And the words either side have to survive, in order.
        assert!(
            hit.line.contains("Aderman family"),
            "context is the real neighbouring text, got {:?}",
            hit.line
        );

        // The box is a fraction of the page, so it has to sit inside it and have area.
        assert!((0.0..=1.0).contains(&hit.left) && (0.0..=1.0).contains(&hit.top));
        assert!(hit.width > 0.0 && hit.height > 0.0);
        assert!((hit.aspect - 2.0).abs() < 0.01, "the 200x100 MediaBox");

        // A word that is on neither page finds nothing rather than erroring.
        assert!(pdf::search(&path, "vancouver").is_some_and(|hits| hits.is_empty()));

        let _ = std::fs::remove_file(&path);
    }

    /// A scan that was never OCR'd has to be told apart from a document nobody's query matched.
    /// Both find nothing; only one of them is worth telling the user about.
    #[test]
    fn a_document_without_a_text_layer_says_so() {
        // Two pages, both bare MediaBoxes with no content stream at all — the shape of a scan.
        let pdf = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]>>endobj\n\
4 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 200]>>endobj\n\
trailer<</Root 1 0 R>>";
        let scan = std::env::temp_dir().join("qrate-pdf-notext.pdf");
        std::fs::write(&scan, pdf).unwrap();
        let typed = document_with_text("layer");

        match pdf::has_text_layer(&scan) {
            Some(layered) => {
                assert!(!layered, "no content stream means no text layer");
                assert_eq!(
                    pdf::has_text_layer(&typed),
                    Some(true),
                    "and a document with words in it has one"
                );
            }
            None => eprintln!("skipping the text-layer check: PDFium is not installed"),
        }

        let _ = std::fs::remove_file(&scan);
        let _ = std::fs::remove_file(&typed);
    }

    /// Collection search reads every page, and reads it again from the cache on the next launch.
    #[test]
    fn document_text_covers_every_page_and_survives_the_cache() {
        let path = document_with_text("whole");
        let Some(text) = crate::document_text(&path) else {
            eprintln!("skipping the document text check: PDFium is not installed");
            return;
        };
        assert!(text.contains("Aderman family") && text.contains("Kitsilano beach"));
        assert_eq!(
            crate::document_text(&path),
            Some(text),
            "the cached copy reads back"
        );
        assert_eq!(
            crate::document_text(std::path::Path::new("photo.jpg")),
            None
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Paging has to actually select a different page, not just re-render the first one. The two
    /// pages are deliberately opposite orientations, which is observable in the result without
    /// needing any page content to draw.
    #[test]
    fn each_page_of_a_document_renders_separately() {
        let pdf = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]>>endobj\n\
4 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 200]>>endobj\n\
trailer<</Root 1 0 R>>";
        let path = std::env::temp_dir().join("qrate-pdf-pages.pdf");
        std::fs::write(&path, pdf).unwrap();

        if super::pdfium().is_none() {
            assert_eq!(crate::page_count(&path), 1, "no library means no paging");
            eprintln!("skipping the paging check: PDFium is not installed");
            let _ = std::fs::remove_file(&path);
            return;
        }

        assert_eq!(crate::page_count(&path), 2);
        let first = pdf::decode(&path, 128, 0).expect("page one renders");
        let second = pdf::decode(&path, 128, 1).expect("page two renders");
        assert!(first.width() > first.height(), "page one is landscape");
        assert!(second.height() > second.width(), "page two is portrait");

        // Past the end must decline rather than clamping to the last page, which would make the
        // viewer's bounds check look like it worked when it had not.
        assert!(pdf::decode(&path, 128, 5).is_none());

        let _ = std::fs::remove_file(&path);
    }
}

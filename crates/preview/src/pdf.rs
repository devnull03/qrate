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

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::DynamicImage;
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

/// Illustrator files are, in practice, PDFs with extra private data, so PDFium opens them. It is
/// not guaranteed for very old `.ai` files — those fall through to the OS tier.
pub fn handles(extension: &str) -> bool {
    matches!(extension, "pdf" | "ai")
}

/// Where the library lives: beside the executable in an install, then wherever the system keeps
/// it for a development checkout. Probed once — a miss is permanent for the session, and retrying
/// a failed load on every card would be a stutter per row.
fn library() -> Option<&'static PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let beside = std::env::current_exe().ok().and_then(|exe| {
                let dir = exe.parent()?;
                Some(Pdfium::pdfium_platform_library_name_at_path(&dir))
                    .filter(|path| path.is_file())
            });
            if beside.is_some() {
                return beside;
            }
            // An empty path asks PDFium's own resolver to try the system library.
            Pdfium::bind_to_system_library()
                .ok()
                .map(|_| PathBuf::new())
                .or_else(|| {
                    log::info!(
                        "PDFium is not installed, so PDFs will show an icon instead of their first page"
                    );
                    None
                })
        })
        .as_ref()
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
fn pdfium() -> Option<&'static Mutex<Pdfium>> {
    static INSTANCE: OnceLock<Option<Mutex<Pdfium>>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            let library = library()?;
            let bindings = if library.as_os_str().is_empty() {
                Pdfium::bind_to_system_library()
            } else {
                Pdfium::bind_to_library(library)
            }
            .map_err(|err| log::warn!("could not load PDFium: {err}"))
            .ok()?;
            Some(Mutex::new(Pdfium::new(bindings)))
        })
        .as_ref()
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
    use crate::pdf;

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

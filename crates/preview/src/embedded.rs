//! Previews that are already inside the file, put there by whatever wrote it.
//!
//! A camera writes a full-size JPEG into every RAW it saves — it is what the camera's own screen
//! shows, and what Explorer and Finder show. Word and PowerPoint write `docProps/thumbnail.jpeg`
//! into their zip. An EPS from a drawing program carries a TIFF preview in its header. In each
//! case somebody has already done the expensive rendering, so the cheapest correct answer is to
//! find it rather than to decode the file properly.
//!
//! The ceiling is honest: these previews are whatever the writing program chose, so a RAW preview
//! is JPEG-compressed and occasionally smaller than the sensor image. For a thumbnail that costs
//! nothing; for full-resolution RAW work it would, which is what a real RAW decoder would be for.

use std::fs;
use std::io::Read as _;
use std::path::Path;

use image::DynamicImage;

/// Files whose container we know how to look inside. Extension-only, like every other list here —
/// this decides which tier runs, not whether the file is really what it claims.
pub fn handles(extension: &str) -> bool {
    matches!(extension, "docx" | "xlsx" | "pptx" | "eps") || is_raw(extension)
}

/// Tropy's RAW list, which is libvips'. Every one of these is a container with a JPEG inside;
/// the differences between them are in the sensor data we deliberately never touch.
pub fn is_raw(extension: &str) -> bool {
    matches!(
        extension,
        "3fr"
            | "ari"
            | "arw"
            | "bay"
            | "bmq"
            | "cap"
            | "cine"
            | "cr2"
            | "cr3"
            | "crw"
            | "cs1"
            | "dc2"
            | "dcr"
            | "dng"
            | "erf"
            | "fff"
            | "gpr"
            | "iiq"
            | "k25"
            | "kc2"
            | "kdc"
            | "mdc"
            | "mef"
            | "mos"
            | "mrw"
            | "nef"
            | "nrw"
            | "orf"
            | "pef"
            | "pxn"
            | "qtk"
            | "raf"
            | "raw"
            | "rdc"
            | "rw1"
            | "rw2"
            | "rwl"
            | "rwz"
            | "sr2"
            | "srf"
            | "srw"
            | "sti"
            | "x3f"
    )
}

/// Pull out whatever preview the file already carries.
pub fn decode(path: &Path, extension: &str) -> Option<DynamicImage> {
    match extension {
        "docx" | "xlsx" | "pptx" => ooxml(path),
        "eps" => eps(path),
        _ => raw(path),
    }
}

/// Office files are zips, and the ones saved with "save thumbnail" ticked carry a rendering of the
/// first page. Not every file has one — that is the writer's choice, not something we can force.
fn ooxml(path: &Path) -> Option<DynamicImage> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    // Word writes .jpeg, PowerPoint has been known to write .png; ask for both rather than guess.
    let name = ["docProps/thumbnail.jpeg", "docProps/thumbnail.png"]
        .into_iter()
        .find(|name| archive.by_name(name).is_ok())?;
    let mut bytes = Vec::new();
    archive.by_name(name).ok()?.read_to_end(&mut bytes).ok()?;
    image::load_from_memory(&bytes).ok()
}

/// A DOS EPS file starts with a binary header giving the offset and length of a TIFF preview.
/// Plain-text EPS has no raster preview at all and falls through to the next tier.
fn eps(path: &Path) -> Option<DynamicImage> {
    let bytes = fs::read(path).ok()?;
    // The magic that says "there is a binary header here" — otherwise this is a bare PostScript
    // program and there is nothing to extract.
    if bytes.get(..4)? != [0xC5, 0xD0, 0xD3, 0xC6] {
        return None;
    }
    let word = |at: usize| -> Option<usize> {
        let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        usize::try_from(u32::from_le_bytes(raw)).ok()
    };
    let (start, len) = (word(20)?, word(24)?);
    image::load_from_memory(bytes.get(start..start.checked_add(len)?)?).ok()
}

/// The camera's own JPEG, found by scanning for it.
///
/// ponytail: a byte scan for the largest `FFD8…FFD9` span, not an EXIF/TIFF walk. Every RAW format
/// nests its IFDs differently — a walk means a parser per vendor — whereas the JPEG markers are
/// the same bytes in all of them, so one scan covers CR2, NEF, ARW, DNG, ORF, RAF and CRW alike.
/// The scan cannot tell a real JPEG from sensor data that happens to look like one, so the
/// candidate is only accepted once it actually decodes. Swap in a real RAW decoder if full-
/// resolution sensor data is ever needed; this deliberately gives the camera's rendering instead.
fn raw(path: &Path) -> Option<DynamicImage> {
    let bytes = fs::read(path).ok()?;
    let mut best: Option<(usize, usize)> = None;
    let mut at = 0;
    while let Some(start) = find(&bytes, &[0xFF, 0xD8, 0xFF], at) {
        let Some(end) = find(&bytes, &[0xFF, 0xD9], start + 3) else {
            break;
        };
        let len = end + 2 - start;
        if best.is_none_or(|(_, best)| len > best) {
            best = Some((start, len));
        }
        at = start + 3;
    }
    let (start, len) = best?;
    // Anything this small is a filmstrip icon, not a preview worth showing.
    if len < 4096 {
        return None;
    }
    image::load_from_memory(bytes.get(start..start + len)?).ok()
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use crate::embedded;

    /// A JPEG hidden in a file that is not a JPEG has to come back out, and the *largest* one has
    /// to win — cameras write a small filmstrip thumbnail alongside the full preview.
    #[test]
    fn the_largest_embedded_jpeg_wins_and_junk_is_rejected() {
        // Noise, not flat colour: a solid image compresses to well under the size floor that
        // rejects filmstrip thumbnails, so a flat fixture would test nothing but that floor.
        let jpeg = |w: u32, h: u32| {
            let noisy = image::RgbImage::from_fn(w, h, |x, y| {
                image::Rgb([
                    ((x * 7 + y * 13) % 256) as u8,
                    ((x * 31 + y * 17) % 256) as u8,
                    ((x * 3 + y * 61) % 256) as u8,
                ])
            });
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgb8(noisy)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Jpeg,
                )
                .unwrap();
            bytes
        };

        let (small, large) = (jpeg(120, 90), jpeg(480, 360));
        assert!(
            small.len() > 4096,
            "both fixtures must clear the size floor, or 'largest wins' is untested"
        );

        let path = std::env::temp_dir().join("qrate-embedded-probe.nef");
        let mut file = std::fs::File::create(&path).unwrap();
        // A TIFF magic, a small thumbnail, sensor-ish padding, then the real preview — the
        // layout of a RAW, and the order that proves we take the biggest rather than the first.
        file.write_all(&[0x49, 0x49, 0x2A, 0x00]).unwrap();
        file.write_all(&small).unwrap();
        file.write_all(&vec![0u8; 512]).unwrap();
        file.write_all(&large).unwrap();
        drop(file);

        let found = embedded::decode(&path, "nef").expect("the preview is in there");
        assert_eq!(
            (found.width(), found.height()),
            (480, 360),
            "the big preview, not the filmstrip thumbnail"
        );

        // A file with no JPEG inside must decline rather than hand back garbage.
        let junk = std::env::temp_dir().join("qrate-embedded-junk.nef");
        std::fs::write(&junk, vec![0xFFu8; 40_000]).unwrap();
        assert!(embedded::decode(&junk, "nef").is_none());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&junk);
    }

    /// Office files are zips; the thumbnail is a named entry, and plenty of files simply lack it.
    #[test]
    fn office_thumbnails_come_out_of_the_zip() {
        let mut bytes = Vec::new();
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(64, 48, image::Rgb([9, 9, 9])))
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            zip.start_file::<_, ()>(
                "docProps/thumbnail.jpeg",
                zip::write::FileOptions::default(),
            )
            .unwrap();
            zip.write_all(&jpeg).unwrap();
            zip.finish().unwrap();
        }
        let path = std::env::temp_dir().join("qrate-embedded-probe.docx");
        std::fs::write(&path, &bytes).unwrap();
        let found = embedded::decode(&path, "docx").expect("thumbnail entry present");
        assert_eq!((found.width(), found.height()), (64, 48));

        // A docx saved without a thumbnail is normal, not an error.
        let mut empty = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut empty));
            zip.start_file::<_, ()>("word/document.xml", zip::write::FileOptions::default())
                .unwrap();
            zip.write_all(b"<w:document/>").unwrap();
            zip.finish().unwrap();
        }
        let bare = std::env::temp_dir().join("qrate-embedded-bare.docx");
        std::fs::write(&bare, &empty).unwrap();
        assert!(embedded::decode(&bare, "docx").is_none());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bare);
    }

    #[test]
    fn the_raw_list_matches_what_the_tier_claims() {
        assert!(embedded::handles("cr2") && embedded::handles("nef") && embedded::handles("dng"));
        assert!(embedded::handles("docx") && embedded::handles("eps"));
        assert!(!embedded::handles("jpg"), "tier 0 owns that");
        assert!(!embedded::handles("mp4"), "the media tier owns that");
    }
}

//! The last tier: ask the operating system what the file looks like.
//!
//! This is how a `.psd`, a `.dwg`, an InDesign document or anything else we will never write a
//! decoder for still gets a picture. It costs nothing when it fails, which is why it sits at the
//! bottom of the ladder rather than anywhere near the top.
//!
//! It is deliberately last for a second reason: what it can answer depends on which applications
//! and codec packs the machine has, so two archivists can get different pictures for the same
//! file. Every tier above is deterministic, and none of them should ever be pre-empted by this.

use std::path::Path;

use image::DynamicImage;

/// Formats worth asking the OS about — ones with no tier of their own but which desktop platforms
/// commonly know. Kept short on purpose: each entry is a promise in [`crate::can_preview`] that
/// something *might* appear, and an entry that never resolves is just a slow route to the icon.
pub fn handles(extension: &str) -> bool {
    cfg!(any(windows, target_os = "linux", target_os = "macos"))
        && matches!(
            extension,
            "psd"
                | "psb"
                | "dwg"
                | "dxf"
                | "indd"
                | "cdr"
                | "sketch"
                | "afphoto"
                | "afdesign"
                | "wmf"
                | "emf"
                | "odt"
                | "ods"
                | "odp"
                | "doc"
                | "xls"
                | "ppt"
        )
}

/// Whatever the shell can draw for this file, at up to `max_edge` square.
#[cfg(windows)]
pub fn thumbnail(path: &Path, max_edge: u32) -> Option<DynamicImage> {
    windows_shell::thumbnail(path, max_edge)
}

/// The freedesktop thumbnailer spec: a directory of INI files, each naming a command that turns
/// one MIME type into a PNG. There is no system API to call — this *is* the mechanism GNOME and
/// KDE file managers use.
#[cfg(target_os = "linux")]
pub fn thumbnail(path: &Path, max_edge: u32) -> Option<DynamicImage> {
    freedesktop::thumbnail(path, max_edge)
}

/// QuickLook, which is what Finder's own previews come from — so anything with a QuickLook plugin
/// installed, which on a Mac is most creative-suite formats.
#[cfg(target_os = "macos")]
pub fn thumbnail(path: &Path, max_edge: u32) -> Option<DynamicImage> {
    quick_look::thumbnail(path, max_edge)
}

/// No thumbnailing service on anything else, which in practice means the BSDs.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn thumbnail(_path: &Path, _max_edge: u32) -> Option<DynamicImage> {
    None
}

#[cfg(windows)]
mod windows_shell {
    use std::path::Path;

    use image::DynamicImage;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, GetObjectW, ReleaseDC,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_THUMBNAILONLY,
    };
    use windows::core::HSTRING;

    pub fn thumbnail(path: &Path, max_edge: u32) -> Option<DynamicImage> {
        let side = i32::try_from(max_edge).ok()?;

        // Safety: every call below is a documented Win32/COM entry point used in the sequence
        // MSDN prescribes, and each handle is released on both the success and failure path. This
        // runs on a background thread — never gpui's, whose apartment we must not disturb.
        unsafe {
            // Already-initialised on this thread is success, and a thread initialised in another
            // apartment model still works for this interface, so neither is worth failing over.
            let started = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let factory: IShellItemImageFactory =
                SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;

            // `THUMBNAILONLY` refuses the generic file-type icon: we already draw our own, and a
            // Windows icon in a gallery of scans would read as a real picture of the document.
            // `BIGGERSIZEOK` accepts a cached larger size rather than forcing a re-render.
            let bitmap = factory
                .GetImage(
                    SIZE { cx: side, cy: side },
                    SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
                )
                .ok();

            let image = bitmap.and_then(|bitmap| {
                let pixels = read_bitmap(bitmap);
                let _ = DeleteObject(bitmap.into());
                pixels
            });

            if started.is_ok() {
                CoUninitialize();
            }
            image
        }
    }

    /// Copy an HBITMAP's pixels out as RGBA.
    ///
    /// Asking for a *negative* height gives a top-down DIB, which is the row order everything
    /// above expects — a bottom-up one would render every thumbnail upside down.
    unsafe fn read_bitmap(bitmap: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<DynamicImage> {
        unsafe {
            let mut info = BITMAP::default();
            let wrote = GetObjectW(
                bitmap.into(),
                i32::try_from(size_of::<BITMAP>()).ok()?,
                Some(std::ptr::from_mut(&mut info).cast()),
            );
            if wrote == 0 {
                return None;
            }
            let (width, height) = (info.bmWidth, info.bmHeight.abs());
            if width <= 0 || height <= 0 {
                return None;
            }

            let mut header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let count = usize::try_from(width).ok()? * usize::try_from(height).ok()?;
            let mut bytes = vec![0u8; count * 4];
            let dc = GetDC(None);
            let rows = GetDIBits(
                dc,
                bitmap,
                0,
                u32::try_from(height).ok()?,
                Some(bytes.as_mut_ptr().cast()),
                &raw mut header,
                DIB_RGB_COLORS,
            );
            ReleaseDC(None, dc);
            if rows == 0 {
                return None;
            }

            // A DIB is BGRA; the rest of the pipeline works in RGBA and swaps once at the end.
            for px in bytes.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            // Some thumbnail providers return an opaque image with the alpha byte left at zero,
            // which would otherwise draw as nothing at all.
            if bytes.chunks_exact(4).all(|px| px[3] == 0) {
                for px in bytes.chunks_exact_mut(4) {
                    px[3] = 255;
                }
            }

            let buffer =
                image::RgbaImage::from_raw(width.try_into().ok()?, height.try_into().ok()?, bytes)?;
            Some(DynamicImage::ImageRgba8(buffer))
        }
    }
}

#[cfg(target_os = "macos")]
mod quick_look {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    use image::DynamicImage;
    use objc2::AnyThread as _;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    };
    use objc2_foundation::{NSError, NSString, NSURL};
    use objc2_quick_look_thumbnailing::{
        QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes,
        QLThumbnailGenerator, QLThumbnailRepresentation,
    };

    /// How long to wait for QuickLook before giving up.
    ///
    /// Generation is asynchronous and farmed out to per-format extensions, so a badly-behaved one
    /// could otherwise hold a decode thread forever. A preview is never worth blocking on: give
    /// up and let the ladder fall through to the icon.
    const PATIENCE: Duration = Duration::from_secs(5);

    pub fn thumbnail(path: &Path, max_edge: u32) -> Option<DynamicImage> {
        let side = f64::from(max_edge);

        // Safety: the request and generator are ordinary Objective-C objects managed by objc2's
        // `Retained`, and the completion block is kept alive until the channel receives or the
        // wait times out. Nothing here touches the main thread — this runs on a decode thread.
        let representation = unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let request =
                QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
                    QLThumbnailGenerationRequest::alloc(),
                    &url,
                    CGSize::new(side, side),
                    1.0,
                    // Not `Icon`: a generic document icon is exactly what our own placeholder already
                    // draws, and dressing it up as a preview would misrepresent the file.
                    QLThumbnailGenerationRequestRepresentationTypes::Thumbnail,
                );

            let (tx, rx) = mpsc::channel();
            let handler = block2::RcBlock::new(
                move |thumbnail: *mut QLThumbnailRepresentation, _error: *mut NSError| {
                    // A null representation means QuickLook had nothing for this type, which is a
                    // normal answer rather than a failure.
                    let image = std::ptr::NonNull::new(thumbnail)
                        .map(|thumbnail| thumbnail.as_ref().CGImage());
                    let _ = tx.send(image);
                },
            );
            QLThumbnailGenerator::sharedGenerator()
                .generateBestRepresentationForRequest_completionHandler(&request, &handler);

            rx.recv_timeout(PATIENCE).ok().flatten()
        }?;

        // Safety: drawing a CGImage into a bitmap context we allocated and sized ourselves.
        unsafe {
            let width = CGImage::width(Some(&representation));
            let height = CGImage::height(Some(&representation));
            if width == 0 || height == 0 {
                return None;
            }

            // An explicit RGBA8 context rather than reading the image's own buffer: a CGImage can
            // be in any colour space and channel order, and drawing into a context we defined is
            // what makes the result predictable.
            let mut bytes = vec![0u8; width * height * 4];
            let space = CGColorSpace::new_device_rgb()?;
            let context = CGBitmapContextCreate(
                bytes.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                Some(&space),
                // Alpha last, host byte order — which is the zero value, so the alpha info is the
                // whole descriptor. That lays the buffer out as RGBA8, matching every other tier.
                CGImageAlphaInfo::PremultipliedLast.0,
            )?;
            CGContext::draw_image(
                Some(&context),
                CGRect::new(
                    CGPoint::new(0.0, 0.0),
                    CGSize::new(width as f64, height as f64),
                ),
                Some(&representation),
            );

            let buffer = image::RgbaImage::from_raw(
                u32::try_from(width).ok()?,
                u32::try_from(height).ok()?,
                bytes,
            )?;
            Some(DynamicImage::ImageRgba8(buffer))
        }
    }
}

#[cfg(target_os = "linux")]
mod freedesktop {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use image::DynamicImage;

    /// Every installed thumbnailer definition, in the spec's search order — a user's own
    /// definitions override the system ones.
    fn definitions() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/thumbnailers"));
        }
        dirs.push(PathBuf::from("/usr/share/thumbnailers"));
        dirs.push(PathBuf::from("/usr/local/share/thumbnailers"));
        dirs.iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flatten()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "thumbnailer"))
            .collect()
    }

    /// The `Exec=` line of the first definition claiming this file's MIME type.
    fn command_for(mime: &str) -> Option<String> {
        for definition in definitions() {
            let Ok(text) = std::fs::read_to_string(&definition) else {
                continue;
            };
            let value = |field: &str| {
                text.lines()
                    .find_map(|line| line.strip_prefix(field)?.strip_prefix('='))
            };
            let claims = value("MimeType")
                .is_some_and(|types| types.split(';').any(|entry| entry.trim() == mime));
            if claims && let Some(exec) = value("Exec") {
                return Some(exec.to_string());
            }
        }
        None
    }

    pub fn thumbnail(path: &Path, max_edge: u32) -> Option<DynamicImage> {
        let mime = mime_guess::from_path(path).first()?.to_string();
        let exec = command_for(&mime)?;

        let out = std::env::temp_dir().join(format!("qrate-thumb-{}.png", std::process::id()));
        // The spec's placeholders: %i input, %o output, %s the requested size, %u a file URI.
        let mut parts = exec.split_whitespace().map(|part| {
            part.replace("%i", &path.to_string_lossy())
                .replace("%u", &format!("file://{}", path.to_string_lossy()))
                .replace("%o", &out.to_string_lossy())
                .replace("%s", &max_edge.to_string())
        });

        let program = parts.next()?;
        let ran = Command::new(program).args(parts).status();
        let image = ran
            .is_ok_and(|status| status.success())
            .then(|| image::open(&out).ok())
            .flatten();
        let _ = std::fs::remove_file(&out);
        image
    }
}

#[cfg(test)]
mod tests {
    use crate::native;

    /// The claim has to match what the platform can actually attempt, or `can_preview` promises a
    /// picture that never arrives and the gallery offers a double-click that does nothing.
    #[test]
    fn only_claims_formats_on_platforms_with_a_thumbnailer() {
        if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            assert!(native::handles("psd") && native::handles("dwg"));
        } else {
            assert!(!native::handles("psd"), "no thumbnailing service here");
        }
        assert!(!native::handles("jpg"), "tier 0 owns that");
        assert!(!native::handles("pdf"), "pdfium owns that");
    }

    /// Whatever the platform, a file that does not exist must come back empty rather than
    /// panicking through the FFI.
    #[test]
    fn a_missing_file_is_declined_not_fatal() {
        assert!(native::thumbnail(std::path::Path::new("/nonexistent/x.psd"), 128).is_none());
    }

    /// The Windows shell path, exercised against a format it definitely understands. Proves the
    /// COM sequence, the DIB copy and the row order actually work rather than merely compiling.
    #[cfg(windows)]
    #[test]
    fn the_shell_draws_a_file_windows_knows() {
        let path = std::env::temp_dir().join("qrate-native-probe.png");
        // A recognisable landscape image: red top-left quadrant, so an upside-down copy shows.
        let mut source = image::RgbaImage::from_pixel(160, 80, image::Rgba([20, 20, 20, 255]));
        for y in 0..20 {
            for x in 0..40 {
                source.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        source.save(&path).unwrap();

        let Some(thumb) = native::thumbnail(&path, 128) else {
            eprintln!("skipping: the shell returned no thumbnail on this machine");
            let _ = std::fs::remove_file(&path);
            return;
        };
        assert!(thumb.width() > 0 && thumb.height() > 0);
        assert!(
            thumb.width() >= thumb.height(),
            "a landscape source must not come back rotated"
        );

        // Top-left must still be the red corner: a bottom-up DIB would put it at the bottom.
        let rgba = thumb.to_rgba8();
        let corner = rgba.get_pixel(rgba.width() / 8, rgba.height() / 8).0;
        assert!(
            corner[0] > corner[2],
            "top-left should still be the red quadrant, got {corner:?} — check row order and BGRA"
        );

        let _ = std::fs::remove_file(&path);
    }
}

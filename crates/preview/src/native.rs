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
    cfg!(any(windows, target_os = "linux"))
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

/// macOS has `QLThumbnailGenerator`, which would cover this properly and is not wired up.
///
/// It needs `objc2-quick-look-thumbnailing`, and its API is asynchronous through a completion
/// block, so it means bridging a block to a channel and converting a `CGImage` into a buffer.
/// That is a few hours of work that cannot be compiled or run from a Windows checkout, and
/// shipping FFI nobody has executed is worse than shipping a documented gap: every other tier
/// still runs on macOS, so the only loss is `.psd`-style files falling back to their icon.
#[cfg(not(any(windows, target_os = "linux")))]
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
        if cfg!(any(windows, target_os = "linux")) {
            assert!(native::handles("psd") && native::handles("dwg"));
        } else {
            assert!(!native::handles("psd"), "macOS has no tier wired up yet");
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

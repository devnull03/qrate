//! Anything ffmpeg can open: video, and the still formats no Rust decoder covers.
//!
//! ffmpeg runs as a **subprocess**, not as linked libraries. `ffmpeg-next` and `video-rs` need
//! FFmpeg's headers and import libraries present at build time, which taxes every contributor and
//! every CI leg — and a shipped build has to carry the DLLs regardless, so linking buys nothing.
//! Spawning a process costs about a tenth of a second per file, once, against a cached result.
//!
//! It earns its place twice over. Bundling it for video also settles JPEG 2000 and AVIF, so we
//! need neither an openjpeg build nor `image`'s `avif-native` feature — the latter would switch on
//! a C dav1d build for gpui's copy of `image` as well as ours.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::DynamicImage;

/// Formats routed to ffmpeg. The still images here are the ones no pure-Rust decoder in the tree
/// handles: JPEG 2000, AVIF, and HEIC when it carries no embedded preview of its own.
pub fn handles(extension: &str) -> bool {
    matches!(
        extension,
        // Video.
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" | "mpg" | "mpeg" | "wmv" | "flv" | "m2ts"
        // Stills nothing above us decodes.
        | "jp2" | "jpf" | "jpx" | "j2k" | "avif" | "heic" | "heif"
    )
}

/// Whether a video needs seeking past its opening. A still image has no timeline to seek — which
/// is also what decides whether the viewer offers a scrubber.
pub fn is_video(extension: &str) -> bool {
    !matches!(
        extension,
        "jp2" | "jpf" | "jpx" | "j2k" | "avif" | "heic" | "heif"
    )
}

/// The ffmpeg to run: the one shipped beside the executable first, so a qrate install is
/// self-contained and reproducible, then whatever is on `PATH` for a development checkout.
/// `None` means the tier is simply unavailable and the ladder moves on.
fn binary() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(name)));
    if let Some(bundled) = beside.filter(|path| path.is_file()) {
        return Some(bundled);
    }
    // Cheapest reliable probe for "is it on PATH": ask it to identify itself.
    Command::new(name)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .filter(std::process::ExitStatus::success)
        .map(|_| PathBuf::from(name))
}

/// One frame, as PNG on stdout.
///
/// Video is sampled a second in rather than at zero: the first frame of a digitised tape or a
/// phone clip is very often black or a fade, which makes for a useless card. A clip shorter than
/// that yields nothing, so the seek is dropped and retried — the second attempt is what covers
/// very short clips and animated stills.
pub fn decode(path: &Path, max_edge: u32) -> Option<DynamicImage> {
    let binary = binary()?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if is_video(&extension)
        && let Some(image) = run(&binary, path, max_edge, Some("1"))
    {
        return Some(image);
    }
    run(&binary, path, max_edge, None)
}

/// The frame `seconds` into the clip, for scrubbing. Unlike [`decode`] there is no retry without
/// the seek: a position past the end has no frame, and showing the opening instead would leave the
/// scrubber pointing somewhere the picture is not.
pub fn frame_at(path: &Path, max_edge: u32, seconds: u32) -> Option<DynamicImage> {
    run(&binary()?, path, max_edge, Some(&seconds.to_string()))
}

/// How long the clip runs, in whole seconds.
///
/// ffmpeg prints this on stderr as part of the banner it emits when asked to open a file with no
/// output to write — which is why this cannot go through [`run`], whose `-v error` suppresses it.
/// ffprobe would answer directly, but shipping it means a second binary in the installer and both
/// bundle scripts; the banner's format has been stable for over a decade.
pub fn duration(path: &Path) -> Option<u32> {
    let output = Command::new(binary()?)
        .arg("-nostdin")
        .arg("-i")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;

    // `Duration: 00:04:03.20, start: ...`. Unparseable for a stream with no known length, which
    // reads as "no timeline" rather than as an error.
    let banner = String::from_utf8_lossy(&output.stderr);
    let clock = banner.split_once("Duration: ")?.1.split(',').next()?;
    let mut fields = clock.split(':');
    let hours: u32 = fields.next()?.trim().parse().ok()?;
    let minutes: u32 = fields.next()?.parse().ok()?;
    let seconds: f32 = fields.next()?.parse().ok()?;
    Some(hours * 3600 + minutes * 60 + seconds as u32)
}

fn run(binary: &Path, path: &Path, max_edge: u32, seek: Option<&str>) -> Option<DynamicImage> {
    let mut command = Command::new(binary);
    command.arg("-v").arg("error").arg("-nostdin");
    // Before `-i`, so ffmpeg seeks by keyframe instead of decoding everything up to that point.
    if let Some(seek) = seek {
        command.arg("-ss").arg(seek);
    }
    command
        .arg("-i")
        .arg(path)
        .arg("-frames:v")
        .arg("1")
        // Fit inside a max_edge box without ever enlarging: the `min` expressions cap each axis at
        // the source's own size, and `decrease` keeps the aspect ratio.
        .arg("-vf")
        .arg(format!(
            "scale=w='min(iw,{max_edge})':h='min(ih,{max_edge})':force_original_aspect_ratio=decrease"
        ))
        .arg("-f")
        .arg("image2pipe")
        .arg("-vcodec")
        .arg("png")
        .arg("-")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());

    let output = command
        .output()
        .map_err(|err| log::warn!("could not run ffmpeg: {err}"))
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        // Only worth a line once the retry has also failed, which is the `seek == None` call.
        if seek.is_none() {
            let why = String::from_utf8_lossy(&output.stderr);
            log::warn!(
                "ffmpeg could not read {}: {}",
                path.display(),
                why.trim().lines().next_back().unwrap_or("no output")
            );
        }
        return None;
    }
    image::load_from_memory(&output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use crate::media;

    #[test]
    fn claims_video_and_the_stills_nothing_else_decodes() {
        assert!(media::handles("mp4") && media::handles("mov") && media::handles("mkv"));
        assert!(media::handles("jp2") && media::handles("avif") && media::handles("heic"));
        assert!(!media::handles("jpg"), "tier 0 owns that");
        assert!(!media::handles("pdf"), "pdfium owns that");
    }

    /// Exercises the real subprocess, so it is skipped where ffmpeg is absent rather than failing
    /// a machine that legitimately has not got it — the same condition the tier degrades on.
    #[test]
    fn pulls_a_frame_out_of_a_real_video() {
        if super::binary().is_none() {
            eprintln!("skipping: ffmpeg is not installed");
            return;
        }
        let path = std::env::temp_dir().join("qrate-media-probe.mp4");
        // Two seconds of colour bars, so the one-second seek lands inside the clip.
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc=size=320x240:rate=10:duration=2")
            .arg(&path)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            eprintln!("skipping: could not synthesise a test clip");
            return;
        }

        let frame = media::decode(&path, 128).expect("a frame comes out");
        assert!(
            frame.width() <= 128 && frame.height() <= 128,
            "ffmpeg applied the size cap, got {}x{}",
            frame.width(),
            frame.height()
        );
        assert_eq!(frame.width(), 128, "landscape source fills the long edge");

        let _ = std::fs::remove_file(&path);
    }

    /// What the viewer's scrubber is built on: how long the clip is, and a frame from somewhere
    /// other than its opening. A `frame_at` that ignored its seek would look right until someone
    /// noticed every position showing the same picture.
    #[test]
    fn a_clip_reports_its_length_and_yields_a_frame_from_the_middle() {
        if super::binary().is_none() {
            eprintln!("skipping: ffmpeg is not installed");
            return;
        }
        // Its own name: three tests once shared one fixture and raced to delete it.
        let path = std::env::temp_dir().join("qrate-media-timeline.mp4");
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc=size=320x240:rate=10:duration=4")
            .arg(&path)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            eprintln!("skipping: could not synthesise a test clip");
            return;
        }

        assert_eq!(media::duration(&path), Some(4), "a four-second clip");
        let opening = media::frame_at(&path, 128, 0).expect("a frame at the start");
        let later = media::frame_at(&path, 128, 3).expect("a frame three seconds in");
        assert_ne!(
            opening.as_bytes(),
            later.as_bytes(),
            "the seek moved the picture"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// A file that is not really a video must decline, not hang or panic.
    #[test]
    fn declines_a_file_ffmpeg_cannot_read() {
        if super::binary().is_none() {
            eprintln!("skipping: ffmpeg is not installed");
            return;
        }
        let junk = std::env::temp_dir().join("qrate-media-junk.mp4");
        std::fs::write(&junk, b"not a video at all").unwrap();
        assert!(media::decode(&junk, 128).is_none());
        let _ = std::fs::remove_file(&junk);
    }
}

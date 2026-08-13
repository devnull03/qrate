//! What an audio file looks like: the artwork tagged inside it.
//!
//! For an oral-history recording or a digitised tape the embedded image is usually a scan of the
//! cassette label or the sleeve — the thing an archivist actually wants to recognise the item by.
//! A waveform would be prettier and tells you nothing about *which* recording this is, so artwork
//! comes first and a file without any falls through to the icon.
//!
//! Symphonia is pure Rust, so unlike every other tier below tier 0 this one needs nothing
//! installed and cannot be unavailable.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use image::DynamicImage;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, Visual};
use symphonia::core::probe::{Hint, ProbeResult};

pub fn handles(extension: &str) -> bool {
    matches!(
        extension,
        "mp3" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "wav" | "aiff" | "aif" | "alac"
    )
}

/// Open the file and read its container header, no further. The extension is only a hint, so a
/// mislabelled recording still probes as whatever it really is.
fn probe(path: &Path) -> Option<ProbeResult> {
    let file = File::open(path).ok()?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()
}

/// How long the recording runs. Read from the container header rather than from the player, so the
/// transport can show a total before anything is playing — and without a sound card at all.
pub fn duration(path: &Path) -> Option<Duration> {
    let probed = probe(path)?;
    let params = &probed.format.default_track()?.codec_params;
    let frames = params.n_frames?;

    let seconds = match params.sample_rate {
        Some(rate) if rate > 0 => frames as f64 / f64::from(rate),
        // `calc_time` asserts on a zero timebase rather than returning an error, so it is only
        // reachable once the timebase has been checked here.
        _ => {
            let base = params
                .time_base
                .filter(|base| base.numer > 0 && base.denom > 0)?;
            let time = base.calc_time(frames);
            time.seconds as f64 + time.frac
        }
    };
    Duration::try_from_secs_f64(seconds).ok()
}

/// Every picture tagged in the file.
///
/// Two places, and which one a writer used depends on the format: tags that sit outside the
/// container (an MP3's ID3 block) are read during the probe, tags inside it belong to the reader.
/// Take both rather than guessing per extension.
fn visuals(path: &Path) -> Vec<Visual> {
    let Some(mut probed) = probe(path) else {
        return Vec::new();
    };
    let mut visuals = Vec::new();
    if let Some(mut log) = probed.metadata.get()
        && let Some(revision) = log.skip_to_latest()
    {
        visuals.extend(revision.visuals().iter().cloned());
    }
    let mut metadata = probed.format.metadata();
    if let Some(revision) = metadata.skip_to_latest() {
        visuals.extend(revision.visuals().iter().cloned());
    }
    visuals
}

/// Whether there is any artwork at all, without decoding it. The viewer asks before it decides
/// where to put the playback controls: a recording with no cover has an empty page to fill.
pub fn has_cover(path: &Path) -> bool {
    !visuals(path).is_empty()
}

/// The largest picture tagged in the file. Largest rather than first because a file often carries
/// both a small icon and a real cover, and the tag order does not say which is which.
pub fn cover(path: &Path) -> Option<DynamicImage> {
    let biggest = visuals(path)
        .into_iter()
        .max_by_key(|visual| visual.data.len())?;
    image::load_from_memory(&biggest.data).ok()
}

#[cfg(test)]
mod tests {
    use crate::audio;

    #[test]
    fn claims_the_usual_recording_formats() {
        assert!(audio::handles("mp3") && audio::handles("flac") && audio::handles("wav"));
        assert!(
            !audio::handles("mp4"),
            "the media tier owns video containers"
        );
        assert!(!audio::handles("jpg"));
    }

    /// A recording with no artwork, and a file that is not audio at all, both have to decline
    /// quietly — this runs on whatever the files folder happens to contain.
    #[test]
    fn declines_without_artwork_and_on_junk() {
        let junk = std::env::temp_dir().join("qrate-audio-junk.mp3");
        std::fs::write(&junk, b"not audio").unwrap();
        assert!(audio::cover(&junk).is_none());
        assert!(audio::cover(std::path::Path::new("/nonexistent/x.mp3")).is_none());
        let _ = std::fs::remove_file(&junk);
    }

    /// A real, artwork-free WAV: symphonia must parse it happily and still report no picture,
    /// rather than erroring in a way that would look the same as an unreadable file.
    #[test]
    fn a_valid_recording_without_artwork_is_not_an_error() {
        // 44-byte canonical WAV header describing one sample of silence.
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(36u32.to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(1u16.to_le_bytes()); // mono
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(2u32.to_le_bytes());
        wav.extend(0u16.to_le_bytes());

        let path = std::env::temp_dir().join("qrate-audio-silent.wav");
        std::fs::write(&path, &wav).unwrap();
        assert!(audio::cover(&path).is_none(), "no artwork, but no panic");
        let _ = std::fs::remove_file(&path);
    }
}

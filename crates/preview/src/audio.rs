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

use image::DynamicImage;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, Visual};

pub fn handles(extension: &str) -> bool {
    matches!(
        extension,
        "mp3" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "wav" | "aiff" | "aif" | "alac"
    )
}

/// The largest picture tagged in the file. Largest rather than first because a file often carries
/// both a small icon and a real cover, and the tag order does not say which is which.
pub fn cover(path: &Path) -> Option<DynamicImage> {
    let file = File::open(path).ok()?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            Default::default(),
            MetadataOptions::default(),
        )
        .ok()?;

    // Artwork can be attached to the media as a whole or to an individual track, and which one a
    // writer used depends on the format — take both rather than guessing per extension.
    let mut visuals: Vec<Visual> = Vec::new();
    let mut collect = |revision: &symphonia::core::meta::MetadataRevision| {
        visuals.extend(revision.media.visuals.iter().cloned());
        visuals.extend(
            revision
                .per_track
                .iter()
                .flat_map(|track| track.metadata.visuals.iter().cloned()),
        );
    };
    let mut metadata = format.metadata();
    if let Some(revision) = metadata.skip_to_latest() {
        collect(revision);
    } else if let Some(revision) = metadata.current() {
        collect(revision);
    }

    let biggest = visuals.into_iter().max_by_key(|visual| visual.data.len())?;
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

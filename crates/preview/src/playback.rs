//! Playing a linked recording, for the one file type the viewer could show but never let anyone
//! hear. An oral history is the material this tool exists for, and transcribing it meant leaving
//! the app for a media player and alt-tabbing back to type.
//!
//! One recording at a time, deliberately: this is the viewer's transport, and the viewer shows one
//! file. Starting a second replaces the first rather than mixing them.
//!
//! `rodio`'s device handle is not `Send`, so this lives where gpui globals live — the main thread.
//! Every entry point tolerates having no audio device at all: a machine with no sound card, or a
//! CI runner, gets a quiet no-op rather than an error the archivist cannot act on.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{App, Global};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

pub use crate::audio::duration;

struct Playback {
    /// The output device. Held because dropping it stops the sound, nothing reads it after setup.
    _device: MixerDeviceSink,
    player: Player,
    /// What was last handed to the player. There is one device and one recording, but more than
    /// one transport can be on screen — each has to know whether the position is even its own.
    playing: PathBuf,
}

impl Global for Playback {}

/// The player, if one has been opened. Only [`play`] opens the device — asking where a recording
/// is, or stopping one, has no business turning a sound card on.
fn player(cx: &App) -> Option<&Player> {
    Some(&cx.try_global::<Playback>()?.player)
}

/// Start `path` from the beginning, replacing whatever was playing. Opens the output device on
/// first use, and stays quiet on a machine that has none.
pub fn play(path: &Path, cx: &mut App) {
    let opened = File::open(path)
        .map_err(|err| err.to_string())
        .and_then(|file| Decoder::new(BufReader::new(file)).map_err(|err| err.to_string()));
    let Ok(source) = opened.inspect_err(|err| log::warn!("cannot play {}: {err}", path.display()))
    else {
        return;
    };

    if !cx.has_global::<Playback>() {
        let Ok(device) = DeviceSinkBuilder::open_default_sink()
            .inspect_err(|err| log::warn!("no audio output device, playback unavailable: {err}"))
        else {
            return;
        };
        let player = Player::connect_new(device.mixer());
        cx.set_global(Playback {
            _device: device,
            player,
            playing: PathBuf::new(),
        });
    }

    let playback = cx.global_mut::<Playback>();
    playback.playing = path.to_path_buf();
    playback.player.clear();
    playback.player.append(source);
    playback.player.play();
}

/// The recording currently loaded, so a transport for some other file knows the position below is
/// not describing it.
pub fn playing(cx: &App) -> Option<&Path> {
    Some(cx.try_global::<Playback>()?.playing.as_path())
}

/// Pause if playing, resume if paused. Does nothing before anything is loaded.
pub fn toggle(cx: &App) {
    let Some(player) = player(cx) else {
        return;
    };
    if player.is_paused() {
        player.play();
    } else {
        player.pause();
    }
}

pub fn seek(to: Duration, cx: &App) {
    if let Some(player) = player(cx)
        && let Err(err) = player.try_seek(to)
    {
        log::warn!("could not seek the recording to {to:?}: {err}");
    }
}

/// How far into the recording the player is, and whether it is running. `None` before anything
/// has been played.
pub fn position(cx: &App) -> Option<(Duration, bool)> {
    let player = player(cx)?;
    Some((player.get_pos(), !player.is_paused() && !player.empty()))
}

/// Silence. The viewer calls this as it closes — without it the recording plays on over an empty
/// screen, with nothing left on the page to stop it.
pub fn stop(cx: &App) {
    if let Some(player) = player(cx) {
        player.clear();
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — a chained `gpui::*` glob would shadow `#[test]`.
    use std::path::Path;

    /// The header figure, not the player's — so this holds on a machine with no sound card, which
    /// is every CI runner. Nothing here opens an output device.
    #[test]
    fn a_recordings_length_is_read_without_playing_it() {
        // 44-byte canonical WAV header, then one second of 8 kHz 16-bit mono silence.
        let samples = 8000usize;
        let data = samples * 2;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + data as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(1u16.to_le_bytes()); // mono
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data as u32).to_le_bytes());
        wav.extend(std::iter::repeat_n(0u8, data));

        let path = std::env::temp_dir().join("qrate-playback-duration.wav");
        std::fs::write(&path, &wav).unwrap();
        let length = crate::playback::duration(&path).expect("a valid WAV has a length");
        assert!(
            (length.as_secs_f64() - 1.0).abs() < 0.01,
            "one second of samples, got {length:?}"
        );
        let _ = std::fs::remove_file(&path);

        assert!(crate::playback::duration(Path::new("/nonexistent/x.wav")).is_none());
    }
}

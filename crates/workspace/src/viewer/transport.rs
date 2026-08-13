//! Playing a recording: what is loaded, how far in it is, and the bar that drives it.
//!
//! It lives beside its state for the same reason [`super::find`] does — the rules are here, not in
//! whichever builder draws it. Two views host one: the viewer, where a recording fills the screen,
//! and the Details panel's preview box, where it sits under the row being catalogued. So the
//! button handlers and the position poll are generic over their [`Host`] rather than reaching for
//! `Viewer` directly.
//!
//! There is one output device and one recording, but there can be two transports on screen. Each
//! only reflects the player while [`preview::playback::playing`] names its own file.

use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, StyledExt as _,
    button::Button,
    h_flex,
    slider::{Slider, SliderEvent, SliderState, SliderValue},
};

/// How often the transport asks the player where it is. Fine enough that the clock never looks
/// stuck, coarse enough that a two-hour recording costs nothing to watch.
const TICK: Duration = Duration::from_millis(250);

/// A view that owns a transport, so the bar it renders can reach back into it.
pub trait Host: 'static + Sized {
    fn transport(&mut self) -> Option<&mut Transport>;
}

pub struct Transport {
    /// The recording this transport drives. Held so its handlers, and the guard against another
    /// transport's playback, both have something to compare against.
    pub path: PathBuf,
    /// How long the recording runs, read from its header before anything plays. `None` where the
    /// file would not probe, which leaves a bar that can play but not scrub.
    pub length: Option<Duration>,
    /// Whether the file carries artwork. The viewer puts the bar in the middle of an empty page
    /// when it does not, rather than along the bottom of a blank rectangle.
    pub art: bool,
    /// How far in the player is, refreshed by the poll.
    pub at: Duration,
    /// Whether sound is coming out right now, as opposed to paused or finished.
    pub playing: bool,
    /// True between grabbing the thumb and letting go, so the poll does not drag it back to
    /// wherever playback happens to be.
    scrubbing: bool,
    slider: Entity<SliderState>,
    /// The position poll. Held rather than detached: it must die with its host, or it outlives the
    /// recording it was following.
    tick: Option<Task<()>>,
}

impl Transport {
    /// Build a transport for `path`, or `None` if that file is not a recording.
    ///
    /// Both the length and the artwork question open the file, so both are asked once here — they
    /// cannot change while the transport is up.
    pub fn new<V: Host>(path: PathBuf, cx: &mut Context<V>) -> Option<Self> {
        if !preview::has_audio(&path) {
            return None;
        }
        let length = preview::playback::duration(&path);
        let seconds = length.map_or(0., |length| length.as_secs_f64() as f32);
        // A step of a second: the resolution anyone scrubbing a two-hour interview works at, and
        // what the readout beside it shows.
        let slider = cx.new(|_| SliderState::new().min(0.).max(seconds.max(1.)).step(1.));

        // Detached because the slider is owned by the host and cannot outlive it.
        cx.subscribe(&slider, |this: &mut V, _, event: &SliderEvent, cx| {
            let Some(transport) = this.transport() else {
                return;
            };
            let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
            let SliderValue::Single(seconds) = *value else {
                return;
            };
            transport.at = Duration::from_secs_f32(seconds.max(0.));
            // Only the thumb moves during the drag. Seeking every frame of one would restart the
            // decoder dozens of times across a single gesture.
            transport.scrubbing = matches!(event, SliderEvent::Change(_));
            if !transport.scrubbing {
                preview::playback::seek(transport.at, cx);
            }
            cx.notify();
        })
        .detach();

        Some(Self {
            path: path.clone(),
            length,
            art: preview::has_cover(&path),
            at: Duration::ZERO,
            playing: false,
            scrubbing: false,
            slider,
            tick: None,
        })
    }

    /// Whether the player is currently loaded with *this* transport's recording.
    fn is_current(&self, cx: &App) -> bool {
        preview::playback::playing(cx) == Some(self.path.as_path())
    }
}

/// Start the recording, or pause and resume it once it is loaded.
///
/// The poll starts with the first press rather than with the host: a recording nobody plays should
/// cost nothing, and there is no position to read before one is loaded.
pub fn toggle<V: Host>(this: &mut V, window: &mut Window, cx: &mut Context<V>) {
    let Some(transport) = this.transport() else {
        return;
    };
    let path = transport.path.clone();
    let needs_tick = transport.tick.is_none();
    if transport.is_current(cx) {
        preview::playback::toggle(cx);
    } else {
        preview::playback::play(&path, cx);
    }

    if needs_tick {
        let tick = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this
                    .update_in(cx, |this, window, cx| poll(this, window, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        if let Some(transport) = this.transport() {
            transport.tick = Some(tick);
        }
    }
    cx.notify();
}

/// Copy the player's position onto the transport. The slider is left alone mid-drag, or the thumb
/// snaps back to wherever playback is every quarter second.
pub fn poll<V: Host>(this: &mut V, window: &mut Window, cx: &mut Context<V>) {
    let Some((at, playing)) = preview::playback::position(cx) else {
        return;
    };
    let current = this
        .transport()
        .is_some_and(|transport| transport.is_current(cx));
    let Some(transport) = this.transport() else {
        return;
    };
    // Another transport took the device. Ours has not moved, and saying otherwise would put this
    // recording's thumb wherever that one happens to be.
    transport.playing = playing && current;
    if !current || transport.scrubbing {
        cx.notify();
        return;
    }
    transport.at = at;
    let seconds = at.as_secs_f32();
    let slider = transport.slider.clone();
    slider.update(cx, |slider, cx| slider.set_value(seconds, window, cx));
    cx.notify();
}

/// `1:04`, or `1:02:03` once there is an hour to show. A leading `0:` on a ten-minute tape is
/// noise, and a bare `62:03` on a long one is a number nobody reads as an hour.
fn clock(at: Duration) -> String {
    let total = at.as_secs();
    let (hours, minutes, seconds) = (total / 3600, (total / 60) % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Play/pause, the elapsed and total times, and the scrubber between them.
///
/// `compact` drops the scrubber to a width that fits the Details panel's preview box, which is a
/// dock the user can drag narrow — the full-width bar would clip its own clock off the end.
///
/// `AnyElement` for the reason [`super::find::panel`] gives: this is one more deeply-nested builder
/// chain, and handing its concrete type back to a caller's own chain is what overflows rustc.
pub fn bar<V: Host>(transport: &Transport, compact: bool, cx: &mut Context<V>) -> AnyElement {
    let total = transport.length.map(clock);

    h_flex()
        .gap_2()
        // A press on the scrubber is not the start of a drag across whatever is behind it.
        .occlude()
        .child(
            Button::new("play-pause")
                .icon(if transport.playing {
                    IconName::Pause
                } else {
                    IconName::Play
                })
                .outline()
                .tooltip(if transport.playing { "Pause" } else { "Play" })
                .on_click(cx.listener(|this, _, window, cx| toggle(this, window, cx))),
        )
        .child(div().px_1().font_semibold().child(clock(transport.at)))
        // Fixed rather than flexed, so the bar does not resize as the clock ticks past 9:59.
        .child(
            div()
                .w(if compact { px(120.) } else { px(240.) })
                .child(Slider::new(&transport.slider).horizontal()),
        )
        .when_some(total, |bar, total| {
            bar.child(
                div()
                    .px_1()
                    .text_color(cx.theme().muted_foreground)
                    .child(total),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use std::time::Duration;

    use crate::viewer::transport::clock;

    /// The readout is the only thing telling an archivist where they are in a two-hour interview,
    /// so both the short and the long form have to be right — and the seconds always two digits,
    /// or the number jitters in width as it counts.
    #[test]
    fn the_clock_grows_an_hours_field_only_when_there_is_one() {
        assert_eq!(clock(Duration::ZERO), "0:00");
        assert_eq!(clock(Duration::from_secs(64)), "1:04");
        assert_eq!(clock(Duration::from_secs(9 * 60 + 5)), "9:05");
        assert_eq!(clock(Duration::from_secs(59 * 60 + 59)), "59:59");
        assert_eq!(clock(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
        // Truncated, not rounded: a readout that reaches the total before the audio does looks
        // like a stall at the end of every recording.
        assert_eq!(clock(Duration::from_millis(1999)), "0:01");
    }
}

//! Right-side status-bar readout: a spinner and a count while previews are decoding, so a gallery
//! filling in reads as work under way rather than a slow app.

use std::time::Duration;

use gpui::*;
use gpui_component::{Sizable as _, h_flex, spinner::Spinner};

pub struct PreviewsBusy {
    pub shown: usize,
    _poll: Task<()>,
}

impl PreviewsBusy {
    /// ponytail: polls, since decodes finish on a background thread that cannot notify a view.
    /// Four cheap loads a second, and a re-render only when the count moves.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let _poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let now = preview::decoding();
                let alive = this.update(cx, |this, cx| {
                    if this.shown == now {
                        return;
                    }
                    // Appearing or leaving changes the bar's dividers, which only the bar redraws.
                    if (this.shown == 0) != (now == 0) {
                        cx.refresh_windows();
                    }
                    this.shown = now;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self { shown: 0, _poll }
    }
}

impl Render for PreviewsBusy {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(Spinner::new().xsmall())
            .child(format!("Loading previews ({})", self.shown))
    }
}

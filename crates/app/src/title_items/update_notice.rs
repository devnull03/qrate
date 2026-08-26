//! Compact title-bar surface for download progress and the explicit update restart.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{IconName, Sizable as _};

use crate::update_check::{AutoUpdater, UpdateStatus};

pub struct UpdateNotice {
    updater: Option<Entity<AutoUpdater>>,
    _sub: Option<Subscription>,
}

impl UpdateNotice {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let updater = AutoUpdater::get(cx);
        let sub = updater
            .as_ref()
            .map(|updater| cx.observe(updater, |_, _, cx| cx.notify()));
        Self { updater, _sub: sub }
    }

    pub fn occupied(cx: &App) -> bool {
        AutoUpdater::get(cx).is_some_and(|updater| updater.read(cx).visible())
    }
}

impl Render for UpdateNotice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(updater) = self.updater.as_ref() else {
            return div().into_any_element();
        };
        let updater = updater.read(cx);
        if !updater.visible() {
            return div().into_any_element();
        }
        let label = match updater.status() {
            UpdateStatus::Downloading {
                version,
                received,
                total,
            } => total
                .filter(|total| *total > 0)
                .map(|total| {
                    format!(
                        "Downloading v{version}: {}%",
                        received.saturating_mul(100) / total
                    )
                })
                .unwrap_or_else(|| format!("Downloading qrate v{version}…")),
            UpdateStatus::Ready { version, .. } => format!("Restart to update qrate to v{version}"),
            UpdateStatus::Error { .. } => "Update failed — open About to retry".into(),
            _ => return div().into_any_element(),
        };
        let ready = matches!(updater.status(), UpdateStatus::Ready { .. });
        gpui_component::h_flex()
            .id("update-notice")
            .gap_1()
            .items_center()
            .when(ready, |this| {
                this.cursor_pointer().on_click(move |_, _, cx| {
                    if let Err(error) = crate::update_check::restart(cx) {
                        log::error!("failed to restart for update: {error:#}");
                    }
                })
            })
            .child(label)
            .child(
                Button::new("dismiss-update")
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .on_click(|_, _, cx| {
                        if let Some(updater) = AutoUpdater::get(cx) {
                            updater.update(cx, |updater, cx| updater.dismiss(cx));
                        }
                    }),
            )
            .into_any_element()
    }
}

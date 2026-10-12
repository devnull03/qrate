//! Compact title-bar surface for download progress and the explicit update restart.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{
    Disableable as _, IconName, Sizable as _, progress::ProgressCircle, spinner::Spinner,
};

use crate::update_check::{AutoUpdater, UpdateStatus};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

pub(super) fn overflow_menu(menu: PopupMenu, cx: &mut Context<PopupMenu>) -> PopupMenu {
    let Some(updater) = AutoUpdater::get(cx) else {
        return menu;
    };
    if !updater.read(cx).visible() {
        return menu;
    }
    let status = updater.read(cx).status().clone();
    let menu = menu.separator();
    let menu = match &status {
        UpdateStatus::Downloading { version, .. } => menu.item(
            PopupMenuItem::new(format!(
                "Downloading qrate v{version}… {}%",
                status.progress_percent().unwrap_or_default()
            ))
            .disabled(true),
        ),
        UpdateStatus::Ready { .. } => {
            menu.item(PopupMenuItem::new("Restart to update").on_click(crate::restart_for_update))
        }
        UpdateStatus::Restarting => menu.item(PopupMenuItem::new("Restarting…").disabled(true)),
        UpdateStatus::Error { .. } => menu.item(
            PopupMenuItem::new("Update failed — open About")
                .on_click(|_, _, cx| crate::about::open_about_window(cx)),
        ),
        _ => return menu,
    };
    if matches!(
        status,
        UpdateStatus::Ready { .. } | UpdateStatus::Error { .. }
    ) {
        menu.item(
            PopupMenuItem::new("Dismiss update notice").on_click(move |_, _, cx| {
                updater.update(cx, |updater, cx| updater.dismiss(cx));
            }),
        )
    } else {
        menu
    }
}

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
        let control = match updater.status() {
            UpdateStatus::Downloading { version, .. } => {
                let percent = updater.status().progress_percent().unwrap_or_default();
                Button::new("update-downloading")
                    .label(format!("Downloading qrate v{version}…"))
                    .icon(ProgressCircle::new("update-progress").value(percent as f32))
                    .small()
                    .disabled(true)
                    .tooltip(format!("{percent}% downloaded"))
                    .into_any_element()
            }
            UpdateStatus::Ready { version, .. } => Button::new("update-ready")
                .label("Restart to update")
                .icon(IconName::ArrowDown)
                .small()
                .tooltip(format!("Install qrate v{version}"))
                .on_click(crate::restart_for_update)
                .into_any_element(),
            UpdateStatus::Restarting => Button::new("update-restarting")
                .label("Restarting…")
                .icon(Spinner::new().xsmall())
                .small()
                .disabled(true)
                .into_any_element(),
            UpdateStatus::Error { .. } => Button::new("update-error")
                .label("Update failed")
                .icon(IconName::TriangleAlert)
                .small()
                .tooltip("Open About to retry")
                .on_click(|_, _, cx| crate::about::open_about_window(cx))
                .into_any_element(),
            _ => return div().into_any_element(),
        };
        let dismissible = matches!(
            updater.status(),
            UpdateStatus::Ready { .. } | UpdateStatus::Error { .. }
        );
        gpui_component::h_flex()
            .id("update-notice")
            .gap_1()
            .items_center()
            // The title bar wraps its children in a `WindowControlArea::Drag` hitbox, which Windows
            // hit-tests as HTCAPTION and never delivers a click from — see `DockToggleButton`.
            .occlude()
            .child(control)
            .when(dismissible, |this| {
                this.child(
                    Button::new("dismiss-update")
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .tooltip("Dismiss")
                        .on_click(|_, _, cx| {
                            if let Some(updater) = AutoUpdater::get(cx) {
                                updater.update(cx, |updater, cx| updater.dismiss(cx));
                            }
                        }),
                )
            })
            .into_any_element()
    }
}

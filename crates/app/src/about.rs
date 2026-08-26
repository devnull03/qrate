//! About qrate, including the shared signed-updater status and manual retry surface.

use gpui::*;
use gpui_component::button::Button;
use gpui_component::{
    ActiveTheme, Root, Sizable as _, StyledExt as _, TitleBar, h_flex, label::Label, v_flex,
};
use window_wrapper::WindowRegistry;

use crate::update_check::{AutoUpdater, UpdateStatus};

const ABOUT_WINDOW_KIND: &str = "about";

pub struct AboutWindow {
    updater: Option<Entity<AutoUpdater>>,
    _sub: Option<Subscription>,
}

impl AboutWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("About qrate");
        let updater = AutoUpdater::get(cx);
        let sub = updater
            .as_ref()
            .map(|updater| cx.observe(updater, |_, _, cx| cx.notify()));
        crate::update_check::check_now(cx);
        Self { updater, _sub: sub }
    }
}

impl Render for AboutWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self
            .updater
            .as_ref()
            .map(|updater| updater.read(cx).status().clone())
            .unwrap_or_else(|| UpdateStatus::Disabled("Updater is unavailable".into()));
        v_flex()
            .size_full()
            .child(
                TitleBar::new()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .child(Label::new("About qrate").font_semibold()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(Label::new("qrate").text_lg().font_semibold())
                    .child(
                        Label::new(format!(
                            "Version {} ({})",
                            env!("CARGO_PKG_VERSION"),
                            env!("QRATE_GIT_SHA")
                        ))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                    )
                    .child(match status {
                        UpdateStatus::Disabled(reason) => Label::new(reason)
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .into_any_element(),
                        UpdateStatus::Idle => h_flex()
                            .gap_2()
                            .child(Label::new("You're up to date").text_sm())
                            .child(
                                Button::new("about-check")
                                    .label("Check again")
                                    .small()
                                    .on_click(|_, _, cx| crate::update_check::check_now(cx)),
                            )
                            .into_any_element(),
                        UpdateStatus::Checking => Label::new("Checking for updates…")
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .into_any_element(),
                        UpdateStatus::Downloading {
                            version,
                            received,
                            total,
                        } => {
                            let detail = total
                                .filter(|total| *total > 0)
                                .map(|total| format!(" — {}%", received * 100 / total))
                                .unwrap_or_default();
                            Label::new(format!("Downloading v{version}{detail}"))
                                .text_sm()
                                .into_any_element()
                        }
                        UpdateStatus::Ready {
                            version,
                            release_notes_url,
                        } => h_flex()
                            .gap_2()
                            .items_center()
                            .child(Label::new(format!("qrate v{version} is ready")).text_sm())
                            .child(
                                Button::new("about-release-notes")
                                    .label("Release notes")
                                    .small()
                                    .on_click(move |_, _, cx| cx.open_url(&release_notes_url)),
                            )
                            .child(
                                Button::new("about-restart")
                                    .label("Restart to update")
                                    .small()
                                    .on_click(move |_, _, cx| {
                                        if let Err(error) = crate::update_check::restart(cx) {
                                            log::error!("failed to restart for update: {error:#}");
                                        }
                                    }),
                            )
                            .into_any_element(),
                        UpdateStatus::Error { stage, message, .. } => h_flex()
                            .gap_2()
                            .child(Label::new(format!("{stage} failed: {message}")).text_sm())
                            .child(
                                Button::new("about-retry")
                                    .label("Retry")
                                    .small()
                                    .on_click(|_, _, cx| crate::update_check::check_now(cx)),
                            )
                            .into_any_element(),
                    }),
            )
    }
}

pub(crate) fn open_about_window(cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(ABOUT_WINDOW_KIND, cx).is_some() {
        return;
    }
    let win_size = size(px(460.0), px(240.0));
    let bounds = Bounds::centered(None, win_size, cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(win_size),
        ..Default::default()
    };
    if let Ok(window_handle) = cx.open_window(window_options, |window, cx| {
        let view = cx.new(|cx| AboutWindow::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(ABOUT_WINDOW_KIND, window_handle.into(), cx);
    }
}

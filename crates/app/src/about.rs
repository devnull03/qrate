//! The About window: version/build info plus an on-demand release check. Reuses
//! `update_check::check_now` for the HTTP call — this is the only other call site.

use gpui::*;
use gpui_component::button::Button;
use gpui_component::{
    ActiveTheme, Root, Sizable as _, StyledExt as _, TitleBar, h_flex, label::Label, v_flex,
};
use window_wrapper::WindowRegistry;

use crate::update_check::{self, UpdateStatus};

const ABOUT_WINDOW_KIND: &str = "about";

/// Result of the on-demand check this window triggers when it opens. `Done(None)` covers both a
/// network failure and a missing platform asset — `check_now` doesn't distinguish them, and
/// neither does the user's next move (try again later).
enum Check {
    Checking,
    Done(Option<UpdateStatus>),
}

pub struct AboutWindow {
    check: Check,
}

impl AboutWindow {
    fn new(cx: &mut Context<Self>) -> Self {
        let task = update_check::check_now(cx);
        cx.spawn(async move |this, cx| {
            let status = task.await;
            this.update(cx, |this, cx| {
                this.check = Check::Done(status);
                cx.notify();
            })
            .ok();
        })
        .detach();
        Self {
            check: Check::Checking,
        }
    }
}

impl Render for AboutWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(match &self.check {
                        Check::Checking => Label::new("Checking for updates…")
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .into_any_element(),
                        Check::Done(None) => Label::new("Could not check for updates")
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .into_any_element(),
                        Check::Done(Some(UpdateStatus::UpToDate)) => {
                            Label::new("You're up to date")
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .into_any_element()
                        }
                        Check::Done(Some(UpdateStatus::Available {
                            version,
                            download_url,
                        })) => {
                            let url = download_url.clone();
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Label::new(format!("Update available: v{version}")).text_sm(),
                                )
                                .child(
                                    Button::new("about-download")
                                        .label("Download")
                                        .small()
                                        .on_click(move |_, _, cx| cx.open_url(&url)),
                                )
                                .into_any_element()
                        }
                    }),
            )
    }
}

/// Opens the About window, focusing the existing one if it's already open.
pub(crate) fn open_about_window(cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(ABOUT_WINDOW_KIND, cx).is_some() {
        return;
    }
    let win_size = size(px(360.0), px(220.0));
    let bounds = Bounds::centered(None, win_size, cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(win_size),
        ..Default::default()
    };
    if let Ok(window_handle) = cx.open_window(window_options, |window, cx| {
        let view = cx.new(AboutWindow::new);
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(ABOUT_WINDOW_KIND, window_handle.into(), cx);
    }
}

//! Stage 1 · the Photoshop-style launcher shown at startup: a Recent
//! Projects list on the left, "Create New" entry cards on the right.
//!
//! Opening a recent project or "Create New" both need to hand off to the
//! real main app window, which lives in the `app` crate — a crate this one
//! can't depend on without a cycle. Callers pass that behavior in as a plain
//! `fn` pointer (mirrors `SettingsWindow::new`'s `build_pages: fn() -> ...`).

use gpui::{prelude::FluentBuilder, *};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Root, StyledExt, TitleBar, h_flex, v_flex};
use window_wrapper::WindowRegistry;

use crate::recent::{self, RecentProject};
use crate::wizard::{self, EntryKind};

pub const LAUNCHER_WINDOW_KIND: &str = "project-launcher";

/// Set once at startup (see `crates/app/src/main.rs`) so the launcher can
/// open the real main window without depending on the `app` crate.
#[derive(Clone, Copy)]
pub struct LauncherHooks {
    pub open_main_window: fn(&mut App),
}

impl Global for LauncherHooks {}

pub struct Launcher {
    recents: Vec<RecentProject>,
}

impl Launcher {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            recents: recent::list(cx),
        }
    }

    fn open_recent(&mut self, path: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        recent::record_opened(name, path, cx);
        if let Some(hooks) = cx.try_global::<LauncherHooks>().copied() {
            (hooks.open_main_window)(cx);
        }
        window.remove_window();
    }

    fn start_new(&mut self, entry_kind: EntryKind, cx: &mut Context<Self>) {
        wizard::open_project_wizard(entry_kind, cx);
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);

        let recent_list = if self.recents.is_empty() {
            v_flex().child(
                Label::new("No recent projects yet — create one to get started.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
        } else {
            let mut list = v_flex().gap_0();
            for (ix, project) in self.recents.iter().enumerate() {
                let path = project.path.clone();
                let name = project.name.clone();
                let when = recent::relative_time(project.opened_at_unix);
                list = list.child(
                    h_flex()
                        .id(("recent-project", ix))
                        .cursor_pointer()
                        .gap_2p5()
                        .items_start()
                        .py_2()
                        .when(ix > 0, |el| el.border_t_1().border_color(cx.theme().border))
                        .hover(|el| el.bg(cx.theme().secondary_hover))
                        .child(
                            div()
                                .size_7()
                                .flex_none()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border),
                        )
                        .child(
                            v_flex()
                                .min_w(px(0.))
                                .child(div().font_semibold().text_sm().child(project.name.clone()))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} · opened {when}", project.path)),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_recent(path.clone(), name.clone(), window, cx)
                        })),
                );
            }
            list
        };

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(TitleBar::new().child(Label::new("qrate").font_semibold()))
            .child(
                h_flex()
                    .id("launcher-body")
                    .flex_1()
                    .gap_5()
                    .p_5()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_2()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_baseline()
                                    .child(div().text_lg().font_semibold().child("Recent Projects"))
                                    .child(
                                        Label::new("Open other…")
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground),
                                    ),
                            )
                            .child(recent_list)
                            .child(
                                Label::new("Opening a recent project skips the wizard entirely.")
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    )
                    .child(
                        v_flex()
                            .w(px(220.))
                            .flex_none()
                            .gap_2()
                            .pl_5()
                            .border_l_1()
                            .border_color(cx.theme().border)
                            .child(div().text_lg().font_semibold().child("Create New"))
                            .child(create_card(
                                "new-blank",
                                "Blank",
                                "Add files whenever you're ready.",
                                cx,
                            ))
                            .child(create_card(
                                "new-csv",
                                "CSV + folder",
                                "Import a spreadsheet and its files.",
                                cx,
                            ))
                            .child(create_card(
                                "new-sheet",
                                "Google Sheet",
                                "Use a shared spreadsheet link.",
                                cx,
                            )),
                    ),
            )
            .children(dialog_layer)
    }
}

fn create_card(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    cx: &mut Context<Launcher>,
) -> impl IntoElement {
    let entry_kind = match id {
        "new-csv" => EntryKind::Csv,
        "new-sheet" => EntryKind::Sheet,
        _ => EntryKind::Blank,
    };
    v_flex()
        .id(id)
        .cursor_pointer()
        .gap_1()
        .p_2p5()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .hover(|el| el.border_color(cx.theme().primary))
        .child(div().font_semibold().text_sm().child(title))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(description),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.start_new(entry_kind, cx)))
}

pub fn open_launcher_window(cx: &mut App) {
    if WindowRegistry::focus_or_clear(LAUNCHER_WINDOW_KIND, cx) {
        return;
    }
    let bounds = Bounds::centered(None, size(px(760.0), px(480.0)), cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(Size::new(px(600.0), px(380.0))),
        ..Default::default()
    };

    cx.spawn(async move |cx| {
        let result = cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| Launcher::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Ok(window_handle) = result {
            cx.update(|cx| {
                WindowRegistry::register(LAUNCHER_WINDOW_KIND, window_handle.into(), cx);
            })
            .ok();
        }
    })
    .detach();
}

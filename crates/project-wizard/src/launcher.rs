//! Stage 1 · the Photoshop-style launcher shown at startup: a Recent
//! Projects list on the left, "Create New" entry cards on the right.
//!
//! Opening a recent project or "Create New" both need to hand off to the
//! real main app window, which lives in the `app` crate — a crate this one
//! can't depend on without a cycle. Callers pass that behavior in as a plain
//! `fn` pointer (mirrors `SettingsWindow::new`'s `build_pages: fn() -> ...`).

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, IconName, Root, Sizable, StyledExt, TitleBar, h_flex, v_flex};
use window_wrapper::WindowRegistry;

use crate::project;
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
    /// Shown above the recents list when opening a project fails (missing or
    /// unreadable `.qrate`).
    error: Option<SharedString>,
}

impl Launcher {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            recents: recent::list(cx),
            error: None,
        }
    }

    /// Loads the `.qrate` file at `path`, sets it current, and hands off to
    /// the main window. On failure the launcher stays up and shows why.
    fn open_project_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        match project::open_project(std::path::Path::new(&path), cx) {
            Ok(name) => {
                recent::record_opened(name, path, cx);
                if let Some(hooks) = cx.try_global::<LauncherHooks>().copied() {
                    (hooks.open_main_window)(cx);
                }
                window.remove_window();
            }
            Err(e) => {
                self.error = Some(format!("Couldn't open that project — {e}").into());
                cx.notify();
            }
        }
    }

    /// "Open other…" — pick a `.qrate` file anywhere on disk.
    fn open_other(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a project (.qrate) file".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let file = path.to_string_lossy().to_string();
                this.update_in(cx, |this, window, cx| {
                    this.open_project_file(file, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn start_new(&mut self, entry_kind: EntryKind, window: &mut Window, cx: &mut Context<Self>) {
        wizard::open_project_wizard(entry_kind, cx);
        // Close the launcher while the wizard is up; `go_back` from the wizard's
        // first step reopens it. Without this it lingers behind the new project.
        window.remove_window();
    }

    fn remove_recent(&mut self, path: String, cx: &mut Context<Self>) {
        recent::remove(&path, cx);
        self.recents = recent::list(cx);
        cx.notify();
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
                                .flex_1()
                                .min_w(px(0.))
                                .child(div().font_semibold().text_sm().child(project.name.clone()))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} · opened {when}", project.path)),
                                ),
                        )
                        .child({
                            let remove_path = project.path.clone();
                            Button::new(("remove-recent", ix))
                                .icon(IconName::Close)
                                .ghost()
                                .xsmall()
                                // Stop the click from also opening the project.
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_recent(remove_path.clone(), cx);
                                }))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_project_file(path.clone(), window, cx)
                        })),
                );
            }
            list
        };

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                TitleBar::new()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .child(Label::new("qrate").font_semibold()),
            )
            .child(
                h_flex()
                    .id("launcher-body")
                    .flex_1()
                    .min_h(px(0.))
                    .h_full()
                    .gap_5()
                    .p_5()
                    .child(
                        v_flex()
                            .flex_1()
                            // min_h(0) overrides the flex default min-height:auto so the inner list scrolls, not the column.
                            .min_h(px(0.))
                            .min_w(px(0.))
                            .gap_2()
                            .child(
                                h_flex()
                                    .flex_none()
                                    .justify_between()
                                    .items_baseline()
                                    .child(div().text_lg().font_semibold().child("Recent Projects"))
                                    .child(
                                        div()
                                            .id("open-other")
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .cursor_pointer()
                                            .hover(|el| el.text_color(cx.theme().primary))
                                            .child("Open other…")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_other(window, cx)
                                            })),
                                    ),
                            )
                            .when_some(self.error.clone(), |el, msg| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .text_sm()
                                        .text_color(cx.theme().danger)
                                        .child(msg),
                                )
                            })
                            // Heading above stays pinned; only the list scrolls.
                            .child(
                                div()
                                    .id("recents-scroll")
                                    .flex_1()
                                    .min_h(px(0.))
                                    .overflow_y_scroll()
                                    .child(v_flex().gap_2().child(recent_list)),
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
        .on_click(cx.listener(move |this, _, window, cx| this.start_new(entry_kind, window, cx)))
}

pub fn open_launcher_window(cx: &mut App) {
    if WindowRegistry::focus_or_clear(LAUNCHER_WINDOW_KIND, cx).is_some() {
        return;
    }
    let bounds = Bounds::centered(None, size(px(760.0), px(480.0)), cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(Size::new(px(600.0), px(380.0))),
        ..Default::default()
    };

    // Open synchronously: gpui quits when the window list is empty (non-macOS), so a window
    // spawned from an async task would leave a zero-window gap that kills the app mid-transition.
    if let Ok(window_handle) = cx.open_window(window_options, |window, cx| {
        let view = cx.new(|cx| Launcher::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(LAUNCHER_WINDOW_KIND, window_handle.into(), cx);
    }
}

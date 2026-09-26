//! Stage 1 · the Photoshop-style launcher shown at startup: a Recent
//! Projects list on the left, "Create New" entry cards on the right.
//!
//! Opening a recent project or "Create New" both need to hand off to the
//! real main app window, which lives in the `app` crate — a crate this one
//! can't depend on without a cycle. Callers pass that behavior in as a plain
//! `fn` pointer (mirrors `SettingsWindow::new`'s `build_pages: fn() -> ...`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::label::Label;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{
    ActiveTheme, Icon, IconName, Root, Sizable, StyledExt, TitleBar, h_flex, v_flex,
};
use window_wrapper::WindowRegistry;

use crate::example;
use crate::project;
use crate::recent::{self, RecentProject};
use crate::wizard::{self, EntryKind};

pub const LAUNCHER_WINDOW_KIND: &str = "project-launcher";

/// Development launch mode: show the first-run welcome even with recent projects.
pub struct OnboardingPreview;
impl Global for OnboardingPreview {}

/// The launcher's one spacing unit — the window's inset on every side, what each column keeps off
/// the divider, and the air under each heading. One value so nothing here drifts a few pixels away
/// from the rest and makes the window look assembled from parts.
const GAP: Pixels = px(24.);

/// How far a `text_lg` heading already sits below the top of its own box, and above the bottom of
/// it: line height, which no padding can see. Subtracted wherever a heading meets a [`GAP`], or the
/// window reads top-heavy against its even sides.
const HEADING_LEAD: Pixels = px(12.);

/// Width the overlay scrollbar takes out of the recents column's right inset. Without it the list
/// sits further from the divider than the Create New cards do, and the divider stops looking
/// centred between them.
const SCROLLBAR: Pixels = px(6.);

/// Asks, on the given window, whether to save the open project's unsaved cell edits, then runs
/// the callback unless the user cancels. Saving is the table's, which this crate can't reach.
pub type ResolveUnsaved = fn(&str, &mut Window, &mut App, Box<dyn FnOnce(&mut App)>);

/// Set once at startup (see `crates/app/src/main.rs`) so the launcher can
/// open the real main window without depending on the `app` crate.
#[derive(Clone, Copy)]
pub struct LauncherHooks {
    pub open_main_window: fn(&mut App),
    pub resolve_unsaved: ResolveUnsaved,
    /// The title bar's contents — the app menus and the updater's state. Same inversion as
    /// above: every action in them belongs to the `app` crate, which builds the view.
    pub title_items: fn(&mut App) -> AnyView,
}

impl Global for LauncherHooks {}

pub struct Launcher {
    recents: Vec<RecentProject>,
    previews: HashMap<String, std::path::PathBuf>,
    /// Shown above the recents list when opening a project fails (missing or
    /// unreadable `.qrate`).
    error: Option<SharedString>,
    /// `None` in a build with no hooks installed — the launcher is the first window up, and it
    /// still has to open without them.
    title_items: Option<AnyView>,
}

impl Launcher {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("Open Project — qrate");
        let title_items = cx
            .try_global::<LauncherHooks>()
            .copied()
            .map(|hooks| (hooks.title_items)(cx));
        let recents = if cx.has_global::<OnboardingPreview>() {
            Vec::new()
        } else {
            recent::list(cx)
        };
        if !recents.is_empty() {
            let paths: Vec<_> = recents.iter().map(|project| project.path.clone()).collect();
            let scan = cx.background_spawn(async move {
                paths
                    .into_iter()
                    .filter_map(|path| {
                        recent::preview_source(Path::new(&path)).map(|image| (path, image))
                    })
                    .collect::<HashMap<_, _>>()
            });
            cx.spawn(async move |this, cx| {
                let previews = scan.await;
                this.update(cx, |this, cx| {
                    this.previews = previews;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        Self {
            recents,
            previews: HashMap::new(),
            error: None,
            title_items,
        }
    }

    /// Runs `then` once the open project's unsaved edits are saved or dropped, never if the user
    /// cancels — the project about to be replaced is the one holding them.
    fn after_unsaved(
        detail: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let handle = window.window_handle();
        let launcher = cx.entity().downgrade();
        let then: Box<dyn FnOnce(&mut App)> = Box::new(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    launcher.update(cx, |this, cx| then(this, window, cx))
                })
                .ok();
        });
        match cx.try_global::<LauncherHooks>().copied() {
            Some(hooks) => (hooks.resolve_unsaved)(detail, window, cx, then),
            None => cx.defer(then),
        }
    }

    /// Loads the `.qrate` file at `path`, sets it current, and hands off to
    /// the main window. On failure the launcher stays up and shows why.
    fn open_project_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        Self::after_unsaved(
            "Save them before opening another project?",
            window,
            cx,
            move |this, window, cx| match project::open_project(std::path::Path::new(&path), cx) {
                Ok(name) => {
                    settings::dirty::clear(settings::dirty::PROJECT_DATA, cx);
                    recent::record_opened(name, path, cx);
                    if let Some(hooks) = cx.try_global::<LauncherHooks>().copied() {
                        (hooks.open_main_window)(cx);
                    }
                    window.remove_window();
                }
                Err(e) => {
                    this.error = Some(format!("Couldn't open that project — {e}").into());
                    cx.notify();
                }
            },
        );
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

    /// "Open example" — unpacks the bundled sample collection the first time, then opens it like
    /// any other project, so the Getting started guide and Problems behave as they would for real.
    fn open_example(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match example::ensure_example_project(cx) {
            Ok(file) => self.open_project_file(file.to_string_lossy().into_owned(), window, cx),
            Err(e) => {
                self.error = Some(format!("Couldn't set up the example project — {e}").into());
                cx.notify();
            }
        }
    }

    /// First run, 1b: a short welcome where the recents list would be — what qrate is, the two
    /// things people worry about (an account, their media), the example, and opening a project
    /// that already exists as its own button so it can't be mistaken for a way to create one.
    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let reassurance = |strong: &'static str, rest: &'static str| {
            h_flex()
                .gap_2()
                .items_center()
                .text_sm()
                .child(
                    Icon::new(IconName::CircleCheck)
                        .small()
                        .text_color(cx.theme().success),
                )
                .child(div().font_semibold().child(strong))
                .child(div().text_color(muted).child(rest))
        };
        v_flex()
            .flex_1()
            .min_h(px(0.))
            .min_w(px(0.))
            .pr(GAP)
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .pb_1()
                    .child("Welcome to qrate"),
            )
            .child(
                div()
                    .text_sm()
                    .child("Build a catalog from a spreadsheet, or start with a blank one."),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .mt_3p5()
                    .child(reassurance(
                        "No account needed.",
                        "Projects live on this computer.",
                    ))
                    .child(reassurance(
                        "Your media stays in its folder.",
                        "qrate links to it.",
                    )),
            )
            .when_some(self.error.clone(), |el, msg| {
                el.child(
                    div()
                        .mt_3()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(msg),
                )
            })
            .child(
                h_flex()
                    .id("open-example")
                    .gap_3()
                    .items_center()
                    .mt(px(18.))
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_dashed()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().tiles)
                    .child(project_thumbnail(Thumbnail::Example, px(56.), cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .font_semibold()
                                    .text_sm()
                                    .child("Explore an example project"),
                            )
                            .child(div().text_sm().text_color(muted).child(
                                "Four slides from a family collection, with photos to browse \
                                     in Gallery and findings to review in Problems.",
                            )),
                    )
                    .child(
                        Button::new("open-example-button")
                            .label("Open example")
                            .outline()
                            .small()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_example(window, cx)),
                            ),
                    ),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .pt(px(14.))
                    .pb(px(18.))
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("open-existing")
                            .icon(IconName::FolderOpen)
                            .label("Open an existing .qrate project…")
                            .outline()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_other(window, cx)),
                            ),
                    )
                    .child(div().text_xs().text_color(muted).child("or drop one here")),
            )
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

    fn accept_drop(
        &mut self,
        paths: &[std::path::PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let is_project = |path: &std::path::Path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("qrate"))
        };
        if paths.len() > 1 && paths.iter().any(|path| is_project(path)) {
            let project = paths.iter().find(|path| is_project(path)).cloned();
            let material: Vec<_> = paths
                .iter()
                .filter(|path| !is_project(path))
                .cloned()
                .collect();
            let answer = window.prompt(
                PromptLevel::Info,
                "Open a project or import material?",
                Some("A project file and import material were dropped together."),
                &["Open project", "Start project from material", "Cancel"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                match answer.await.unwrap_or(2) {
                    0 => {
                        if let Some(project) = project {
                            this.update_in(cx, |this, window, cx| {
                                this.open_project_file(
                                    project.to_string_lossy().into_owned(),
                                    window,
                                    cx,
                                )
                            })
                            .ok();
                        }
                    }
                    1 if !material.is_empty() => {
                        this.update_in(cx, |_, window, cx| {
                            wizard::open_project_wizard_seeded(
                                EntryKind::Blank,
                                None,
                                material,
                                cx,
                            );
                            window.remove_window();
                        })
                        .ok();
                    }
                    _ => {}
                }
            })
            .detach();
            return;
        }
        let path = &paths[0];
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if paths.len() == 1 && extension == "qrate" {
            self.open_project_file(path.to_string_lossy().into_owned(), window, cx);
            return;
        }
        if paths.len() == 1 && path.is_dir() {
            wizard::open_project_wizard_seeded(EntryKind::Blank, None, paths.to_vec(), cx);
            window.remove_window();
            return;
        }
        if paths.len() == 1
            && matches!(
                extension.as_str(),
                "csv" | "tsv" | "xlsx" | "xlsm" | "xlsb" | "xls" | "ods"
            )
        {
            wizard::open_project_wizard_seeded(
                EntryKind::LocalFile,
                Some(path.to_string_lossy().into_owned()),
                Vec::new(),
                cx,
            );
            window.remove_window();
            return;
        }
        if paths.iter().all(|path| path.is_file() || path.is_dir()) {
            wizard::open_project_wizard_seeded(EntryKind::Blank, None, paths.to_vec(), cx);
            window.remove_window();
        } else {
            self.error = Some("One or more dropped paths no longer exist.".into());
            cx.notify();
        }
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);

        let recent_list = {
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
                        .child(project_thumbnail(
                            Thumbnail::Recent(
                                self.previews.get(&project.path).map(PathBuf::as_path),
                            ),
                            px(28.),
                            cx,
                        ))
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
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().secondary_hover))
            .on_drop(cx.listener(|_, paths: &ExternalPaths, window, cx| {
                let paths = paths.paths().to_vec();
                Self::after_unsaved(
                    "Save them before switching projects?",
                    window,
                    cx,
                    move |this, window, cx| this.accept_drop(&paths, window, cx),
                )
            }))
            .child(
                TitleBar::new()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    // The window's name is the first menu's own label once `app` has installed
                    // its hooks; the plain word is what a build without them falls back to.
                    .map(|bar| match self.title_items.clone() {
                        Some(items) => bar.child(h_flex().flex_1().pr_4().child(items)),
                        None => bar.child(Label::new("qrate").font_semibold()),
                    }),
            )
            .child(
                h_flex()
                    .id("launcher-body")
                    .flex_1()
                    // `h_flex` centres its children on the cross axis, which let a recents list
                    // taller than the window hang off both ends of the body — over the title bar
                    // at the top, past the frame at the bottom, and never scrolling because
                    // nothing bounded its height. Stretched, the column is exactly the body tall
                    // and `min_h(0)` lets the list inside it scroll.
                    .items_stretch()
                    .min_h(px(0.))
                    // No gap between the columns: the recents column runs to the divider so its
                    // scrollbar sits at the far right, and the list pads itself off it instead.
                    // No bottom inset: the list runs to the frame, so a long history reads as
                    // continuing rather than as ending in a band of empty window.
                    .px(GAP)
                    .pt(GAP - HEADING_LEAD)
                    .when(self.recents.is_empty(), |body| {
                        body.child(self.render_welcome(cx))
                    })
                    .when(!self.recents.is_empty(), |body| {
                        body.child(
                            v_flex()
                                .flex_1()
                                // min_h(0) overrides the flex default min-height:auto so the inner list scrolls, not the column.
                                .min_h(px(0.))
                                .min_w(px(0.))
                                .gap(px(0.))
                                .child(
                                    h_flex()
                                        .flex_none()
                                        .justify_between()
                                        .items_baseline()
                                        // Everything but the scrollbar keeps clear of the divider by
                                        // the same amount the Create New column does.
                                        .pr(GAP)
                                        // The heading carries the air under it, not the list: the
                                        // rows scroll, and a gap that scrolls with them is a gap that
                                        // disappears the moment somebody uses the list.
                                        .pb(GAP - HEADING_LEAD)
                                        .child(
                                            div()
                                                .text_lg()
                                                .font_semibold()
                                                .child("Recent Projects"),
                                        )
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
                                            .pr(GAP)
                                            .pb(GAP - HEADING_LEAD)
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
                                        // Scrollbar, not bare overflow: a launcher opened on a long
                                        // history has to say there is more of it below the fold.
                                        .overflow_y_scrollbar()
                                        // Matches the inset on the other side of the divider, and holds
                                        // the rows off the scrollbar drawn at this column's right edge.
                                        .pr(GAP - SCROLLBAR)
                                        .child(recent_list),
                                ),
                        )
                    })
                    .child(
                        v_flex()
                            .w(px(220.))
                            .flex_none()
                            .gap(GAP - HEADING_LEAD)
                            .pl(GAP)
                            .pb(GAP)
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
                                "new-local",
                                "Spreadsheet + folder",
                                "Import a CSV or Excel file and its folder.",
                                cx,
                            ))
                            // Public-link import is independent of the OAuth export/sync setting.
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

enum Thumbnail<'a> {
    Example,
    Recent(Option<&'a Path>),
}

fn project_thumbnail(image: Thumbnail<'_>, size: Pixels, cx: &App) -> AnyElement {
    let frame = div()
        .size(size)
        .flex_none()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().tiles)
        .overflow_hidden();
    match image {
        Thumbnail::Example => frame
            .child(
                img(std::sync::Arc::new(Image::from_bytes(
                    ImageFormat::Jpeg,
                    example::THUMBNAIL.to_vec(),
                )))
                .size_full()
                .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        Thumbnail::Recent(Some(path)) => frame
            .child(preview::thumb(
                Some(path),
                preview::CARD,
                ObjectFit::Cover,
                cx,
            ))
            .into_any_element(),
        Thumbnail::Recent(None) => frame
            .flex()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(IconName::File).size_4())
            .into_any_element(),
    }
}

fn create_card(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    cx: &mut Context<Launcher>,
) -> impl IntoElement {
    let entry_kind = match id {
        "new-local" => EntryKind::LocalFile,
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

/// Opens (or focuses) the launcher with `error` shown above the recents list.
pub fn open_launcher_with_error(error: SharedString, cx: &mut App) {
    open_launcher_window(cx);
    let Some(root) = WindowRegistry::focus_or_clear(LAUNCHER_WINDOW_KIND, cx)
        .and_then(|handle| handle.downcast::<Root>())
    else {
        return;
    };
    root.update(cx, |root, _, cx| {
        if let Ok(launcher) = root.view().clone().downcast::<Launcher>() {
            launcher.update(cx, |launcher, cx| {
                launcher.error = Some(error);
                cx.notify();
            });
        }
    })
    .ok();
}

mod actions;
mod app_menus;
mod app_settings;
mod components;
mod helpers;
mod status_items;
mod theming;
mod title_items;

use gpui::*;
use gpui_component::{Root, TitleBar, v_flex};
use project_wizard::{EntryKind, LauncherHooks};
use settings::{
    AppSettings, MainWindowBounds, SettingsPersistence, SettingsWindow, SettingsWriter,
    load_app_settings,
};
use window_wrapper::{
    OpenBrowser, WindowLock, WindowRegistry, status_bar::StatusBar, title_bar::AppTitleBar,
};

const SETTINGS_WINDOW_KIND: &str = "settings";
const MAIN_WINDOW_KIND: &str = "main";
/// `.qrate` `__settings` key for the per-project remembered window size/display.
/// Mirrors `workspace::DOCK_LAYOUT_KEY`'s project-first, global-fallback pattern.
const MAIN_WINDOW_BOUNDS_KEY: &str = "main_window_bounds";

/// Weak handle to the one-and-only main-window `Workspace`, so `open_main_window`
/// can reload its layout when it *focuses* an already-open window instead of
/// building a fresh one (see `Workspace::reload_layout`).
struct MainWorkspaceHandle(WeakEntity<Workspace>);
impl Global for MainWorkspaceHandle {}

use crate::app_settings::build_pages;
use crate::{
    actions::{NewProject, ToggleBottomDock, ToggleLeftDock, ToggleRightDock},
    app_menus::{OpenProjects, OpenSettings, Quit, app_menus},
    status_items::build_status_bar_registry,
    title_items::build_title_bar_registry,
};
use gpui_component::dock::DockPlacement;
use workspace::Workspace;

/// Opens the Settings window, focusing the existing one if it's already open.
// `gpui::App`, spelled out: a bare `App` here would resolve to this file's own
// `pub struct App` below, not the GPUI context type — local items always win
// over glob imports regardless of source order.
pub(crate) fn open_settings_window(cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(SETTINGS_WINDOW_KIND, cx) {
        return;
    }
    let bounds = Bounds::centered(None, size(px(1000.0), px(800.0)), cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(Size::new(px(600.0), px(400.0))),
        ..Default::default()
    };

    cx.spawn(async move |cx| {
        let result = cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| SettingsWindow::new(window, cx, build_pages));
            cx.new(|cx| Root::new(view, window, cx))
        });

        if let Ok(window_handle) = result {
            cx.update(|cx| {
                WindowRegistry::register(SETTINGS_WINDOW_KIND, window_handle.into(), cx);
            })
            .ok();
        }
    })
    .detach();
}

/// Opens the real main app window, focusing the existing one if it's already open. Called by
/// the launcher (`project-wizard` crate) when a recent project is opened or a wizard finishes.
pub(crate) fn open_main_window(cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(MAIN_WINDOW_KIND, cx) {
        // The window already exists (opening a project just switched `CurrentProject`
        // to a different one) — reload its layout instead of leaving the previous
        // project's panels in place.
        if let Some(handle) = WindowRegistry::get(MAIN_WINDOW_KIND, cx) {
            handle
                .update(cx, |_, window, cx| {
                    if let Some(workspace) = cx
                        .try_global::<MainWorkspaceHandle>()
                        .and_then(|h| h.0.upgrade())
                    {
                        workspace.update(cx, |ws, cx| ws.reload_layout(window, cx));
                    }
                })
                .ok();
        }
        return;
    }

    let project_bounds = cx
        .try_global::<settings::project::CurrentProject>()
        .and_then(|p| settings::project::read_setting(&p.file, MAIN_WINDOW_BOUNDS_KEY).ok())
        .flatten()
        .and_then(|raw| serde_json::from_str::<MainWindowBounds>(&raw).ok());
    let (main_bounds, main_display) = match &project_bounds {
        Some(b) => MainWindowBounds::startup_placement(Some(b), cx),
        None => AppSettings::get(cx).main_window_startup_placement(cx),
    };
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(main_bounds)),
        display_id: main_display,
        window_min_size: Some(Size::new(px(520.0), px(300.0))),
        ..Default::default()
    };

    cx.spawn(async move |cx| {
        let result = cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| App::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });

        if let Ok(window_handle) = result {
            cx.update(|cx| {
                WindowRegistry::register(MAIN_WINDOW_KIND, window_handle.into(), cx);
            })
            .ok();
        }
    })
    .detach();
}

pub struct App {
    workspace: Entity<Workspace>,
    status_bar: Entity<StatusBar>,
    _main_window_bounds_sub: Subscription,
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        cx.set_global(MainWorkspaceHandle(workspace.downgrade()));
        let status_bar = cx.new(|_| StatusBar::new());

        let dock = workspace.read(cx).dock_area();
        let status_registry = build_status_bar_registry(&mut *cx, dock.clone());
        cx.set_global(status_registry);
        let title_registry = build_title_bar_registry(&mut *cx, dock);
        cx.set_global(title_registry);

        let _main_window_bounds_sub = cx.observe_window_bounds(window, |_, window, cx| {
            let b = MainWindowBounds::capture_from_window(window, cx);
            match cx.try_global::<settings::project::CurrentProject>() {
                Some(project) => {
                    if let Ok(json) = serde_json::to_string(&b)
                        && let Err(err) = settings::project::write_setting(
                            &project.file,
                            MAIN_WINDOW_BOUNDS_KEY,
                            &json,
                        )
                    {
                        eprintln!("failed to save window bounds to project: {err}");
                    }
                }
                None => AppSettings::update(cx, |s| {
                    s.main_window_bounds = Some(b);
                }),
            }
        });

        // Block the OS close button while a background task is running.
        window.on_window_should_close(cx, |_, cx| !WindowLock::is_locked(cx));

        Self {
            workspace,
            status_bar,
            _main_window_bounds_sub,
        }
    }

    fn toggle_dock(
        &mut self,
        placement: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |ws, cx| ws.toggle_dock(placement, window, cx));
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);

        div()
            .size_full()
            // Dock toggles are handled here on the root so the shortcuts work window-wide,
            // regardless of which panel currently holds focus.
            .key_context("qrate")
            .on_action(cx.listener(|this, _: &ToggleLeftDock, window, cx| {
                this.toggle_dock(DockPlacement::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleBottomDock, window, cx| {
                this.toggle_dock(DockPlacement::Bottom, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRightDock, window, cx| {
                this.toggle_dock(DockPlacement::Right, window, cx)
            }))
            .child(
                v_flex()
                    .size_full()
                    .child(AppTitleBar::new())
                    .child(
                        div()
                            .id("window-body")
                            .w_full()
                            .flex_1()
                            .child(self.workspace.clone()),
                    )
                    .child(self.status_bar.clone()),
            )
            .children(dialog_layer)
    }
}

fn main() {
    let app = Application::new().with_assets(gpui_component_assets::Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        cx.set_global(WindowLock::default());

        // Settings ------------------------------------
        let settings = load_app_settings().unwrap_or_default();
        cx.set_global(settings);
        cx.set_global(SettingsPersistence {
            writer: Some(SettingsWriter::start()),
        });
        theming::init(cx);
        cx.set_global(WindowRegistry::default());

        // Lets the launcher (in the `project-wizard` crate, which can't depend on `app`) open
        // the real main window without a crate cycle. See `project_wizard::launcher`.
        cx.set_global(LauncherHooks { open_main_window });

        cx.on_action(|_: &OpenSettings, cx| open_settings_window(cx));
        cx.on_action(|_: &NewProject, cx| {
            project_wizard::open_project_wizard(EntryKind::Blank, cx)
        });
        cx.on_action(|_: &OpenProjects, cx| project_wizard::open_launcher_window(cx));
        // ----------------------------------------------

        cx.set_menus(app_menus());

        // Keyboard shortcuts (Layer 1). See `actions.rs` to add more.
        cx.bind_keys(actions::key_bindings());
        actions::register_global_handlers(cx);

        cx.on_action(|action: &OpenBrowser, cx| {
            cx.open_url(&action.url);
        });

        cx.on_action(|_: &Quit, cx| {
            cx.quit();
        });

        // The launcher (Recent Projects + Create New) is the real startup window — it opens
        // the main window itself for a recent project, or a project-creation wizard window for
        // "Create New".
        project_wizard::open_launcher_window(cx);
    });
}

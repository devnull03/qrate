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
// `gpui::App` spelled out: a bare `App` binds to this file's own `struct App`, not the context type.
pub(crate) fn open_settings_window(cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(SETTINGS_WINDOW_KIND, cx) {
        return;
    }
    // Reopen at the saved size (persisted per-resize), else a compact default.
    let saved = AppSettings::get(cx)
        .values
        .get(settings::SETTINGS_WINDOW_BOUNDS_KEY)
        .map(|v| v.text())
        .and_then(|raw| serde_json::from_str::<MainWindowBounds>(&raw).ok());
    let display = saved.as_ref().and_then(|b| b.display_id).and_then(|raw| {
        cx.displays()
            .into_iter()
            .find(|d| u64::from(d.id()) == raw)
            .map(|d| d.id())
    });
    let win_size = saved
        .as_ref()
        .filter(|b| {
            b.width.is_finite() && b.height.is_finite() && b.width >= 480.0 && b.height >= 360.0
        })
        .map(|b| size(px(b.width), px(b.height)))
        .unwrap_or_else(|| size(px(760.0), px(560.0)));
    let bounds = Bounds::centered(display, win_size, cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id: display,
        window_min_size: Some(Size::new(px(480.0), px(360.0))),
        ..Default::default()
    };

    // Open synchronously: gpui quits when the window list is empty (non-macOS), so a window
    // spawned from an async task would leave a zero-window gap that kills the app mid-transition.
    if let Ok(window_handle) = cx.open_window(window_options, |window, cx| {
        let view = cx.new(|cx| SettingsWindow::new(window, cx, build_pages));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(SETTINGS_WINDOW_KIND, window_handle.into(), cx);
    }
}

/// Opens the real main app window, focusing the existing one if it's already open. Called by
/// the launcher (`project-wizard` crate) when a recent project is opened or a wizard finishes.
/// Sets the OS window title to "<project> — qrate" (just "qrate" with no project open).
fn set_main_window_title(window: &mut Window, cx: &gpui::App) {
    let title = cx
        .try_global::<settings::project::CurrentProject>()
        .map(|p| format!("{} — qrate", p.display_name()))
        .unwrap_or_else(|| "qrate".into());
    window.set_window_title(&title);
}

pub(crate) fn open_main_window(cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(MAIN_WINDOW_KIND, cx) {
        // Window already exists (a project switch); reload its layout instead of keeping the old one's.
        if let Some(handle) = WindowRegistry::get(MAIN_WINDOW_KIND, cx) {
            handle
                .update(cx, |_, window, cx| {
                    if let Some(workspace) = cx
                        .try_global::<MainWorkspaceHandle>()
                        .and_then(|h| h.0.upgrade())
                    {
                        workspace.update(cx, |ws, cx| ws.reload_layout(window, cx));
                    }
                    set_main_window_title(window, cx);
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

    // Open synchronously — see `open_settings_window` for why (quit-on-empty-window-list).
    if let Ok(window_handle) = cx.open_window(window_options, |window, cx| {
        let view = cx.new(|cx| App::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(MAIN_WINDOW_KIND, window_handle.into(), cx);
    }
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
        let status_bar = cx.new(|_| StatusBar);

        let dock = workspace.read(cx).dock_area();
        let status_registry = build_status_bar_registry(&mut *cx, dock.clone());
        cx.set_global(status_registry);
        let title_registry = build_title_bar_registry(&mut *cx, dock);
        cx.set_global(title_registry);

        let _main_window_bounds_sub = cx.observe_window_bounds(window, |_, window, cx| {
            let b = MainWindowBounds::capture_from_window(window, cx);
            match cx.try_global::<settings::project::CurrentProject>() {
                Some(project) => {
                    // Debounced — this observer fires on every pixel of a
                    // resize/move drag, so don't block the UI thread on file I/O.
                    if let Ok(json) = serde_json::to_string(&b) {
                        settings::project::queue_write(
                            &project.file,
                            MAIN_WINDOW_BOUNDS_KEY,
                            &json,
                            cx,
                        );
                    }
                }
                None => AppSettings::update(cx, |s| {
                    s.main_window_bounds = Some(b);
                }),
            }
        });

        // Native X skips `on_app_quit` on Windows (zed#40385/#40290), so flush here too — unless locked.
        window.on_window_should_close(cx, |_, cx| {
            if WindowLock::is_locked(cx) {
                return false;
            }
            flush_all_state(cx);
            true
        });

        set_main_window_title(window, cx);

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
                    .child(AppTitleBar::new(
                        cx.try_global::<settings::project::CurrentProject>()
                            .map(|p| p.display_name())
                            .unwrap_or_default(),
                    ))
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

/// Final persist before the app or main window goes away. Called from both
/// `on_app_quit` (menu Quit) and `on_window_should_close` (native X), since on
/// Windows the native close doesn't route through the app-quit path.
fn flush_all_state(cx: &mut gpui::App) {
    if let Some(dock) = cx
        .try_global::<MainWorkspaceHandle>()
        .and_then(|h| h.0.upgrade())
        .and_then(|ws| ws.read(cx).dock_area().upgrade())
    {
        Workspace::persist_layout(&dock, cx);
    }
    if let Some(writer) = cx
        .try_global::<settings::project::ProjectPersistence>()
        .and_then(|p| p.writer.clone())
    {
        writer.flush();
    }
    if let Err(err) = settings::flush_app_settings(AppSettings::get(cx)) {
        eprintln!("failed to flush app settings on quit: {err}");
    }
    // Everything above reached disk synchronously, so nothing is outstanding.
    settings::dirty::clear_all(cx);
}

fn main() {
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        cx.set_global(WindowLock::default());

        // Settings ------------------------------------
        let settings = load_app_settings().unwrap_or_default();
        cx.set_global(settings);
        cx.set_global(SettingsPersistence {
            writer: Some(SettingsWriter::start()),
        });
        cx.set_global(settings::project::ProjectPersistence {
            writer: Some(settings::project::ProjectSettingsWriter::start()),
        });
        settings::dirty::init(cx);
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

        // Flush before exit: writers debounce 450ms, and dock toggles/resizes never emit `LayoutChanged`.
        cx.on_app_quit(|cx| {
            flush_all_state(cx);
            async {}
        })
        .detach();

        // The launcher is the real startup window; it opens the main window or the wizard itself.
        project_wizard::open_launcher_window(cx);
    });
}

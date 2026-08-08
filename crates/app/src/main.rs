mod actions;
mod app_menus;
mod app_settings;
mod logging;
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
    app_menus::{
        CopyDebugInfo, OpenLogsFolder, OpenPluginsFolder, OpenProjects, OpenSettings, Quit,
        REPO_URL, ReloadPlugins, ReportIssue, app_menus,
    },
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
    /// Repaints the title bar's unsaved-changes dot when the dirty set changes. `dirty::mark`/
    /// `clear` mutate the global, so this fires on every edit and every save.
    _dirty_sub: Subscription,
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
        window.on_window_should_close(cx, |window, cx| {
            if WindowLock::is_locked(cx) {
                return false;
            }
            // Unsaved cell edits (autosave off, or a pending "timed" save) are the only thing the
            // user might want to discard — layout/settings auto-persist on the flush below.
            if settings::dirty::Dirty::has(settings::dirty::PROJECT_DATA, cx) {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "You have unsaved changes.",
                    Some("Save them before closing?"),
                    &["Save", "Don't Save", "Cancel"],
                    cx,
                );
                cx.spawn(async move |cx| {
                    let choice = answer.await.unwrap_or(2);
                    cx.update(|cx| {
                        match choice {
                            // Save: the quit flush persists PROJECT_DATA (still dirty).
                            0 => {}
                            // Don't Save: drop the mark so the flush skips it — edits are discarded.
                            1 => settings::dirty::clear(settings::dirty::PROJECT_DATA, cx),
                            // Cancel: leave the window open.
                            _ => return,
                        }
                        cx.quit();
                    });
                })
                .detach();
                // Veto this close; the async handler quits once the user decides.
                return false;
            }
            flush_all_state(cx);
            true
        });

        set_main_window_title(window, cx);

        let _dirty_sub = cx.observe_global::<settings::dirty::Dirty>(|_, cx| cx.notify());

        Self {
            workspace,
            status_bar,
            _main_window_bounds_sub,
            _dirty_sub,
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
                    .child(
                        AppTitleBar::new(
                            cx.try_global::<settings::project::CurrentProject>()
                                .map(|p| p.display_name())
                                .unwrap_or_default(),
                        )
                        // Only cell data (gated by autosave/Ctrl+S) can be genuinely unsaved;
                        // column layout/settings auto-persist via the debounced writer, so `any()`
                        // would light the dot forever for those (nothing clears them until quit).
                        .dirty(settings::dirty::Dirty::has(
                            settings::dirty::PROJECT_DATA,
                            cx,
                        )),
                    )
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

/// Register the spell checker, once its dictionary has parsed off the UI thread — half a
/// megabyte of affix rules is not something to put in front of first paint.
///
/// The only place that links both the checker and the table, which is what lets `spellcheck` know
/// nothing about grids and `table` nothing about dictionaries.
fn register_spell_checker(cx: &mut gpui::App) {
    if !spellcheck::enabled(cx) {
        log::info!("spell checking is turned off in settings");
        return;
    }
    let language = spellcheck::language(cx);
    let ignore_capitalized = spellcheck::ignore_capitalized(cx);
    let load = cx
        .background_executor()
        .spawn(async move { spellcheck::SpellCheck::load(&language, ignore_capitalized) });
    cx.spawn(async move |cx| {
        let Some(spell) = load.await else {
            return;
        };
        cx.update(|cx| {
            diagnostics::Validators::register(Box::new(spell.clone()), cx);
            cx.set_global(spell);
            cx.set_global(diagnostics::SpellActions {
                suggest: spellcheck::misspellings,
                add_word: |word, cx| {
                    if spellcheck::add_word(word, cx) {
                        table::revalidate_now(cx);
                    }
                },
            });
            // A project may already be open by the time the dictionary lands.
            table::revalidate_now(cx);
        });
    })
    .detach();
}

/// Select and scroll to whatever a problem points at. Three index spaces meet here: the
/// diagnostic's source row and column *name*, the delegate's filtered view row, and the table's
/// display column (data col + 1, past the pinned `#`).
fn reveal_in_table(location: &diagnostics::Location, cx: &mut gpui::App) {
    let Some(state) = cx
        .try_global::<table::TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return;
    };

    let delegate_lookup = {
        let delegate = state.read(cx).delegate();
        let row = location.row.map(|r| delegate.view_row(r));
        let col = location
            .column
            .as_ref()
            .map(|c| delegate.data_col(c).map(|ix| ix + 1));
        (row, col)
    };

    // ponytail: a row filtered out of view is silently unreachable; clearing the user's filter
    // on what they think is a navigation click would be worse than nothing happening.
    match delegate_lookup {
        (Some(Some(row)), Some(Some(col))) => {
            state.update(cx, |s, cx| s.set_selected_cell(row, col, cx))
        }
        (Some(Some(row)), None) => state.update(cx, |s, cx| s.set_selected_row(row, cx)),
        (None, Some(Some(col))) => state.update(cx, |s, cx| s.set_selected_col(col, cx)),
        _ => {}
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
        log::error!("failed to flush app settings on quit: {err}");
    }
    // Flush unsaved cell edits (a pending "timed" autosave, or edits made with autosave off).
    if settings::dirty::Dirty::has(settings::dirty::PROJECT_DATA, cx) {
        table::save_now(cx);
    }
    // Everything above reached disk synchronously, so nothing is outstanding.
    settings::dirty::clear_all(cx);
}

fn main() {
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);

    app.run(move |cx| {
        // First, so a failure during the startup below — the case where no window ever appears to
        // report it — still reaches the log file.
        logging::init();

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

        // Same inversion for the Problems panel: `diagnostics` must not depend on `table`, so
        // the jump comes back through here. See `diagnostics::DiagnosticHooks`.
        cx.set_global(diagnostics::DiagnosticHooks {
            reveal: reveal_in_table,
            text_at: table::cell_text,
            set_text: table::set_cell_text,
        });
        diagnostics::init(cx);

        // The composition root is the only place that knows where validators come from; `table`
        // triggers the runs without naming any of them.
        plugin_host::on_command_finished(table::revalidate_now);
        plugin_host::reload(cx);
        register_spell_checker(cx);
        diagnostics::AsyncValidators::register(
            table::file_links::SOURCE,
            table::file_links::check,
            cx,
        );

        cx.on_action(|_: &ReloadPlugins, cx| {
            plugin_host::reload(cx);
            table::revalidate_now(cx);
        });
        cx.on_action(|_: &OpenPluginsFolder, _| plugin_host::open_plugins_folder());

        cx.on_action(|_: &CopyDebugInfo, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(logging::debug_info(cx, 200)));
        });
        cx.on_action(|_: &ReportIssue, cx| {
            // A shorter tail than the clipboard gets: GitHub stops honouring `?body=` somewhere
            // past 8 KB, and a silently truncated report is worse than a short one.
            let body = logging::urlencode(&logging::debug_info(cx, 30));
            cx.open_url(&format!("{REPO_URL}/issues/new?body={body}"));
        });
        cx.on_action(|_: &OpenLogsFolder, _| match logging::reveal_target() {
            Some(target) => {
                if let Err(err) = settings::os_open::reveal_in_folder(&target) {
                    log::error!("failed to reveal the log folder: {err}");
                }
            }
            None => log::error!("no local data dir, so there is no log folder to open"),
        });
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

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about;
mod actions;
mod agent_bridge;
mod app_menus;
mod app_settings;
mod assets;
mod export;
mod google;
mod instance_handoff;
mod logging;
mod plugin_marketplace;
mod site;
mod status_items;
mod theming;
mod title_items;
mod update_check;

use gpui::*;
use gpui_component::{Root, TitleBar, v_flex};
use project_wizard::{EntryKind, LauncherHooks};
use settings::{
    AppSettings, MainWindowBounds, SettingsPersistence, SettingsWindow, SettingsWriter,
    load_app_settings,
};
use window_wrapper::{OpenBrowser, WindowRegistry, status_bar::StatusBar, title_bar::AppTitleBar};

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
    actions::{NewProject, ToggleBottomDock, ToggleLeftDock, ToggleProblemsPanel, ToggleRightDock},
    app_menus::{
        CopyDebugInfo, LoadColumnConfig, OpenAbout, OpenColumnSettings, OpenLogsFolder,
        OpenPluginsFolder, OpenProjects, OpenSettings, Quit, ReloadPlugins, ReportBug,
        ReportUxIssue, RequestFeature,
    },
    status_items::build_status_bar_registry,
    title_items::build_title_bar_registry,
};
use gpui_component::dock::DockPlacement;
use workspace::Workspace;

/// Opens the Settings window, focusing the existing one if it's already open.
// `gpui::App` spelled out: a bare `App` binds to this file's own `struct App`, not the context type.
pub(crate) fn open_settings_window(initial_page: Option<usize>, cx: &mut gpui::App) {
    if WindowRegistry::focus_or_clear(SETTINGS_WINDOW_KIND, cx).is_some() {
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
        let view = cx.new(|cx| SettingsWindow::new(window, cx, build_pages, initial_page));
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
    let started = std::time::Instant::now();
    // Window already exists (a project switch); reload its layout instead of keeping the old one's.
    if let Some(handle) = WindowRegistry::focus_or_clear(MAIN_WINDOW_KIND, cx) {
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
        return;
    }

    let project_bounds = cx
        .try_global::<settings::project::CurrentProject>()
        .and_then(|p| settings::project::read_setting(&p.file, MAIN_WINDOW_BOUNDS_KEY).ok())
        .flatten()
        .and_then(|raw| serde_json::from_str::<MainWindowBounds>(&raw).ok());
    let (main_bounds, main_display) = MainWindowBounds::startup_placement(
        project_bounds
            .as_ref()
            .or(AppSettings::get(cx).main_window_bounds.as_ref()),
        cx,
    );
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
        log::info!("opened the main window in {:?}", started.elapsed());
    }
}

pub struct App {
    workspace: Entity<Workspace>,
    status_bar: Entity<StatusBar>,
    /// Focused at startup so the `qrate` root is always on the dispatch path. Without it nothing
    /// in the window holds focus until a panel is clicked, and gpui dispatches to the *window*
    /// root instead — so Ctrl+B and friends match their binding but reach no handler.
    focus_handle: FocusHandle,
    _main_window_bounds_sub: Subscription,
    /// Repaints the title bar's unsaved-changes dot when the dirty set changes. `dirty::mark`/
    /// `clear` mutate the global, so this fires on every edit and every save.
    _dirty_sub: Subscription,
    _settings_sub: Subscription,
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

        // Native X skips `on_app_quit` on Windows (zed#40385/#40290), so flush here too.
        window.on_window_should_close(cx, |window, cx| {
            if settings::dirty::Dirty::has(settings::dirty::PROJECT_DATA, cx) {
                resolve_unsaved(
                    "Save them before closing?",
                    window,
                    cx,
                    Box::new(|cx| {
                        settings::dirty::clear(settings::dirty::PROJECT_DATA, cx);
                        cx.quit();
                    }),
                );
                // Veto this close; the prompt quits once the user decides.
                return false;
            }
            flush_all_state(cx);
            true
        });

        set_main_window_title(window, cx);

        let _dirty_sub = cx.observe_global::<settings::dirty::Dirty>(|_, cx| cx.notify());
        let _settings_sub = cx.observe_global::<AppSettings>(|_, cx| cx.notify());

        let focus_handle = cx.focus_handle();
        if window.focused(cx).is_none() {
            focus_handle.focus(window, cx);
        }

        Self {
            workspace,
            status_bar,
            focus_handle,
            _main_window_bounds_sub,
            _dirty_sub,
            _settings_sub,
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
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .size_full()
            // Dock toggles are handled here on the root so the shortcuts work window-wide,
            // regardless of which panel currently holds focus.
            .key_context("qrate")
            .track_focus(&self.focus_handle)
            .id("qrate-root")
            .role(Role::Group)
            .on_action(cx.listener(|this, _: &ToggleLeftDock, window, cx| {
                this.toggle_dock(DockPlacement::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleBottomDock, window, cx| {
                this.toggle_dock(DockPlacement::Bottom, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRightDock, window, cx| {
                this.toggle_dock(DockPlacement::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleProblemsPanel, window, cx| {
                if let Some(dock) = this.workspace.read(cx).dock_area().upgrade() {
                    workspace::PanelRegistry::toggle("ProblemsPanel", &dock, window, cx);
                }
            }))
            // Same reason as the dock toggles: switching views re-arranges the docks, so it needs
            // a `Window`, and it has to fire with focus in any panel — not just the centre.
            .on_action(cx.listener(|_, action: &workspace::ShowView, window, cx| {
                workspace::show_view(action.mode, window, cx)
            }))
            // Here rather than globally: the Zotero mapping dialog opens in this window.
            .on_action(cx.listener(|_, action: &export::Export, window, cx| {
                export::run(action.format, window, cx)
            }))
            // Here for the same reason: the dialog opens in this window.
            .on_action(cx.listener(|_, _: &LoadColumnConfig, window, cx| {
                project_wizard::open_column_config_dialog(window, cx)
            }))
            .on_action(
                cx.listener(|_, action: &export::PluginExport, _, cx| {
                    export::run_plugin(action, cx)
                }),
            )
            .child(
                v_flex()
                    .size_full()
                    .child(
                        AppTitleBar::new(
                            cx.try_global::<settings::project::CurrentProject>()
                                .map(|p| p.display_name())
                                .unwrap_or_default(),
                        )
                        .author(settings::history::author(cx).unwrap_or_else(|| "Not set".into()))
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
            .children(notification_layer)
    }
}

/// Which spell-checker load is current, so a slow earlier load cannot land over a newer choice.
#[derive(Default)]
struct SpellLoad(u64);

impl gpui::Global for SpellLoad {}

/// (Re)register the spell checker, once its dictionaries have parsed off the UI thread — half a
/// megabyte of affix rules is not something to put in front of first paint. Called again whenever a
/// spelling setting changes, so the new choice applies without a restart.
///
/// The only place that links both the checker and the table, which is what lets `spellcheck` know
/// nothing about grids and `table` nothing about dictionaries.
pub(crate) fn register_spell_checker(cx: &mut gpui::App) {
    let generation = {
        let load = cx.default_global::<SpellLoad>();
        load.0 += 1;
        load.0
    };
    if !spellcheck::enabled(cx) {
        log::info!("spell checking is turned off in settings");
        diagnostics::Validators::remove(&spellcheck::SPELLING_VALIDATOR_NAME.into(), cx);
        return;
    }
    let languages = spellcheck::languages(cx);
    let ignore_capitalized = spellcheck::ignore_capitalized(cx);
    let load = cx.background_executor().spawn(async move {
        let started = std::time::Instant::now();
        let spell = spellcheck::SpellCheck::load(&languages, ignore_capitalized);
        log::info!("loaded dictionaries in {:?}", started.elapsed());
        spell
    });
    cx.spawn(async move |cx| {
        let spell = load.await;
        cx.update(|cx| {
            if cx.global::<SpellLoad>().0 != generation {
                return;
            }
            diagnostics::Validators::remove(&spellcheck::SPELLING_VALIDATOR_NAME.into(), cx);
            let Some(spell) = spell else {
                return;
            };
            diagnostics::Validators::register(Box::new(spell.clone()), cx);
            diagnostics::GroupFixProviders::register(
                spellcheck::SPELLING_VALIDATOR_NAME,
                spellcheck::spelling_group_fixes,
                cx,
            );
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

fn register_variant_checker(cx: &mut gpui::App) {
    let variants = clustering::ValueVariants::default();
    diagnostics::Validators::register(Box::new(variants.clone()), cx);
    diagnostics::FixProviders::register(
        clustering::VALUE_VARIANTS_NAME,
        clustering::variant_fixes,
        cx,
    );
    diagnostics::GroupFixProviders::register(
        clustering::VALUE_VARIANTS_NAME,
        clustering::variant_group_fixes,
        cx,
    );
    cx.set_global(variants);
    table::revalidate_now(cx);
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
    if settings::dirty::Dirty::has(settings::dirty::PROJECT_DATA, cx)
        && let Err(error) = table::save_now(cx)
    {
        log::error!("unsaved cell edits could not be written on quit: {error}");
    }
    // Everything else reached disk synchronously above; cell edits stay marked if their save failed.
    for domain in settings::dirty::Dirty::domains(cx) {
        if domain != settings::dirty::PROJECT_DATA {
            settings::dirty::clear(domain, cx);
        }
    }
    agent_bridge::shutdown();
}

/// Runs `then` once unsaved cell edits are saved or knowingly dropped, asking on `window` if there
/// are any. Cancel never runs it, and a failed save asks again with the error.
pub(crate) fn resolve_unsaved(
    detail: &str,
    window: &mut Window,
    cx: &mut gpui::App,
    then: Box<dyn FnOnce(&mut gpui::App)>,
) {
    if !settings::dirty::Dirty::has(settings::dirty::PROJECT_DATA, cx) {
        cx.defer(then);
        return;
    }
    let answer = window.prompt(
        PromptLevel::Warning,
        "You have unsaved changes.",
        Some(detail),
        &["Save", "Don't Save", "Cancel"],
        cx,
    );
    let handle = window.window_handle();
    let detail = detail.to_string();
    cx.spawn(async move |cx| {
        let choice = answer.await.unwrap_or(2);
        cx.update(|cx| match choice {
            0 => match table::save_now(cx) {
                Ok(()) => then(cx),
                Err(error) => {
                    let detail = format!("Saving failed: {error}\n\n{detail}");
                    handle
                        .update(cx, |_, window, cx| {
                            resolve_unsaved(&detail, window, cx, then)
                        })
                        .ok();
                }
            },
            1 => {
                then(cx);
            }
            _ => {}
        });
    })
    .detach();
}

/// [`resolve_unsaved`] for a menu command, which arrives with no window: it asks on the main
/// window, the one holding the edits.
fn resolve_unsaved_from_menu(
    detail: &'static str,
    cx: &mut gpui::App,
    then: Box<dyn FnOnce(&mut gpui::App)>,
) {
    match WindowRegistry::focus_or_clear(MAIN_WINDOW_KIND, cx) {
        Some(main) => {
            main.update(cx, |_, window, cx| {
                resolve_unsaved(detail, window, cx, then)
            })
            .ok();
        }
        None => then(cx),
    }
}

pub(crate) fn restart_for_update(_: &ClickEvent, window: &mut Window, cx: &mut gpui::App) {
    resolve_unsaved(
        "Save them before restarting to update?",
        window,
        cx,
        Box::new(|cx| match update_check::prepare_restart(cx) {
            Ok(helper) => {
                settings::dirty::clear(settings::dirty::PROJECT_DATA, cx);
                flush_all_state(cx);
                cx.set_restart_path(helper);
                cx.restart();
            }
            Err(error) => {
                log::error!("failed to prepare update restart: {error:#}");
                if let Some(updater) = update_check::AutoUpdater::get(cx) {
                    updater.update(cx, |updater, cx| updater.fail_restart(&error, cx));
                }
            }
        }),
    );
}

fn main() {
    // First, so failures in GPUI platform construction and startup still reach the log file.
    logging::init();
    log::info!("site origin: {}", site::url("/"));
    let initial_project = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .find(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("qrate"))
        });
    let initial_link = std::env::args()
        .find(|argument| argument.starts_with("qrate://"))
        .filter(|link| {
            plugin_package::parse_install_link(link)
                .inspect_err(|error| log::warn!("ignored invalid plugin install link: {error:#}"))
                .is_ok()
        });
    if initial_link.is_some() {
        log::info!("received plugin install link at startup");
    }
    let (url_sender, url_receiver) = async_channel::unbounded();
    if !instance_handoff::start(initial_link.as_deref(), url_sender.clone()) {
        return;
    }
    let app = gpui_platform::application().with_assets(assets::Assets);
    app.on_open_urls(move |urls| {
        for url in urls {
            log::info!("received plugin install link from the operating system");
            let _ = url_sender.try_send(url);
        }
    });

    app.run(move |cx| {
        let started = std::time::Instant::now();
        gpui_component::init(cx);
        cx.register_url_scheme("qrate").detach();

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
        // Here and only here: past the single-instance hand-off, before any tier loads a part.
        components::init(cx);
        components::found_by(components::ComponentId::Pdfium, preview::pdfium_found, cx);
        components::found_by(components::ComponentId::Ffmpeg, preview::ffmpeg_found, cx);
        preview::cache::set_cap(app_settings::preview_cache_bytes(cx));
        theming::init(cx);
        cx.set_global(WindowRegistry::default());

        // Lets the launcher (in the `project-wizard` crate, which can't depend on `app`) open
        // the real main window without a crate cycle. See `project_wizard::launcher`.
        cx.set_global(LauncherHooks {
            open_main_window,
            resolve_unsaved,
            title_items: title_items::launcher_bar::LauncherBar::view,
        });

        // Same inversion for the Problems panel: `diagnostics` must not depend on `table`, so
        // the jump comes back through here. See `diagnostics::DiagnosticHooks`.
        cx.set_global(diagnostics::DiagnosticHooks {
            reveal: reveal_in_table,
            text_at: table::cell_text,
            set_text: table::set_cell_text,
            set_texts: table::set_cell_texts,
            revalidate: table::revalidate_now,
        });
        diagnostics::init(cx);
        log::debug!(
            "startup: settings and theme ready at {:?}",
            started.elapsed()
        );

        // The composition root is the only place that knows where validators come from; `table`
        // triggers the runs without naming any of them.
        plugin_host::on_command_finished(table::revalidate_now);
        plugin_host::reload(cx);
        log::debug!("startup: plugins loaded at {:?}", started.elapsed());
        register_variant_checker(cx);
        register_spell_checker(cx);
        update_check::init(cx);
        diagnostics::AsyncValidators::register(
            table::file_links::SOURCE,
            table::file_links::check,
            cx,
        );
        checks::init(cx);
        agent_bridge::init(cx);
        agent_runtime::init(cx);
        log::debug!(
            "startup: validators and bridges registered at {:?}",
            started.elapsed()
        );

        cx.on_action(|_: &ReloadPlugins, cx| {
            plugin_host::reload(cx);
            app_menus::install(cx);
            table::revalidate_now(cx);
        });
        cx.on_action(|_: &OpenPluginsFolder, _| plugin_host::open_plugins_folder());
        cx.on_action(|_: &app_menus::DiscoverPlugins, cx| plugin_marketplace::open_catalog(cx));
        cx.on_action(|_: &app_menus::ManagePlugins, cx| {
            open_settings_window(Some(app_settings::PLUGINS_PAGE), cx)
        });

        cx.on_action(|_: &CopyDebugInfo, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(logging::debug_info(cx, 200)));
        });
        cx.on_action(|_: &ReportBug, cx| {
            cx.open_url(&logging::feedback_url(cx, Some(logging::FeedbackKind::Bug)));
        });
        cx.on_action(|_: &RequestFeature, cx| {
            cx.open_url(&logging::feedback_url(
                cx,
                Some(logging::FeedbackKind::Feature),
            ));
        });
        cx.on_action(|_: &ReportUxIssue, cx| {
            cx.open_url(&logging::feedback_url(cx, Some(logging::FeedbackKind::Ux)));
        });
        cx.on_action(|_: &OpenLogsFolder, _| match logging::reveal_target() {
            Some(target) => {
                if let Err(err) = settings::os_open::reveal_in_folder(&target) {
                    log::error!("failed to reveal the log folder: {err}");
                }
            }
            None => log::error!("no local data dir, so there is no log folder to open"),
        });
        cx.on_action(|_: &OpenSettings, cx| open_settings_window(None, cx));
        cx.on_action(|_: &OpenColumnSettings, cx| {
            open_settings_window(Some(app_settings::COLUMNS_PAGE), cx)
        });
        cx.on_action(|_: &OpenAbout, cx| about::open_about_window(cx));
        cx.on_action(|_: &NewProject, cx| {
            project_wizard::open_project_wizard(EntryKind::Blank, cx)
        });
        cx.on_action(|_: &OpenProjects, cx| project_wizard::open_launcher_window(cx));
        // ----------------------------------------------

        app_menus::install(cx);

        // Keyboard shortcuts (Layer 1). See `actions.rs` to add more.
        cx.bind_keys(actions::key_bindings());
        actions::register_global_handlers(cx);

        cx.on_action(|action: &OpenBrowser, cx| {
            cx.open_url(&action.url);
        });

        cx.on_action(|_: &Quit, cx| {
            resolve_unsaved_from_menu(
                "Save them before quitting?",
                cx,
                Box::new(|cx| {
                    settings::dirty::clear(settings::dirty::PROJECT_DATA, cx);
                    cx.quit();
                }),
            )
        });

        cx.spawn(async move |cx| {
            while let Ok(link) = url_receiver.recv().await {
                cx.update(|cx| open_install_link(&link, cx));
            }
        })
        .detach();

        // Flush before exit: writers debounce 450ms, and dock toggles/resizes never emit `LayoutChanged`.
        cx.on_app_quit(|cx| {
            flush_all_state(cx);
            async {}
        })
        .detach();

        match initial_link {
            Some(link) if open_install_link(&link, cx) => {}
            _ if initial_project.as_ref().is_some_and(
                |path| match project_wizard::open_project(path, cx) {
                    Ok(name) => {
                        project_wizard::record_opened(
                            name,
                            path.to_string_lossy().into_owned(),
                            cx,
                        );
                        open_main_window(cx);
                        true
                    }
                    Err(error) => {
                        log::error!("could not open project {}: {error:#}", path.display());
                        project_wizard::open_launcher_with_error(
                            format!("Couldn't open {} — {error:#}", path.display()).into(),
                            cx,
                        );
                        true
                    }
                },
            ) => {}
            // The launcher is the normal startup window; it opens the main window or the wizard.
            _ => project_wizard::open_launcher_window(cx),
        }
    });
}

fn open_install_link(link: &str, cx: &mut gpui::App) -> bool {
    match plugin_package::parse_install_link(link) {
        Ok(target) => {
            log::info!("opening plugin installation target: {target:?}");
            plugin_marketplace::open_install_target(target, cx);
            true
        }
        Err(error) => {
            log::warn!("ignored invalid plugin install link: {error:#}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    /// `components::init` sweeps away every component folder without a receipt, including one a
    /// running qrate is installing into, so it must follow the single-instance hand-off. It must
    /// also come before anything that loads a component.
    #[test]
    fn components_are_swept_after_the_hand_off_and_before_any_tier_loads() {
        let source = include_str!("main.rs");
        let main =
            &source[source.find("fn main()").unwrap()..source.rfind("#[cfg(test)]").unwrap()];
        let at = |call: &str| {
            main.find(call)
                .unwrap_or_else(|| panic!("main() no longer calls {call}"))
        };
        let init = at("components::init(cx)");
        assert_eq!(
            main.matches("components::init(").count(),
            1,
            "once per process"
        );
        assert!(at("instance_handoff::start(") < init);
        for later in ["preview::", "agent_runtime::init(", "plugin_host::reload("] {
            assert!(init < at(later), "{later} runs before the sweep");
        }
    }
}

//! Persisted preferences, reusable setting field builders, path picker widgets, and a generic
//! settings-window shell (`SettingsWindow`). Product-specific pages live in `app`.

pub mod columns;
pub mod description;
pub mod dirty;
pub use qrate_export::filenames;
pub mod history;
pub mod onboarding;
pub mod os_open;
pub mod path_picker;
pub mod plugins;
pub mod project;

mod db;

pub use db::{SettingsWriter, data_dir, flush_app_settings, load_app_settings};
/// Increment when the persisted SQLite JSON schema (`db::PersistSettings`) changes.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

use std::collections::HashMap;
use std::sync::Arc;

use gpui::*;
use gpui_component::{
    ActiveTheme as _, StyledExt, TitleBar,
    input::InputState,
    label::Label,
    setting::{SelectIndex, SettingField, SettingItem, SettingPage, Settings},
    v_flex,
};
use serde::{Deserialize, Serialize};

use crate::path_picker::PathPickerApp;

/// `AppSettings` value key for the Settings window's last size (a JSON [`MainWindowBounds`]).
pub const SETTINGS_WINDOW_BOUNDS_KEY: &str = "settings_window_bounds";

/// Setting key for autosave behavior: `"timed"` (buffered, the default), `"immediate"`,
/// or `"off"`. Read by the table crate to decide when a committed cell edit reaches disk.
pub const AUTOSAVE_KEY: &str = "autosave";

/// User-wide author for new notes and history entries. Empty leaves them unsigned.
pub const NOTE_AUTHOR_KEY: &str = "note_author";

/// `AppSettings` key for whether Google Sheets export and sync are enabled. User-wide only: a
/// project must not opt the person using qrate into an external service. Authentication is a
/// separate action, and public-sheet import does not use this setting.
pub const GOOGLE_SYNC_KEY: &str = "google_sync_enabled";

/// Whether Google sync is switched on. Deliberately not [`effective_bool`]: a project must not be
/// able to turn on a connection the person using it never agreed to.
pub fn google_enabled(cx: &App) -> bool {
    AppSettings::get(cx)
        .values
        .get(GOOGLE_SYNC_KEY)
        .map(|v| v.bool())
        .unwrap_or(false)
}

/// `AppSettings` key for the credential endpoint. Empty means the built-in default; pointing it
/// elsewhere is how someone runs this flow against their own Google Cloud project.
pub const GOOGLE_CONFIG_ENDPOINT_KEY: &str = "google_config_endpoint";

/// Setting key for the string that separates several values inside one cell, e.g.
/// `;` in `Film; Video`. Empty means a cell is one indivisible value.
///
/// Lives here rather than in `table` because it is no longer only the table's: a plugin checking a
/// column has to split a cell the same way the grid does, and `plugin-host` cannot depend on
/// `table` to ask.
pub const FILTER_SUBDELIMITER_KEY: &str = "filter_subdelimiter";

/// Whether the open project holds a value of its own for `key`, as opposed to inheriting the
/// user-wide one. Legacy overrides remain visible and can be cleared from Settings.
fn has_project_override(key: &str, cx: &App) -> bool {
    cx.try_global::<project::CurrentProject>()
        .is_some_and(|p| p.data.values.contains_key(key))
}

/// Drops the open project's own value for `key`, so it inherits the user-wide one again.
fn clear_project_override(key: &str, cx: &mut App) {
    if has_project_override(key, cx) {
        project::CurrentProject::clear(key, cx);
    }
}

/// New edits are user-wide. Clear an old project override so the edit takes effect now.
pub fn set_user_bool(key: &'static str, val: bool, cx: &mut App) {
    clear_project_override(key, cx);
    AppSettings::set_bool(key, val, cx);
}

/// Text sibling of [`set_user_bool`].
pub fn set_user_text(key: &'static str, val: SharedString, cx: &mut App) {
    clear_project_override(key, cx);
    AppSettings::set_text(key, val, cx);
}

/// Resolves a bool setting for a *consumer* (e.g. the table's stripe toggle): the open project's
/// value wins if present, else the user-wide default, else `false`.
pub fn effective_bool(key: &str, cx: &App) -> bool {
    if let Some(project) = cx.try_global::<project::CurrentProject>()
        && let Some(v) = project.data.values.get(key)
    {
        return v.bool();
    }
    AppSettings::get(cx)
        .values
        .get(key)
        .map(|v| v.bool())
        .unwrap_or(false)
}

/// Resolves a text setting for a *consumer*: the open project's value wins if present, else the
/// user-wide default, else empty. Text sibling of [`effective_bool`].
pub fn effective_text(key: &str, cx: &App) -> SharedString {
    if let Some(project) = cx.try_global::<project::CurrentProject>()
        && let Some(v) = project.data.values.get(key)
    {
        return v.text();
    }
    AppSettings::get(cx)
        .values
        .get(key)
        .map(|v| v.text())
        .unwrap_or_default()
}

// --- Setting Field Enum ---

pub enum Setting {
    Text {
        key: &'static str,
        label: &'static str,
        description: &'static str,
    },
    TextWithAction {
        key: &'static str,
        label: &'static str,
        description: &'static str,
        on_change: fn(&mut App),
    },
    Switch {
        key: &'static str,
        label: &'static str,
        description: &'static str,
    },
    Dropdown {
        key: &'static str,
        label: &'static str,
        description: &'static str,
        options: &'static [(&'static str, &'static str)],
    },
    /// A [`Setting::Dropdown`] whose consumer has to be told, as `TextWithAction` is to Text.
    DropdownWithAction {
        key: &'static str,
        label: &'static str,
        description: &'static str,
        options: &'static [(&'static str, &'static str)],
        on_change: fn(&mut App),
    },
    FilePicker {
        key: &'static str,
        label: &'static str,
        description: &'static str,
        prompt: &'static str,
    },
    DirPicker {
        key: &'static str,
        label: &'static str,
        description: &'static str,
        prompt: &'static str,
    },
}

impl Setting {
    /// Builds the row this setting draws. Takes `&App` to show legacy project overrides.
    pub fn into_item(self, cx: &App) -> SettingItem {
        match self {
            Setting::Text {
                key,
                label,
                description,
            } => SettingItem::new(
                label,
                resettable(
                    key,
                    SettingField::input(
                        move |cx: &App| effective_text(key, cx),
                        move |val: SharedString, cx: &mut App| set_user_text(key, val, cx),
                    ),
                    None,
                ),
            )
            .description(described(description, key, cx))
            // gpui-component pins horizontal-layout inputs to a fixed `w_64`, which clips inside a
            // narrow settings pane; the stacked layout gives them `w_full` instead.
            .layout(Axis::Vertical),
            Setting::TextWithAction {
                key,
                label,
                description,
                on_change,
            } => SettingItem::new(
                label,
                resettable(
                    key,
                    SettingField::input(
                        move |cx: &App| effective_text(key, cx),
                        move |val: SharedString, cx: &mut App| {
                            set_user_text(key, val, cx);
                            on_change(cx);
                        },
                    ),
                    Some(on_change),
                ),
            )
            .description(described(description, key, cx))
            .layout(Axis::Vertical),

            Setting::Switch {
                key,
                label,
                description,
            } => SettingItem::new(
                label,
                resettable(
                    key,
                    SettingField::switch(
                        move |cx: &App| effective_bool(key, cx),
                        move |val: bool, cx: &mut App| set_user_bool(key, val, cx),
                    ),
                    None,
                ),
            )
            .description(described(description, key, cx)),

            Setting::Dropdown {
                key,
                label,
                description,
                options,
            } => {
                let opts: Vec<(SharedString, SharedString)> = options
                    .iter()
                    .map(|(k, v)| ((*k).into(), (*v).into()))
                    .collect();
                SettingItem::new(
                    label,
                    resettable(
                        key,
                        SettingField::dropdown(
                            opts,
                            move |cx: &App| effective_text(key, cx),
                            move |val: SharedString, cx: &mut App| set_user_text(key, val, cx),
                        ),
                        None,
                    ),
                )
                .description(described(description, key, cx))
            }

            Setting::DropdownWithAction {
                key,
                label,
                description,
                options,
                on_change,
            } => SettingItem::new(
                label,
                resettable(
                    key,
                    SettingField::dropdown(
                        options
                            .iter()
                            .map(|(k, v)| ((*k).into(), (*v).into()))
                            .collect(),
                        move |cx: &App| effective_text(key, cx),
                        move |val: SharedString, cx: &mut App| {
                            set_user_text(key, val, cx);
                            on_change(cx);
                        },
                    ),
                    Some(on_change),
                ),
            )
            .description(described(description, key, cx)),

            Setting::FilePicker {
                key,
                label,
                description,
                prompt,
            } => path_picker_item(
                key,
                label,
                description,
                prompt,
                Picks::Files,
                move |cx: &App| user_text(key, cx),
                move |val: SharedString, cx: &mut App| AppSettings::set_text(key, val, cx),
            ),

            Setting::DirPicker {
                key,
                label,
                description,
                prompt,
            } => path_picker_item(
                key,
                label,
                description,
                prompt,
                Picks::Directories,
                move |cx: &App| user_text(key, cx),
                move |val: SharedString, cx: &mut App| AppSettings::set_text(key, val, cx),
            ),
        }
    }
}

fn user_text(key: &'static str, cx: &App) -> SharedString {
    AppSettings::get(cx)
        .values
        .get(key)
        .map(|v| v.text())
        .unwrap_or_default()
}

/// Mark a legacy project override beside the effective value it currently supplies.
fn described(description: &'static str, key: &'static str, cx: &App) -> SharedString {
    match has_project_override(key, cx) {
        true => format!("{description}\n\nProject override; Reset uses the user default.").into(),
        false => description.into(),
    }
}

/// Let an existing project override be cleared without exposing a scope switcher.
fn resettable<T: 'static>(
    key: &'static str,
    field: SettingField<T>,
    on_change: Option<fn(&mut App)>,
) -> SettingField<T> {
    field.on_reset(
        move |cx: &App| has_project_override(key, cx),
        move |_window: &mut Window, cx: &mut App| {
            clear_project_override(key, cx);
            if let Some(on_change) = on_change {
                on_change(cx);
            }
        },
    )
}

/// A read-only path row with a Browse button. `read`/`write` are the only difference between the
/// user-wide pickers and a project-scoped one, so both go through here rather than through two
/// copies of the `InputState` handling.
/// What a path picker's Browse button opens. An enum rather than the `files: bool,
/// directories: bool` pair it replaces: every call site passed one `true` and one `false`, and
/// `true, false` in argument position named neither of them.
#[derive(Clone, Copy)]
pub enum Picks {
    Files,
    Directories,
}

pub fn path_picker_item(
    key: &'static str,
    label: &'static str,
    description: &'static str,
    prompt: &'static str,
    picks: Picks,
    read: impl Fn(&App) -> SharedString + 'static,
    write: impl Fn(SharedString, &mut App) + Send + Sync + 'static,
) -> SettingItem {
    let (files, directories) = match picks {
        Picks::Files => (true, false),
        Picks::Directories => (false, true),
    };
    let prompt: SharedString = prompt.into();
    let write = Arc::new(write);
    SettingItem::new(
        label,
        SettingField::render(move |options, window, cx| {
            let want = read(cx);
            let input = window.use_keyed_state(
                SharedString::from(format!("path-picker-{key}")),
                cx,
                |window, cx| {
                    InputState::new(window, cx)
                        .placeholder("No file selected...")
                        .default_value(want.clone())
                },
            );
            input.update(cx, |state, cx| {
                if state.value() != want {
                    state.set_value(want.to_string(), window, cx);
                }
            });
            let write = Arc::clone(&write);
            PathPickerApp {
                field_size: options.size(),
                button_size: Some(options.size()),
                button_id: SharedString::from(format!("browse-{key}")),
                files,
                directories,
                prompt: prompt.clone(),
                input,
                on_pick: Arc::new(move |val, cx| write(val, cx)),
            }
        }),
    )
    .description(description)
    .layout(Axis::Vertical)
}

// --- Setting Value ---

#[derive(Clone)]
pub enum Val {
    Text(SharedString),
    Bool(bool),
}

impl Val {
    pub fn text(&self) -> SharedString {
        match self {
            Val::Text(s) => s.clone(),
            Val::Bool(b) => if *b { "true" } else { "false" }.into(),
        }
    }

    pub fn bool(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Text(s) => s == "true",
        }
    }
}

/// Last main window size and display, for restore on launch (position is not persisted).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MainWindowBounds {
    pub width: f32,
    pub height: f32,
    /// `PlatformDisplay::id` as `u32`; matched at startup against [`App::displays`].
    #[serde(default)]
    pub display_id: Option<u64>,
}

impl MainWindowBounds {
    pub fn capture_from_window(window: &Window, cx: &App) -> Self {
        let b = window.bounds();
        Self {
            width: b.size.width.into(),
            height: b.size.height.into(),
            display_id: window.display(cx).map(|d| u64::from(d.id())),
        }
    }

    /// Resolves target bounds/display for opening the main window: centered on the
    /// remembered display (or primary) at the remembered size, falling back to a sane
    /// default when `bounds` is missing or invalid. Position is not restored. Free
    /// function (not tied to `AppSettings`) so callers can pass either the global
    /// bounds or a per-project one read from a `.qrate` file.
    pub fn startup_placement(
        bounds: Option<&Self>,
        cx: &App,
    ) -> (Bounds<Pixels>, Option<DisplayId>) {
        let display = bounds.and_then(|b| b.display_id).and_then(|raw| {
            cx.displays()
                .into_iter()
                .find(|d| u64::from(d.id()) == raw)
                .map(|d| d.id())
        });
        const MIN_W: f32 = 400.0;
        const MIN_H: f32 = 250.0;
        const DEFAULT_W: f32 = 600.0;
        const DEFAULT_H: f32 = 800.0;
        if let Some(b) = bounds
            && b.width.is_finite()
            && b.height.is_finite()
            && b.width >= MIN_W
            && b.height >= MIN_H
        {
            let bounds = Bounds::centered(display, size(px(b.width), px(b.height)), cx);
            return (bounds, display);
        }
        let bounds = Bounds::centered(display, size(px(DEFAULT_W), px(DEFAULT_H)), cx);
        (bounds, display)
    }
}

// --- App Settings ---

pub struct AppSettings {
    pub values: HashMap<String, Val>,
    pub main_window_bounds: Option<MainWindowBounds>,
    /// Schema version last read from disk (`settings_version` in JSON). See [`SETTINGS_SCHEMA_VERSION`].
    #[allow(dead_code)]
    pub settings_schema_version: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            main_window_bounds: None,
            settings_schema_version: SETTINGS_SCHEMA_VERSION,
        }
    }
}

impl Global for AppSettings {}

#[derive(Clone, Default)]
pub struct SettingsPersistence {
    pub writer: Option<SettingsWriter>,
}

impl Global for SettingsPersistence {}

impl AppSettings {
    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Single mutation entrypoint so we can trigger persistence.
    pub fn update<R>(cx: &mut App, f: impl FnOnce(&mut Self) -> R) -> R {
        let r = {
            let s = cx.global_mut::<Self>();
            f(s)
        };
        if let Some(writer) = cx.global::<SettingsPersistence>().writer.clone() {
            let snapshot = cx.global::<Self>();
            writer.enqueue_save(snapshot);
        }
        r
    }

    pub fn set_text(key: &'static str, val: SharedString, cx: &mut App) {
        if key != SETTINGS_WINDOW_BOUNDS_KEY {
            log::debug!("settings: app text changed key={key}");
        }
        Self::update(cx, |s| {
            s.values.insert(key.into(), Val::Text(val));
        });
    }

    pub fn set_bool(key: &'static str, val: bool, cx: &mut App) {
        log::debug!("settings: app bool changed key={key} value={val}");
        Self::update(cx, |s| {
            s.values.insert(key.into(), Val::Bool(val));
        });
    }
}

// --- Settings Window ---

pub struct SettingsWindow {
    /// Takes `&App` so a page can build itself from live state — the Columns page lists the open
    /// project's columns, which a context-free builder can't see. Re-invoked every render.
    pub build_pages: fn(&App) -> Vec<SettingPage>,
    /// Page to open on, as an index into [`Self::build_pages`]. Only the first render reads it —
    /// the widget owns which page is selected after that, so a menu item that names a page can't
    /// yank it away mid-session.
    initial_page: Option<usize>,
    /// Persists the window's size (debounced) so it reopens where it was left.
    _bounds_sub: Subscription,
    /// Re-render when any setting changes so dynamic pages rebuild live.
    _settings_sub: Subscription,
    _project_sub: Subscription,
}

impl SettingsWindow {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        build_pages: fn(&App) -> Vec<SettingPage>,
        initial_page: Option<usize>,
    ) -> Self {
        window.set_window_title("Settings — qrate");
        let _bounds_sub = cx.observe_window_bounds(window, |_this, window, cx| {
            let bounds = MainWindowBounds::capture_from_window(window, cx);
            if let Ok(json) = serde_json::to_string(&bounds) {
                AppSettings::set_text(SETTINGS_WINDOW_BOUNDS_KEY, json.into(), cx);
            }
        });
        let _settings_sub = cx.observe_global::<AppSettings>(|_this, cx| cx.notify());
        let _project_sub = cx.observe_global::<project::CurrentProject>(|_this, cx| cx.notify());
        Self {
            build_pages,
            initial_page,
            _bounds_sub,
            _settings_sub,
            _project_sub,
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                TitleBar::new()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .child(Label::new("Settings").font_semibold()),
            )
            .child(div().flex_1().min_h_0().child({
                let pages = (self.build_pages)(cx);
                let page_ix = self
                    .initial_page
                    .filter(|ix| *ix < pages.len())
                    .unwrap_or_default();
                Settings::new("app-settings")
                    .default_selected_index(SelectIndex {
                        page_ix,
                        group_ix: None,
                    })
                    .pages(pages)
            }))
    }
}

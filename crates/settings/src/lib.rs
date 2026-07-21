//! Persisted preferences, reusable setting field builders, path picker widgets, and a generic
//! settings-window shell (`SettingsWindow`). Product-specific pages live in `app`.

pub mod columns;
pub mod dirty;
pub mod os_open;
pub mod path_picker;
pub mod project;

mod db;

pub use db::{SettingsWriter, flush_app_settings, load_app_settings};
/// Increment when the persisted SQLite JSON schema (`db::PersistSettings`) changes.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

use std::collections::HashMap;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme as _, StyledExt, TitleBar, h_flex,
    input::InputState,
    label::Label,
    setting::{SettingField, SettingItem, SettingPage, Settings},
    v_flex,
};
use serde::{Deserialize, Serialize};

use crate::path_picker::PathPickerApp;

/// `AppSettings` value key for the Settings window's last size (a JSON [`MainWindowBounds`]).
pub const SETTINGS_WINDOW_BOUNDS_KEY: &str = "settings_window_bounds";

/// Setting key (either scope) for autosave behavior: `"timed"` (buffered, the default), `"immediate"`,
/// or `"off"`. Read by the table crate to decide when a committed cell edit reaches disk.
pub const AUTOSAVE_KEY: &str = "autosave";

// --- Settings Scope ---

/// Which store a settings field reads and writes. The same fields render in both scopes; only
/// the backing store differs — `User` is the app-wide `AppSettings`, `Project` is the open
/// project's `.qrate` file.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsScope {
    #[default]
    User,
    Project,
}

/// The scope the Settings window's fields currently target. Unset means [`SettingsScope::User`].
#[derive(Clone, Copy, Default)]
pub struct CurrentSettingsScope(pub SettingsScope);

impl Global for CurrentSettingsScope {}

impl SettingsScope {
    pub fn current(cx: &App) -> Self {
        cx.try_global::<CurrentSettingsScope>()
            .map(|s| s.0)
            .unwrap_or_default()
    }
}

/// Reads a bool from whichever scope the Settings window currently targets.
fn scoped_bool(key: &str, cx: &App) -> bool {
    match SettingsScope::current(cx) {
        SettingsScope::User => AppSettings::get(cx)
            .values
            .get(key)
            .map(|v| v.bool())
            .unwrap_or(false),
        SettingsScope::Project => cx
            .try_global::<project::CurrentProject>()
            .map(|p| p.get_bool(key))
            .unwrap_or(false),
    }
}

/// Writes a bool to whichever scope the Settings window currently targets. Falls back to the
/// user scope when `Project` is active but no project is open, so a stale scope can't panic.
fn set_scoped_bool(key: &'static str, val: bool, cx: &mut App) {
    let target = match SettingsScope::current(cx) {
        SettingsScope::Project if cx.has_global::<project::CurrentProject>() => {
            SettingsScope::Project
        }
        _ => SettingsScope::User,
    };
    match target {
        SettingsScope::User => AppSettings::set_bool(key, val, cx),
        SettingsScope::Project => project::CurrentProject::set_bool(key, val, cx),
    }
}

pub fn scoped_text(key: &str, cx: &App) -> SharedString {
    match SettingsScope::current(cx) {
        SettingsScope::User => AppSettings::get(cx)
            .values
            .get(key)
            .map(|v| v.text())
            .unwrap_or_default(),
        SettingsScope::Project => cx
            .try_global::<project::CurrentProject>()
            .and_then(|p| p.data.values.get(key).map(|v| v.text()))
            .unwrap_or_default(),
    }
}

pub fn set_scoped_text(key: &'static str, val: SharedString, cx: &mut App) {
    let target = match SettingsScope::current(cx) {
        SettingsScope::Project if cx.has_global::<project::CurrentProject>() => {
            SettingsScope::Project
        }
        _ => SettingsScope::User,
    };
    match target {
        SettingsScope::User => AppSettings::set_text(key, val, cx),
        SettingsScope::Project => project::CurrentProject::set_text(key, val, cx),
    }
}

/// Resolves a bool setting for a *consumer* (e.g. the table's stripe toggle): the open project's
/// value wins if present, else the user-wide default, else `false`. Unlike [`scoped_bool`], this
/// ignores the Settings window's active scope — it's what the feature should actually use.
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

impl From<Setting> for SettingItem {
    fn from(setting: Setting) -> Self {
        match setting {
            Setting::Text {
                key,
                label,
                description,
            } => SettingItem::new(
                label,
                SettingField::input(
                    move |cx: &App| scoped_text(key, cx),
                    move |val: SharedString, cx: &mut App| set_scoped_text(key, val, cx),
                ),
            )
            .description(description),

            Setting::Switch {
                key,
                label,
                description,
            } => SettingItem::new(
                label,
                SettingField::switch(
                    move |cx: &App| scoped_bool(key, cx),
                    move |val: bool, cx: &mut App| set_scoped_bool(key, val, cx),
                ),
            )
            .description(description),

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
                    SettingField::dropdown(
                        opts,
                        move |cx: &App| scoped_text(key, cx),
                        move |val: SharedString, cx: &mut App| set_scoped_text(key, val, cx),
                    ),
                )
                .description(description)
            }

            Setting::FilePicker {
                key,
                label,
                description,
                prompt,
            } => build_path_picker(key, label, description, prompt, true, false),

            Setting::DirPicker {
                key,
                label,
                description,
                prompt,
            } => build_path_picker(key, label, description, prompt, false, true),
        }
    }
}

fn build_path_picker(
    key: &'static str,
    label: &'static str,
    description: &'static str,
    prompt: &'static str,
    files: bool,
    directories: bool,
) -> SettingItem {
    let prompt: SharedString = prompt.into();
    SettingItem::new(
        label,
        SettingField::render(move |options, window, cx| {
            let want = AppSettings::get(cx)
                .values
                .get(key)
                .map(|v| v.text())
                .unwrap_or_default();
            let input = window.use_keyed_state(
                SharedString::from(format!(
                    "path-picker-{}-{}-{}",
                    options.page_ix, options.group_ix, options.item_ix
                )),
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
            PathPickerApp {
                layout: options.layout,
                field_size: options.size,
                button_size: Some(options.size),
                button_id: SharedString::from(format!("browse-{}", key)),
                files,
                directories,
                prompt: prompt.clone(),
                input,
                on_pick: Arc::new(move |val, cx| {
                    AppSettings::set_text(key, val, cx);
                }),
            }
        }),
    )
    .description(description)
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
    pub fn get_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }

    /// Window size and target display for startup, from the global (non-project) bounds.
    /// See [`MainWindowBounds::startup_placement`] for the per-project equivalent.
    pub fn main_window_startup_placement(&self, cx: &App) -> (Bounds<Pixels>, Option<DisplayId>) {
        MainWindowBounds::startup_placement(self.main_window_bounds.as_ref(), cx)
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
        Self::update(cx, |s| {
            s.values.insert(key.into(), Val::Text(val));
        });
    }

    pub fn set_bool(key: &'static str, val: bool, cx: &mut App) {
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
    /// Persists the window's size (debounced) so it reopens where it was left.
    _bounds_sub: Subscription,
    /// Re-render when any setting changes so scope-dependent pages (autosave's method row, the
    /// columns filter picker) rebuild live instead of on the next unrelated repaint.
    _settings_sub: Subscription,
    _project_sub: Subscription,
}

impl SettingsWindow {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        build_pages: fn(&App) -> Vec<SettingPage>,
    ) -> Self {
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
            _bounds_sub,
            _settings_sub,
            _project_sub,
        }
    }
}

/// A small text-only scope tab (Zed-style): accented when selected, muted otherwise.
fn scope_tab(
    id: &'static str,
    label: &'static str,
    selected: bool,
    enabled: bool,
    target: SettingsScope,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let color = if selected {
        cx.theme().foreground
    } else {
        cx.theme().muted_foreground
    };
    let base = div()
        .id(id)
        .text_sm()
        .text_color(color)
        .when(selected, |d| d.font_semibold())
        .child(label);
    if enabled {
        base.cursor_pointer()
            .on_click(cx.listener(move |_this, _ev, _window, cx| {
                cx.set_global(CurrentSettingsScope(target));
                cx.notify();
            }))
    } else {
        base
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_project = cx.has_global::<project::CurrentProject>();
        // A stale `Project` scope (project closed while it was selected) snaps back to User so
        // fields don't target a nonexistent store.
        let scope = if has_project {
            SettingsScope::current(cx)
        } else {
            SettingsScope::User
        };

        v_flex()
            .size_full()
            .child(TitleBar::new().child(Label::new("Settings").font_semibold()))
            // Fixed scope switcher, right-aligned over the settings content section.
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap_4()
                    .px_4()
                    .py_2()
                    .child(scope_tab(
                        "scope-user",
                        "User",
                        scope == SettingsScope::User,
                        true,
                        SettingsScope::User,
                        cx,
                    ))
                    .child(scope_tab(
                        "scope-project",
                        "Project",
                        scope == SettingsScope::Project,
                        has_project,
                        SettingsScope::Project,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(Settings::new("app-settings").pages((self.build_pages)(cx))),
            )
    }
}

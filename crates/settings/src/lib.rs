//! Persisted preferences, reusable setting field builders, path picker widgets, and a generic
//! settings-window shell (`SettingsWindow`). Product-specific pages live in `app`.

pub mod path_picker;

mod db;

pub use db::{SettingsWriter, load_app_settings};
use gpui_component::Sizable;

/// Increment when the persisted SQLite JSON schema (`db::PersistSettings`) changes.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::HashMap, env};

use gpui::*;
use gpui_component::{
    IconName, StyledExt, TitleBar,
    button::Button,
    h_flex,
    input::InputState,
    label::Label,
    scroll::ScrollableElement,
    setting::{SettingField, SettingItem, SettingPage, Settings},
    v_flex,
};
use serde::{Deserialize, Serialize};

use crate::path_picker::PathPickerApp;

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
                    move |cx: &App| {
                        AppSettings::get(cx)
                            .values
                            .get(key)
                            .map(|v| v.text())
                            .unwrap_or_default()
                    },
                    move |val: SharedString, cx: &mut App| {
                        AppSettings::set_text(key, val, cx);
                    },
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
                    move |cx: &App| {
                        AppSettings::get(cx)
                            .values
                            .get(key)
                            .map(|v| v.bool())
                            .unwrap_or(false)
                    },
                    move |val: bool, cx: &mut App| {
                        AppSettings::set_bool(key, val, cx);
                    },
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
                        move |cx: &App| {
                            AppSettings::get(cx)
                                .values
                                .get(key)
                                .map(|v| v.text())
                                .unwrap_or_default()
                        },
                        move |val: SharedString, cx: &mut App| {
                            AppSettings::set_text(key, val, cx);
                        },
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
    pub display_id: Option<u32>,
}

impl MainWindowBounds {
    pub fn capture_from_window(window: &Window, cx: &App) -> Self {
        let b = window.bounds();
        Self {
            width: b.size.width.into(),
            height: b.size.height.into(),
            display_id: window.display(cx).map(|d| u32::from(d.id())),
        }
    }
}

// --- App Settings ---

pub fn picker_with_path_button(
    key: &'static str,
    label: &'static str,
    description: &'static str,
    prompt: &'static str,
    path_candidates: Vec<&'static str>,
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
                    "path-picker-pathbtn-{}-{}-{}",
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

            let on_pick_key = key;
            let on_path_key = key;
            let path_candidates = path_candidates.clone();

            h_flex()
                .gap_2()
                .w_full()
                .child(PathPickerApp {
                    layout: options.layout,
                    field_size: options.size,
                    button_size: Some(options.size),
                    button_id: SharedString::from(format!("browse-{}", key)),
                    files: true,
                    directories: false,
                    prompt: prompt.clone(),
                    input: input.clone(),
                    on_pick: std::sync::Arc::new(move |val, cx| {
                        AppSettings::set_text(on_pick_key, val, cx);
                    }),
                })
                .child(
                    Button::new(SharedString::from(format!("get-from-path-{}", key)))
                        .outline()
                        .icon(IconName::Redo2)
                        .tooltip("Get from PATH")
                        .with_size(options.size)
                        .on_click(move |_, _, cx| {
                            if let Some(p) = find_on_path(&path_candidates) {
                                AppSettings::set_text(
                                    on_path_key,
                                    p.to_string_lossy().to_string().into(),
                                    cx,
                                );
                            }
                        }),
                )
        }),
    )
    .description(description)
}

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

    fn main_window_resolved_display(&self, cx: &App) -> Option<DisplayId> {
        self.main_window_bounds
            .as_ref()
            .and_then(|b| b.display_id)
            .and_then(|raw| {
                cx.displays()
                    .into_iter()
                    .find(|d| u32::from(d.id()) == raw)
                    .map(|d| d.id())
            })
    }

    /// Window size and target display for startup. The frame is always **centered** on the
    /// remembered display (or primary); window **position is not restored** from disk.
    /// Invalid/missing size falls back to 600×800 centered the same way.
    pub fn main_window_startup_placement(&self, cx: &App) -> (Bounds<Pixels>, Option<DisplayId>) {
        let display = self.main_window_resolved_display(cx);
        const MIN_W: f32 = 400.0;
        const MIN_H: f32 = 250.0;
        const DEFAULT_W: f32 = 600.0;
        const DEFAULT_H: f32 = 800.0;
        if let Some(b) = &self.main_window_bounds
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
    pub build_pages: fn() -> Vec<SettingPage>,
}

impl SettingsWindow {
    pub fn new(
        _window: &mut Window,
        _cx: &mut Context<Self>,
        build_pages: fn() -> Vec<SettingPage>,
    ) -> Self {
        Self { build_pages }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(TitleBar::new().child(Label::new("Settings").font_semibold()))
            .child(
                div()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .child(Settings::new("app-settings").pages((self.build_pages)())),
            )
    }
}

pub fn find_on_path(candidates: &[&str]) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for name in candidates {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

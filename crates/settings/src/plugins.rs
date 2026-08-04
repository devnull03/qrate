//! Untyped per-plugin settings, in the two scopes that are not per-column.
//!
//! Opaque by design: one JSON object per plugin id, which the host stores and never reads into.
//! That is what lets a plugin add a setting without a Rust change — the alternative is a typed
//! struct here, which would mean every third-party knob needs a qrate release.
//!
//! Column scope lives in [`crate::columns::ColumnSettings::plugins`] instead, because it is keyed
//! by column and shares that blob's lifecycle.

use std::collections::BTreeMap;

use gpui::App;
use serde_json::Value as Json;

use crate::project::CurrentProject;
use crate::{AppSettings, dirty};

/// The key both scopes store their map under — user-wide in `AppSettings`, per-project in the
/// `.qrate` file's `__settings`.
pub const PLUGIN_SETTINGS_KEY: &str = "plugin_settings";

/// plugin id → that plugin's object. `BTreeMap` so the serialized JSON is diff-friendly.
pub type PluginSettingsMap = BTreeMap<String, Json>;

/// A malformed blob degrades to "nothing stored" rather than propagating — a corrupt plugin
/// setting must not stop a project opening.
fn parse(raw: Option<&str>) -> PluginSettingsMap {
    raw.and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

fn stored_user(cx: &App) -> PluginSettingsMap {
    let raw = cx
        .try_global::<AppSettings>()
        .and_then(|s| s.values.get(PLUGIN_SETTINGS_KEY))
        .map(|v| v.text());
    parse(raw.as_deref())
}

fn stored_project(cx: &App) -> PluginSettingsMap {
    let raw = cx
        .try_global::<CurrentProject>()
        .and_then(|p| p.data.values.get(PLUGIN_SETTINGS_KEY))
        .map(|v| v.text());
    parse(raw.as_deref())
}

/// One plugin's user-wide settings, [`Json::Null`] if it has never stored any.
pub fn user(plugin: &str, cx: &App) -> Json {
    stored_user(cx).remove(plugin).unwrap_or(Json::Null)
}

pub fn set_user(plugin: &str, value: Json, cx: &mut App) {
    let mut map = stored_user(cx);
    map.insert(plugin.to_string(), value);
    let Ok(json) = serde_json::to_string(&map) else {
        return;
    };
    AppSettings::set_text(PLUGIN_SETTINGS_KEY, json.into(), cx);
}

/// One plugin's settings for the open project, [`Json::Null`] with no project or nothing stored.
pub fn project(plugin: &str, cx: &App) -> Json {
    stored_project(cx).remove(plugin).unwrap_or(Json::Null)
}

pub fn set_project(plugin: &str, value: Json, cx: &mut App) {
    if !cx.has_global::<CurrentProject>() {
        return;
    }
    let mut map = stored_project(cx);
    map.insert(plugin.to_string(), value);
    let Ok(json) = serde_json::to_string(&map) else {
        return;
    };
    CurrentProject::set_text(PLUGIN_SETTINGS_KEY, json.into(), cx);
    dirty::mark(dirty::PLUGIN_SETTINGS, cx);
}

/// Whether a stored value counts as "this plugin has settings here" — the condition contributed
/// menu entries gate on. Lua cannot tell an empty table from an empty list, so both read as
/// nothing stored.
pub fn is_set(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Object(map) => !map.is_empty(),
        Json::Array(items) => !items.is_empty(),
        _ => true,
    }
}

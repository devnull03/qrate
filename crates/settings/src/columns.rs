//! Per-column preferences for the data table, stored per project.
//!
//! Columns belong to a project, so these are project-scoped only — there is no user-scope
//! fallback and no `SettingsScope` involvement. Values live as one JSON blob in the `.qrate`
//! file's `__settings` table (the same shape `table_columns` uses), read back through
//! `CurrentProject`'s in-memory cache so nothing here touches disk during a render.
//!
//! Keyed by the table's stable column key (`c{ix}`, the column's index into the on-disk header
//! order). Stored as a *map*, deliberately unlike the positional `ColumnLayout`: that one drops
//! the entire layout when the key set stops matching, which is fine for widths and wrong for
//! preferences. A map loses only the columns that actually went away.

use std::collections::BTreeMap;

use gpui::App;
use serde::{Deserialize, Serialize};

use crate::dirty;
use crate::project::CurrentProject;

/// `__settings` key holding the JSON map below.
pub const COLUMN_SETTINGS_KEY: &str = "table_column_settings";

/// A column's preferences. A column only has an entry once the user adds it on the Columns
/// settings page; absent means "not configured", which reads as all-defaults.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq, Debug)]
pub struct ColumnSettings {
    /// Whether this column offers the filter dropdown in its header.
    #[serde(default)]
    pub filter_enabled: bool,
}

/// `c{ix}` → settings. `BTreeMap` so the settings page lists columns in a stable order and the
/// serialized JSON is diff-friendly.
pub type ColumnSettingsMap = BTreeMap<String, ColumnSettings>;

/// Parse the stored blob. A malformed or absent value degrades to "nothing configured" rather
/// than propagating an error — a corrupt preference must not stop the table loading.
fn parse(raw: Option<&str>) -> ColumnSettingsMap {
    raw.and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

/// Every configured column's settings for the open project. Empty when no project is open.
pub fn load(cx: &App) -> ColumnSettingsMap {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return ColumnSettingsMap::new();
    };
    parse(
        project
            .data
            .values
            .get(COLUMN_SETTINGS_KEY)
            .map(|v| v.text())
            .as_deref(),
    )
}

/// The column keys the user has added to the settings page, in `c{ix}` order.
pub fn tracked(cx: &App) -> Vec<String> {
    load(cx).into_keys().collect()
}

/// One column's settings, all-defaults if it was never added.
pub fn get(col_key: &str, cx: &App) -> ColumnSettings {
    load(cx).get(col_key).cloned().unwrap_or_default()
}

/// Whether a column has been added to the settings page.
pub fn is_tracked(col_key: &str, cx: &App) -> bool {
    load(cx).contains_key(col_key)
}

/// Persist the whole map, updating `CurrentProject`'s cache and queueing a debounced write.
/// No-op without an open project.
fn store(map: &ColumnSettingsMap, cx: &mut App) {
    if !cx.has_global::<CurrentProject>() {
        return;
    }
    let Ok(json) = serde_json::to_string(map) else {
        return;
    };
    CurrentProject::set_text(COLUMN_SETTINGS_KEY, json.into(), cx);
    dirty::mark(dirty::COLUMN_SETTINGS, cx);
}

/// Add a column to the settings page, unlocking its settings at their defaults. Adding one that
/// is already there leaves it untouched.
pub fn add(col_key: &str, cx: &mut App) {
    let mut map = load(cx);
    if map.contains_key(col_key) {
        return;
    }
    map.insert(col_key.to_string(), ColumnSettings::default());
    store(&map, cx);
}

/// Drop a column from the settings page, discarding its settings.
pub fn remove(col_key: &str, cx: &mut App) {
    let mut map = load(cx);
    if map.remove(col_key).is_none() {
        return;
    }
    store(&map, cx);
}

/// Mutate one column's settings. Adds the entry first if it is missing, so a toggle can't
/// silently no-op.
pub fn update(col_key: &str, f: impl FnOnce(&mut ColumnSettings), cx: &mut App) {
    let mut map = load(cx);
    f(map.entry(col_key.to_string()).or_default());
    store(&map, cx);
}

#[cfg(test)]
mod tests {
    use super::{ColumnSettings, ColumnSettingsMap, parse};

    fn map_with(key: &str, filter_enabled: bool) -> ColumnSettingsMap {
        let mut m = ColumnSettingsMap::new();
        m.insert(key.to_string(), ColumnSettings { filter_enabled });
        m
    }

    #[test]
    fn round_trips_through_json() {
        let map = map_with("c3", true);
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(parse(Some(&json)), map);
    }

    #[test]
    fn absent_or_malformed_reads_as_empty() {
        assert!(parse(None).is_empty());
        assert!(parse(Some("")).is_empty());
        assert!(parse(Some("{ not json")).is_empty());
        assert!(parse(Some("[1,2,3]")).is_empty());
    }

    #[test]
    fn an_unknown_column_reads_as_defaults() {
        let map = map_with("c0", true);
        assert_eq!(
            map.get("c9").cloned().unwrap_or_default(),
            ColumnSettings::default()
        );
        assert!(!ColumnSettings::default().filter_enabled);
    }

    /// The reason this is a map and not `ColumnLayout`'s positional vec: a column disappearing
    /// must cost only that column's prefs, not everyone's.
    #[test]
    fn a_dropped_column_does_not_take_the_others_with_it() {
        let mut map = map_with("c0", true);
        map.insert(
            "c1".into(),
            ColumnSettings {
                filter_enabled: true,
            },
        );
        map.remove("c0");
        assert_eq!(map.len(), 1);
        assert!(map["c1"].filter_enabled);
    }

    /// Forward-compatibility: a blob written by a future build with more fields must still load.
    #[test]
    fn unknown_fields_are_ignored() {
        let parsed = parse(Some(r#"{"c2":{"filter_enabled":true,"future_thing":42}}"#));
        assert!(parsed["c2"].filter_enabled);
    }
}

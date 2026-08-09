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

/// What a column's values are shaped like, as declared in `__columns.data_type`.
///
/// The stored value stays free-form `TEXT` — this only *interprets* it, so a project written by
/// hand or by an older build still loads and a type nobody here knows reads as [`Self::Text`]
/// rather than becoming an error. Validators key off this instead of matching type strings
/// themselves, which is what stops each one growing its own spelling list.
///
/// What a column is *checked against* is a separate question, and lives in
/// [`ColumnSettings::authority`]: a subject heading is `Text` that also has to exist in LCSH.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ColumnType {
    #[default]
    Text,
    /// A date, validated as EDTF.
    Date,
    /// Names a file in the project's files folder.
    Filename,
    Number,
    Url,
    /// An accession number, call number, or other opaque handle.
    Identifier,
}

impl ColumnType {
    /// Canonical spelling — what the wizard writes and `column_config.csv` should say.
    pub fn as_str(self) -> &'static str {
        match self {
            ColumnType::Text => "Text",
            ColumnType::Date => "Date",
            ColumnType::Filename => "Filename",
            ColumnType::Number => "Number",
            ColumnType::Url => "Url",
            ColumnType::Identifier => "Identifier",
        }
    }

    /// Every type, in the order the wizard offers them.
    pub const ALL: [ColumnType; 6] = [
        ColumnType::Text,
        ColumnType::Date,
        ColumnType::Filename,
        ColumnType::Number,
        ColumnType::Url,
        ColumnType::Identifier,
    ];

    /// Read a declared type. Case- and space-insensitive, and it accepts the spellings a person
    /// writing a CSV by hand actually uses (`int`, `datetime`, `uri`) so a config isn't rejected
    /// over a synonym. Anything unrecognised is [`Self::Text`], the type that assumes least.
    pub fn from_declared(declared: &str) -> Self {
        match declared.trim().to_ascii_lowercase().as_str() {
            "date" | "datetime" | "time" | "year" | "edtf" => ColumnType::Date,
            "filename" | "file" | "filepath" | "path" => ColumnType::Filename,
            "number" | "integer" | "int" | "float" | "decimal" => ColumnType::Number,
            "url" | "uri" | "link" => ColumnType::Url,
            "identifier" | "id" => ColumnType::Identifier,
            _ => ColumnType::Text,
        }
    }

    /// Whether cells hold prose — the question spell checking and any other language-aware rule
    /// asks. Everything that is not [`Self::Text`] is a code, a number, or a machine handle.
    pub fn is_prose(self) -> bool {
        self == ColumnType::Text
    }
}

/// `__settings` key holding the JSON map below.
pub const COLUMN_SETTINGS_KEY: &str = "table_column_settings";

/// `__settings` key for the master "column filters" switch — the kill-switch gating every
/// column's filter dropdown. Absent means on, so opening a project that already selected columns
/// shows their filters without first flipping a toggle.
pub const COLUMN_FILTERS_ENABLED_KEY: &str = "table_column_filters_enabled";

/// A column's preferences. A column only has an entry once the user adds it on the Columns
/// settings page; absent means "not configured", which reads as all-defaults.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ColumnSettings {
    /// Whether this column offers the filter dropdown in its header.
    #[serde(default)]
    pub filter_enabled: bool,
    /// Whether cells in this column are spell-checked. Unlike every other field here it defaults
    /// *on*, so the feature works on a project nobody has configured — which is why this struct
    /// can't derive `Default`.
    #[serde(default = "yes")]
    pub spellcheck: bool,
    /// Which authority file this column's values must exist in, by name (`"LCSH"`), or `None` to
    /// check nothing. Separate from the column's [`ColumnType`] because the two answer different
    /// questions: the type is the shape a value has, this is the list it has to appear on.
    #[serde(default)]
    pub authority: Option<String>,
    /// Plugin id → whatever that plugin keeps for this column. Opaque here on purpose: a typed
    /// field would mean every third-party knob needs a qrate release. See [`crate::plugins`] for
    /// the two scopes that are not per-column.
    #[serde(default)]
    pub plugins: BTreeMap<String, serde_json::Value>,
    /// Producer name → the severity its findings take on this column, overriding what the check
    /// itself reported. Absent means "as reported". Held as the string spellings
    /// `diagnostics::Severity::key` uses rather than the enum, because that crate depends on this
    /// one and not the other way round.
    #[serde(default)]
    pub severity: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

impl Default for ColumnSettings {
    fn default() -> Self {
        Self {
            filter_enabled: false,
            spellcheck: true,
            authority: None,
            plugins: BTreeMap::new(),
            severity: BTreeMap::new(),
        }
    }
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

/// One column's settings, all-defaults if it was never added.
pub fn get(col_key: &str, cx: &App) -> ColumnSettings {
    load(cx).get(col_key).cloned().unwrap_or_default()
}

/// The master filter switch (default on — see [`COLUMN_FILTERS_ENABLED_KEY`]).
pub fn filters_master_enabled(cx: &App) -> bool {
    cx.try_global::<CurrentProject>()
        .and_then(|p| {
            p.data
                .values
                .get(COLUMN_FILTERS_ENABLED_KEY)
                .map(|v| v.bool())
        })
        .unwrap_or(true)
}

/// Flip the master filter switch. No-op without an open project.
pub fn set_filters_master_enabled(on: bool, cx: &mut App) {
    if !cx.has_global::<CurrentProject>() {
        return;
    }
    CurrentProject::set_bool(COLUMN_FILTERS_ENABLED_KEY, on, cx);
    dirty::mark(dirty::COLUMN_SETTINGS, cx);
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

/// Mutate one column's settings. Adds the entry first if it is missing, so a toggle can't
/// silently no-op.
pub fn update(col_key: &str, f: impl FnOnce(&mut ColumnSettings), cx: &mut App) {
    let mut map = load(cx);
    f(map.entry(col_key.to_string()).or_default());
    store(&map, cx);
}

#[cfg(test)]
mod tests {
    use super::{ColumnSettings, ColumnSettingsMap, ColumnType, parse};

    #[test]
    fn every_type_round_trips_through_its_canonical_spelling() {
        for ty in ColumnType::ALL {
            assert_eq!(ColumnType::from_declared(ty.as_str()), ty);
        }
    }

    /// The spellings people actually write in a `column_config.csv`.
    #[test]
    fn synonyms_and_casing_read_as_the_canonical_type() {
        assert_eq!(ColumnType::from_declared("  DATETIME "), ColumnType::Date);
        assert_eq!(ColumnType::from_declared("int"), ColumnType::Number);
        assert_eq!(ColumnType::from_declared("uri"), ColumnType::Url);
        assert_eq!(ColumnType::from_declared("ID"), ColumnType::Identifier);
        assert_eq!(ColumnType::from_declared("file"), ColumnType::Filename);
    }

    /// A type this build has never heard of must not stop the column loading — and "assumes
    /// least" means prose, which is the only setting that turns checks *on*.
    #[test]
    fn an_unknown_or_missing_type_is_text() {
        assert_eq!(ColumnType::from_declared("Coordinates"), ColumnType::Text);
        assert_eq!(ColumnType::from_declared(""), ColumnType::Text);
        assert!(ColumnType::default().is_prose());
        assert!(!ColumnType::Date.is_prose());
    }

    /// The authority is a separate axis from the type: a subject heading is prose *and* checked.
    #[test]
    fn an_authority_round_trips_and_defaults_to_none() {
        assert!(ColumnSettings::default().authority.is_none());
        let mut map = ColumnSettingsMap::new();
        map.insert(
            "c1".into(),
            ColumnSettings {
                authority: Some("LCSH".into()),
                ..Default::default()
            },
        );
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(parse(Some(&json)), map);
    }

    fn map_with(key: &str, filter_enabled: bool) -> ColumnSettingsMap {
        let mut m = ColumnSettingsMap::new();
        m.insert(
            key.to_string(),
            ColumnSettings {
                filter_enabled,
                ..Default::default()
            },
        );
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
                ..Default::default()
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

    /// The other direction, which is what the plugin bucket relies on: a blob written before the
    /// field existed still loads, and reads as "no plugin has stored anything here".
    #[test]
    fn a_blob_without_a_plugin_bucket_reads_as_empty() {
        let parsed = parse(Some(r#"{"c2":{"filter_enabled":true}}"#));
        assert!(parsed["c2"].plugins.is_empty());
    }

    /// One column can override two producers differently — the map is keyed by producer for
    /// exactly that reason.
    #[test]
    fn a_severity_override_round_trips_per_producer() {
        let mut map = map_with("c2", false);
        let overrides = &mut map.get_mut("c2").unwrap().severity;
        overrides.insert("LCSH".into(), "warning".into());
        overrides.insert("islandora".into(), "error".into());
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(parse(Some(&json)), map);
    }

    /// A blob written before the field existed reads as "nothing overridden", which is what keeps
    /// every check reporting its own severity on an untouched project.
    #[test]
    fn a_blob_without_severity_overrides_nothing() {
        let parsed = parse(Some(r#"{"c2":{"filter_enabled":true}}"#));
        assert!(parsed["c2"].severity.is_empty());
        assert!(ColumnSettings::default().severity.is_empty());
    }

    /// A plugin's own shape is never declared to Rust, so whatever it stores has to survive the
    /// round trip untouched — that is the entire promise the bucket makes.
    #[test]
    fn a_plugins_bucket_round_trips_whatever_it_holds() {
        let mut map = map_with("c2", true);
        map.get_mut("c2").unwrap().plugins.insert(
            "vocab".into(),
            serde_json::json!({ "allowed_values": ["Film", "Video"], "strict": true }),
        );
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(parse(Some(&json)), map);
    }
}

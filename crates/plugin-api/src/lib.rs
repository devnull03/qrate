//! What a plugin contributes to the app's own UI, as plain data.
//!
//! A leaf crate holding no logic, for one reason: `table` must be able to show a plugin's menu
//! entries without depending on the plugin host, which would link an embedded VM into `table`,
//! `workspace`, and `app`. So contributions cross as data in [`MenuContributions`] and the click
//! comes back through the function pointer in [`PluginHooks`] — the same inversion
//! `diagnostics::DiagnosticHooks` uses for revealing a cell.
//!
//! Nothing here runs plugin code. Menus are built synchronously while the user waits, so what a
//! menu needs to know has to already be sitting in a global by the time it is opened. Settings
//! ([`SettingSpec`]) do not need that inversion — the Settings window lives in `app`, which links
//! the host — but they live here so the two kinds of contribution stay described in one place.
//!
//! The one dependency beyond gpui is `settings`, which [`ColumnMapContributions`] reads and writes
//! so its two renderers — a Settings page and a right-click submenu, in different crates — cannot
//! drift apart about where a mapping is stored. Still no VM: stored JSON in, stored JSON out.

use gpui::{App, Global, SharedString};

/// Which right-click menu an entry belongs in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuTarget {
    Column,
    Cell,
    Row,
}

impl MenuTarget {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "column" => Some(Self::Column),
            "cell" => Some(Self::Cell),
            "row" => Some(Self::Row),
            _ => None,
        }
    }
}

/// One contributed entry.
#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: SharedString,
    pub target: MenuTarget,
    /// Passed back to the plugin verbatim when the entry is clicked.
    pub command: SharedString,
    /// Show the entry only when the plugin already has settings stored for what was clicked.
    /// This is the whole conditional vocabulary — a "Clear …" entry needs something to clear, and
    /// no contributed menu has yet needed a condition that is not that one.
    //
    // ponytail: one flag instead of VS Code's `when` expressions. If a second condition turns up,
    // add a second flag; only reach for an expression grammar at the third.
    pub requires_settings: bool,
}

/// Which bar an item sits in, and which end of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bar {
    Status,
    Title,
}

impl Bar {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "status" => Some(Self::Status),
            "title" => Some(Self::Title),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// What one mouse button on a bar item does. Left and right are declared independently, so an item
/// can act on click and still offer the rest of its commands on the other button.
#[derive(Clone, Debug)]
pub enum BarAction {
    Command(SharedString),
    /// `(label, command)`, in declaration order.
    Menu(Vec<(SharedString, SharedString)>),
}

/// One contributed status- or title-bar item. `text` carries inline markup — see `markup` in the
/// `app` crate for the accepted spellings.
#[derive(Clone, Debug)]
pub struct BarItem {
    /// Plugin-local; `(plugin, id)` is what a runtime text update addresses.
    pub id: SharedString,
    pub bar: Bar,
    pub side: Side,
    pub text: SharedString,
    pub tooltip: Option<SharedString>,
    pub left: Option<BarAction>,
    pub right: Option<BarAction>,
}

/// Every loaded plugin's bar items, as `(plugin id, item)`. Replaced wholesale on each reload, and
/// patched in place when a plugin updates its own text — either way through `set_global`, so the
/// containers rendering them are notified.
#[derive(Default)]
pub struct BarContributions(pub Vec<(SharedString, BarItem)>);
impl Global for BarContributions {}

impl BarContributions {
    pub fn at(bar: Bar, side: Side, cx: &App) -> Vec<(SharedString, BarItem)> {
        cx.try_global::<Self>().map_or(Vec::new(), |this| {
            this.0
                .iter()
                .filter(|(_, item)| item.bar == bar && item.side == side)
                .cloned()
                .collect()
        })
    }
}

/// Which of the two non-column scopes a declared setting is stored in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingScope {
    User,
    Project,
}

impl SettingScope {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "user" => Some(Self::User),
            "project" => Some(Self::Project),
            _ => None,
        }
    }
}

/// How a declared setting is edited.
//
// ponytail: a dropdown is one more arm in `plugin_item`, add it when a plugin declares a knob with
// a fixed option list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingKind {
    Switch,
    Text,
    /// Typed behind dots, and never handed to the plugin that declared it — see the host's
    /// `settings_table`. User scope only, enforced at load: a secret in a `.qrate` is a secret in
    /// whatever repository that project gets committed to.
    Password,
}

impl SettingKind {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "switch" => Some(Self::Switch),
            "text" => Some(Self::Text),
            "password" => Some(Self::Password),
            _ => None,
        }
    }
}

/// One knob a plugin declares for the Settings window. `key` names a field inside the plugin's own
/// object in `scope` — the same object `validate` already reads as `settings.user`/`settings.project`,
/// so a declared setting needs no second storage concept.
#[derive(Clone, Debug)]
pub struct SettingSpec {
    pub key: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub scope: SettingScope,
    pub kind: SettingKind,
}

/// What was clicked, handed to the plugin so a command can act on it.
///
/// Every column field is optional because a bar item has no column under it. A command reached from
/// the status or title bar gets whatever the table has selected, and nothing when it has nothing —
/// which a plugin can tell apart from an empty column, unlike a blank string.
#[derive(Clone, Debug, Default)]
pub struct CommandContext {
    /// Column header text, which is also how diagnostics address a column.
    pub column: Option<SharedString>,
    /// The stable `c{ix}` key its settings are stored under.
    pub column_key: Option<SharedString>,
    /// The clicking plugin's own object for this column, so a command sees the same settings
    /// `validate` does without a second lookup on the Lua side.
    pub column_settings: serde_json::Value,
    pub row: Option<usize>,
    /// Every row's text for this column, in source-row order — the same view `validate` gets, so
    /// a command like "restrict to the values already here" needs nothing else.
    pub values: Vec<SharedString>,
    /// What this invocation is *about*: the option a mapping menu entry carried, or the text a
    /// suggestion request was typed against. One field rather than two, because no call is both.
    pub argument: Option<SharedString>,
}

/// A plugin's offer of what could go in the cell being typed in, and which cell it was asked for.
///
/// The cell is echoed back from the request rather than tracked here, so the table can drop an
/// answer that arrived after the user moved on — the host's generation counter stops a *stale*
/// answer for the same cell, and this stops a *late* answer for a different one.
#[derive(Default)]
pub struct Suggestions {
    pub column_key: Option<SharedString>,
    pub row: Option<usize>,
    pub items: Vec<SharedString>,
}
impl Global for Suggestions {}

/// One plugin's offer to map each column onto something it holds a list of.
///
/// The options are not fetched here and no plugin code runs to produce them: both the Settings page
/// and the column-header menu read whatever the plugin last stored under [`Self::options`], which
/// is what lets a menu — built synchronously while the user waits — show a list that came from a
/// server. Refreshing that list is an ordinary command the user clicks.
#[derive(Clone, Debug)]
pub struct ColumnMapSpec {
    /// Field written into each column's own bucket of this plugin's settings. `validate` reads it
    /// back as `settings.column[key]`.
    pub key: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    /// Key in the plugin's project-scope object holding `[{ value, label }, …]`.
    pub options: SharedString,
    /// Command that repopulates that list.
    pub refresh: SharedString,
    /// Whether a column may carry several options at once.
    pub multiple: bool,
}

/// Every loaded plugin's column map, as `(plugin id, spec)`. Replaced wholesale on each reload.
#[derive(Default)]
pub struct ColumnMapContributions(pub Vec<(SharedString, ColumnMapSpec)>);
impl Global for ColumnMapContributions {}

impl ColumnMapContributions {
    pub fn all(cx: &App) -> Vec<(SharedString, ColumnMapSpec)> {
        cx.try_global::<Self>()
            .map_or(Vec::new(), |this| this.0.clone())
    }

    /// The options a plugin last stored, as `(value, label)`. An option that is only a string reads
    /// as its own label, since a machine name is a usable label and demanding both is noise.
    pub fn options(
        plugin: &SharedString,
        spec: &ColumnMapSpec,
        cx: &App,
    ) -> Vec<(SharedString, SharedString)> {
        let stored = settings::plugins::project(plugin, cx);
        stored
            .get(spec.options.as_ref())
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| match item {
                        serde_json::Value::String(value) => {
                            Some((SharedString::from(value.clone()), value.clone().into()))
                        }
                        serde_json::Value::Object(fields) => {
                            let value = fields.get("value")?.as_str()?.to_string();
                            let label = fields
                                .get("label")
                                .and_then(|l| l.as_str())
                                .unwrap_or(&value)
                                .to_string();
                            Some((value.into(), label.into()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What a column is currently mapped to. Always a list, so the single- and multi-select forms
    /// read the same on both sides.
    pub fn selected(
        plugin: &SharedString,
        spec: &ColumnMapSpec,
        column_key: &str,
        cx: &App,
    ) -> Vec<SharedString> {
        settings::columns::get(column_key, cx)
            .plugins
            .get(plugin.as_ref())
            .and_then(|bucket| bucket.get(spec.key.as_ref()))
            .map(|value| match value {
                serde_json::Value::Array(items) => items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| SharedString::from(s.to_string())))
                    .collect(),
                serde_json::Value::String(one) => vec![one.clone().into()],
                _ => Vec::new(),
            })
            .unwrap_or_default()
    }

    /// Store a column's mapping. Both the Settings page and the column-header menu land here, so
    /// there is one write path and the two surfaces cannot disagree about the shape.
    pub fn select(
        plugin: &SharedString,
        spec: &ColumnMapSpec,
        column_key: &str,
        chosen: Vec<SharedString>,
        cx: &mut App,
    ) {
        let id = plugin.to_string();
        let key = spec.key.to_string();
        let value: Vec<String> = chosen.iter().map(SharedString::to_string).collect();
        settings::columns::update(
            column_key,
            |column| {
                let bucket = column
                    .plugins
                    .entry(id)
                    .or_insert_with(|| serde_json::Value::Object(Default::default()));
                if !bucket.is_object() {
                    *bucket = serde_json::Value::Object(Default::default());
                }
                // An empty mapping is removed rather than stored as `[]`, so `requires_settings`
                // and the "has this column been mapped" question stay the same question.
                match value.is_empty() {
                    true => {
                        if let Some(object) = bucket.as_object_mut() {
                            object.remove(&key);
                        }
                    }
                    false => bucket[&key] = value.into(),
                }
            },
            cx,
        );
    }
}

/// Every loaded plugin's contributed entries, as `(plugin id, item)`. Replaced wholesale on each
/// plugin reload.
#[derive(Default)]
pub struct MenuContributions(pub Vec<(SharedString, MenuItem)>);
impl Global for MenuContributions {}

impl MenuContributions {
    pub fn for_target(target: MenuTarget, cx: &App) -> Vec<(SharedString, MenuItem)> {
        cx.try_global::<Self>().map_or(Vec::new(), |this| {
            this.0
                .iter()
                .filter(|(_, item)| item.target == target)
                .cloned()
                .collect()
        })
    }
}

/// How a click reaches the plugin host. Installed by the host, called by `table`.
///
/// `suggest` takes no plugin id: every loaded plugin that offers suggestions is asked, and the
/// answers arrive later in [`Suggestions`] rather than as a return value, because a plugin may
/// block on a server and a keystroke cannot wait on one.
#[derive(Clone, Copy)]
pub struct PluginHooks {
    pub invoke: fn(&SharedString, &SharedString, &CommandContext, &mut App),
    pub suggest: fn(&CommandContext, &mut App),
    /// Drop what was offered *and* make sure an answer still in flight cannot put it back —
    /// clearing the global alone would not, since the run that would overwrite it is the host's.
    pub forget_suggestions: fn(&mut App),
}
impl Global for PluginHooks {}

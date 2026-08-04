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
// ponytail: switch and text only. A dropdown is one more arm in `plugin_item`, add it when a
// plugin declares a knob with a fixed option list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingKind {
    Switch,
    Text,
}

impl SettingKind {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "switch" => Some(Self::Switch),
            "text" => Some(Self::Text),
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
#[derive(Clone, Copy)]
pub struct PluginHooks {
    pub invoke: fn(&SharedString, &SharedString, &CommandContext, &mut App),
}
impl Global for PluginHooks {}

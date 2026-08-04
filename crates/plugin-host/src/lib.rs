//! Loads Lua plugins from a folder, registers each one as a validator, and publishes what they
//! contribute to the app's menus.
//!
//! Neovim's model, not an extension marketplace: a plugin is a directory containing `init.lua`,
//! dropped in by hand, discovered on startup and on demand. There is no manifest — the table the
//! script returns *is* the descriptor — and the folder name is the plugin's identity, which is
//! what the Problems panel shows, what its findings are replaced by, and what its settings are
//! stored under.

mod plugin;

pub use plugin::{LuaPlugin, Writes};

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use diagnostics::{ColumnInfo, ColumnValidator, Severity, Validators};
use gpui::{App, Global, SharedString};
use plugin_api::{CommandContext, MenuContributions, PluginHooks};

/// What the last [`reload`] loaded. Kept here as well as in the validator registry because a menu
/// command needs the plugin itself, which a `dyn ColumnValidator` cannot be recovered from.
#[derive(Default)]
struct Plugins(Vec<Rc<LuaPlugin>>);
impl Global for Plugins {}

/// So one handle can sit in the validator registry and another in [`Plugins`], rather than
/// loading each script twice. A newtype because the orphan rule forbids implementing a foreign
/// trait for `Rc` directly.
struct Shared(Rc<LuaPlugin>);

impl ColumnValidator for Shared {
    fn name(&self) -> SharedString {
        self.0.name()
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: &[SharedString],
    ) -> Vec<(usize, Severity, SharedString)> {
        self.0.validate(column, values)
    }
}

/// Where a user drops a plugin. Created on demand by [`open_plugins_folder`], since a menu item
/// that opens nothing is worse than no menu item.
pub fn plugins_dir() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("plugins"))
}

/// Scan, load, and register every plugin, replacing whatever the previous call registered.
///
/// A plugin that fails to load is still registered: it reports its own failure on every run, which
/// is how a syntax error reaches the Problems panel instead of vanishing into stderr.
pub fn reload(cx: &mut App) {
    for plugin in std::mem::take(&mut cx.default_global::<Plugins>().0) {
        Validators::remove(&plugin.name(), cx);
    }

    let loaded: Vec<Rc<LuaPlugin>> = discover()
        .into_iter()
        .map(|(name, source)| {
            let project = settings::plugins::project(&name, cx);
            let user = settings::plugins::user(&name, cx);
            Rc::new(LuaPlugin::load(&name, &source, project, user))
        })
        .collect();

    let contributions = loaded
        .iter()
        .flat_map(|plugin| {
            let name = plugin.name();
            plugin
                .menu()
                .iter()
                .map(move |item| (name.clone(), item.clone()))
        })
        .collect();

    for plugin in &loaded {
        Validators::register(Box::new(Shared(plugin.clone())), cx);
    }
    cx.set_global(MenuContributions(contributions));
    cx.set_global(PluginHooks { invoke });
    cx.default_global::<Plugins>().0 = loaded;
}

pub fn open_plugins_folder() {
    let Some(dir) = plugins_dir() else { return };
    if let Err(err) =
        fs::create_dir_all(&dir).and_then(|()| settings::os_open::open_in_default_app(&dir))
    {
        eprintln!("failed to open the plugins folder: {err}");
    }
}

/// Run a contributed menu command and store whatever it asks for. Reached through
/// [`PluginHooks`], so `table` can trigger this without depending on this crate.
fn invoke(plugin: &SharedString, command: &SharedString, ctx: &CommandContext, cx: &mut App) {
    let Some(found) = cx
        .try_global::<Plugins>()
        .and_then(|plugins| plugins.0.iter().find(|p| &p.name() == plugin).cloned())
    else {
        return;
    };

    match found.command(command, ctx) {
        // ponytail: a failed command is only reported to stderr. It has no run to attach a
        // diagnostic to the way `validate` does; give commands their own diagnostic source if
        // silent failures start costing debugging time.
        Err(err) => eprintln!("{plugin}: {err}"),
        Ok(written) => {
            if let Some(value) = written.column {
                let id = plugin.to_string();
                settings::columns::update(
                    &ctx.column_key,
                    |column| {
                        column.plugins.insert(id, value);
                    },
                    cx,
                );
            }
            let scoped = written.project.is_some() || written.user.is_some();
            if let Some(value) = written.project {
                settings::plugins::set_project(plugin, value, cx);
            }
            if let Some(value) = written.user {
                settings::plugins::set_user(plugin, value, cx);
            }
            // Project and user settings are snapshotted into each VM at load, so a write to them
            // only takes effect on the next load. Column settings are read per call and need no
            // such round trip.
            if scoped {
                reload(cx);
            }
        }
    }
}

/// Every `<dir>/<name>/init.lua` under the searched roots, as `(folder name, source)`.
///
/// Reading the file here rather than in [`LuaPlugin`] keeps the VM ignorant of the filesystem, so
/// a plugin can be tested from a string.
fn discover() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for dir in search_paths() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(source) = fs::read_to_string(entry.path().join("init.lua")) else {
                continue;
            };
            found.push((entry.file_name().to_string_lossy().into_owned(), source));
        }
    }
    found
}

fn search_paths() -> Vec<PathBuf> {
    // The working directory comes second so the repo's own plugins load from `cargo run` without a
    // copy step. Drop it once there is an installer.
    plugins_dir()
        .into_iter()
        .chain([PathBuf::from("plugins")])
        .collect()
}

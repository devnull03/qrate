//! Loads Lua plugins from a folder, registers each one as a validator, and publishes what they
//! contribute to the app's menus and Settings window.
//!
//! Neovim's model, not an extension marketplace: a plugin is a `<name>.lua` file or a `<name>/`
//! directory containing `init.lua`, dropped in by hand, discovered on startup and on demand. There
//! is no manifest — the table the script returns *is* the descriptor, and it may declare a `name`
//! to override the one on disk. That name is the plugin's identity, which is what the Problems
//! panel shows, what its findings are replaced by, and what its settings are stored under, so
//! renaming a plugin orphans whatever it had already stored.

mod plugin;

pub use plugin::{LuaPlugin, Writes};
// So the Settings window can render a plugin's knobs without depending on `plugin-api` directly.
pub use plugin_api::{SettingKind, SettingScope, SettingSpec};

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use diagnostics::{ColumnInfo, ColumnValidator, Severity, Validators};
use gpui::{App, Global, SharedString};
use plugin_api::{CommandContext, MenuContributions, PluginHooks};
use serde_json::Value as Json;

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

    // Settings are fetched after loading rather than passed in, because a plugin may rename itself
    // in its descriptor and its stored object is keyed by whatever it ends up called.
    let loaded: Vec<Rc<LuaPlugin>> = discover()
        .into_iter()
        .map(|(id, source)| Rc::new(LuaPlugin::load(&id, &source)))
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
    refresh_scoped(cx);
}

/// Every loaded plugin that declares settings, as `(name, description, its knobs)`. Read directly
/// rather than through a global because the Settings window lives in `app`, which already links
/// the host.
pub fn setting_specs(cx: &App) -> Vec<(SharedString, Option<SharedString>, Vec<SettingSpec>)> {
    cx.try_global::<Plugins>().map_or(Vec::new(), |plugins| {
        plugins
            .0
            .iter()
            .filter(|plugin| !plugin.settings().is_empty())
            .map(|plugin| {
                (
                    plugin.name(),
                    plugin.description(),
                    plugin.settings().to_vec(),
                )
            })
            .collect()
    })
}

/// One declared setting's stored value, [`Json::Null`] if the plugin has never stored it.
pub fn setting_value(plugin: &str, spec: &SettingSpec, cx: &App) -> Json {
    let object = match spec.scope {
        SettingScope::User => settings::plugins::user(plugin, cx),
        SettingScope::Project => settings::plugins::project(plugin, cx),
    };
    object.get(spec.key.as_ref()).cloned().unwrap_or(Json::Null)
}

/// Store one declared setting, merging into the plugin's object for that scope rather than
/// replacing it the way a command's write does — the knobs are independent of each other.
pub fn set_setting(plugin: &str, spec: &SettingSpec, value: Json, cx: &mut App) {
    let mut object = match spec.scope {
        SettingScope::User => settings::plugins::user(plugin, cx),
        SettingScope::Project => settings::plugins::project(plugin, cx),
    };
    if !object.is_object() {
        object = Json::Object(Default::default());
    }
    object[spec.key.as_ref()] = value;
    match spec.scope {
        SettingScope::User => settings::plugins::set_user(plugin, object, cx),
        SettingScope::Project => settings::plugins::set_project(plugin, object, cx),
    }
    refresh_scoped(cx);
}

/// Hand every loaded plugin the current project- and user-scope objects, so a write takes effect on
/// the next `validate` without reloading the VMs.
fn refresh_scoped(cx: &mut App) {
    let plugins = cx
        .try_global::<Plugins>()
        .map_or(Vec::new(), |plugins| plugins.0.clone());
    for plugin in plugins {
        let name = plugin.name();
        plugin.set_scoped(
            settings::plugins::project(&name, cx),
            settings::plugins::user(&name, cx),
        );
    }
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
            // Column settings are read per call; the other two scopes are held in the plugin and
            // have to be handed back after a write.
            if scoped {
                refresh_scoped(cx);
            }
        }
    }
}

/// Every plugin under the searched roots, as `(id, source)`. A plugin is either a `<name>.lua`
/// file or a `<name>/init.lua` folder — the folder form for anything that outgrows one file — and
/// either way the name on disk is the id until the descriptor overrides it.
///
/// Reading the file here rather than in [`LuaPlugin`] keeps the VM ignorant of the filesystem, so
/// a plugin can be tested from a string.
fn discover() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for dir in search_paths() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let (id, file) = if path.is_dir() {
                (entry.file_name(), path.join("init.lua"))
            } else if path.extension().is_some_and(|ext| ext == "lua") {
                match path.file_stem() {
                    Some(stem) => (stem.to_os_string(), path.clone()),
                    None => continue,
                }
            } else {
                continue;
            };
            // First form wins, so converting `foo.lua` into `foo/` and forgetting to delete the
            // file leaves one plugin rather than two fighting over one settings bucket.
            let id = id.to_string_lossy().into_owned();
            if found.iter().any(|(seen, _)| seen == &id) {
                continue;
            }
            if let Ok(source) = fs::read_to_string(&file) {
                found.push((id, source));
            }
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

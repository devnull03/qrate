//! Loads Lua plugins from a folder, runs them off the UI thread, and publishes what they
//! contribute to the app's menus, bars, and Settings window.
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
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use diagnostics::{
    AsyncValidators, ColumnSnapshot, ColumnValidator as _, DATASET_MAIN, Diagnostics, Source,
    Validators,
};
use gpui::{App, AppContext as _, Global, SharedString, Task};
use plugin_api::{BarContributions, CommandContext, MenuContributions, PluginHooks};
use serde_json::Value as Json;

/// How long the grid must be quiet before plugins run. A commit fires per keystroke, and coalescing
/// them is a bigger win than the threading — the threading only stops one slow plugin dropping a
/// frame; this stops thirty runs being started at all.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// What the last [`reload`] loaded, and whatever run is in flight.
#[derive(Default)]
struct Plugins {
    loaded: Vec<Arc<LuaPlugin>>,
    /// Dropping this cancels the publish that would have followed, so assigning a fresh run is the
    /// whole staleness scheme — the same trick `TablePanel`'s autosave task uses.
    run: Option<Task<()>>,
}
impl Global for Plugins {}

/// Where a user drops a plugin. Created on demand by [`open_plugins_folder`], since a menu item
/// that opens nothing is worse than no menu item.
pub fn plugins_dir() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("plugins"))
}

/// Scan and load every plugin, replacing whatever the previous call loaded.
///
/// A plugin that fails to load is kept: it reports its own failure on every run, which is how a
/// syntax error reaches the Problems panel instead of vanishing into stderr.
pub fn reload(cx: &mut App) {
    // The in-flight run goes first: it holds handles to plugins that are about to be replaced, and
    // its publish would resurrect findings this loop is clearing.
    let previous = {
        let plugins = cx.default_global::<Plugins>();
        plugins.run = None;
        std::mem::take(&mut plugins.loaded)
    };
    for plugin in previous {
        Validators::remove(&plugin.name(), cx);
    }

    // Settings are fetched after loading rather than passed in, because a plugin may rename itself
    // in its descriptor and its stored object is keyed by whatever it ends up called.
    let loaded: Vec<Arc<LuaPlugin>> = discover()
        .into_iter()
        .map(|(id, source)| Arc::new(LuaPlugin::load(&id, &source)))
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
    let bar = loaded
        .iter()
        .flat_map(|plugin| {
            let name = plugin.name();
            plugin
                .bar()
                .iter()
                .map(move |item| (name.clone(), item.clone()))
        })
        .collect();

    cx.set_global(MenuContributions(contributions));
    cx.set_global(BarContributions(bar));
    cx.set_global(PluginHooks { invoke });
    cx.set_global(AsyncValidators {
        run: validate_async,
    });
    cx.default_global::<Plugins>().loaded = loaded;
    refresh_scoped(cx);
}

/// Run every plugin over the snapshot on the background executor, and publish what they find.
///
/// Plugins are not in the [`Validators`] registry: a `ColumnValidator` answers on the stack, and a
/// plugin that fetches over HTTP cannot. They reach the same store by the same replace-by-source
/// rule, just later — which is invisible to the Problems panel and the squiggle, since both read
/// the store rather than the run.
fn validate_async(columns: &[ColumnSnapshot], cx: &mut App) {
    let plugins = cx
        .try_global::<Plugins>()
        .map_or(Vec::new(), |plugins| plugins.loaded.clone());
    if plugins.is_empty() {
        return;
    }
    let columns = columns.to_vec();

    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(DEBOUNCE).await;
        let found = cx
            .background_spawn(async move {
                plugins
                    .iter()
                    .map(|plugin| {
                        let name = plugin.name();
                        let items = columns
                            .iter()
                            .flat_map(|column| {
                                let found = plugin.validate(&column.info(), &column.values);
                                diagnostics::address(name.clone(), column, found)
                            })
                            .collect();
                        (name, items, plugin.take_bar_updates())
                    })
                    .collect::<Vec<_>>()
            })
            .await;

        cx.update(|cx| {
            for (name, items, updates) in found {
                Diagnostics::set(&Source::Validator(name.clone()), DATASET_MAIN, items, cx);
                apply_bar_updates(&name, updates, cx);
            }
        });
    });
    cx.default_global::<Plugins>().run = Some(task);
}

/// Retitle the items a plugin asked to change while it ran. Written back through `set_global` so
/// the containers rendering the bars are notified; an update naming an item the plugin never
/// declared is dropped, since there is nothing to show it on.
fn apply_bar_updates(
    plugin: &SharedString,
    updates: Vec<(SharedString, SharedString)>,
    cx: &mut App,
) {
    if updates.is_empty() {
        return;
    }
    let mut bar = cx
        .try_global::<BarContributions>()
        .map_or(Vec::new(), |bar| bar.0.clone());
    for (id, text) in updates {
        if let Some((_, item)) = bar
            .iter_mut()
            .find(|(owner, item)| owner == plugin && item.id == id)
        {
            item.text = text;
        }
    }
    cx.set_global(BarContributions(bar));
}

/// Every loaded plugin that declares settings, as `(name, description, its knobs)`. Read directly
/// rather than through a global because the Settings window lives in `app`, which already links
/// the host.
pub fn setting_specs(cx: &App) -> Vec<(SharedString, Option<SharedString>, Vec<SettingSpec>)> {
    cx.try_global::<Plugins>().map_or(Vec::new(), |plugins| {
        plugins
            .loaded
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
        .map_or(Vec::new(), |plugins| plugins.loaded.clone());
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

/// What to run once a command's writes have landed. Installed by `app`, which is the only crate
/// linking both `table` and this one. A command finishes after the click that started it has
/// returned, so the caller cannot revalidate for us the way it could when commands were blocking.
static AFTER_COMMAND: OnceLock<fn(&mut App)> = OnceLock::new();

pub fn on_command_finished(hook: fn(&mut App)) {
    let _ = AFTER_COMMAND.set(hook);
}

/// Run a contributed menu command and store whatever it asks for. Reached through
/// [`PluginHooks`], so `table` can trigger this without depending on this crate.
///
/// The command runs on the background executor because a plugin may block on a server for as long
/// as `HTTP_TIMEOUT`, and doing that on the UI thread froze the grid outright.
fn invoke(plugin: &SharedString, command: &SharedString, ctx: &CommandContext, cx: &mut App) {
    let Some(found) = cx
        .try_global::<Plugins>()
        .and_then(|plugins| plugins.loaded.iter().find(|p| &p.name() == plugin).cloned())
    else {
        return;
    };
    let (plugin, command, ctx) = (plugin.clone(), command.clone(), ctx.clone());

    cx.spawn(async move |cx| {
        let ran = cx
            .background_spawn({
                let (found, command, ctx) = (found.clone(), command.clone(), ctx.clone());
                async move { found.command(&command, &ctx) }
            })
            .await;
        let updates = found.take_bar_updates();

        cx.update(|cx| {
            // Applied even when the command failed: a plugin that set "⏳ checking…" and then hit an
            // error would otherwise leave that text on the bar forever.
            apply_bar_updates(&plugin, updates, cx);
            match ran {
                // ponytail: a failed command is only reported to stderr. It has no run to attach a
                // diagnostic to the way `validate` does; give commands their own diagnostic source if
                // silent failures start costing debugging time.
                Err(err) => eprintln!("{plugin}: {err}"),
                Ok(written) => {
                    // A bar command has no column to write to; dropping the write beats inventing a
                    // column for it to land in.
                    if let (Some(value), Some(key)) = (written.column, ctx.column_key.as_ref()) {
                        let id = plugin.to_string();
                        settings::columns::update(
                            key,
                            |column| {
                                column.plugins.insert(id, value);
                            },
                            cx,
                        );
                    }
                    let scoped = written.project.is_some() || written.user.is_some();
                    if let Some(value) = written.project {
                        settings::plugins::set_project(&plugin, value, cx);
                    }
                    if let Some(value) = written.user {
                        settings::plugins::set_user(&plugin, value, cx);
                    }
                    // Column settings are read per call; the other two scopes are held in the plugin
                    // and have to be handed back after a write.
                    if scoped {
                        refresh_scoped(cx);
                    }
                    if let Some(after) = AFTER_COMMAND.get() {
                        after(cx);
                    }
                }
            }
        });
    })
    .detach();
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

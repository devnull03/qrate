//! Loads Lua plugins from a folder, runs them off the UI thread, and publishes what they
//! contribute to the app's menus, bars, and Settings window.
//!
//! Neovim's model, not an extension marketplace: a plugin is a `<name>.lua` file or a `<name>/`
//! directory containing `init.lua`, dropped in by hand, discovered on startup and on demand. There
//! is no manifest — the table the script returns *is* the descriptor, and it may declare a `name`
//! to override the one on disk. That name is the plugin's identity, which is what the Problems
//! panel shows, what its findings are replaced by, and what its settings are stored under, so
//! renaming a plugin orphans whatever it had already stored. A folder plugin may `require` any
//! other `.lua` file beside its `init.lua`, and nothing else — the host reads the folder, so the
//! sandbox never gains a way to name a file itself.

mod plugin;

pub use plugin::{Env, LuaPlugin, PERMISSION_NET, Writes};
// So the Settings window can render a plugin's knobs without depending on `plugin-api` directly.
pub use plugin_api::{SettingKind, SettingScope, SettingSpec};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use diagnostics::{
    AsyncValidators, ColumnSnapshot, ColumnValidator as _, DATASET_MAIN, Diagnostics, Source,
    Validators,
};
use gpui::{App, AppContext as _, Global, SharedString, Task};
use plugin_api::{
    BarContributions, ColumnMapContributions, CommandContext, MenuContributions, PluginHooks,
    Suggestions,
};
use serde_json::Value as Json;

/// How long the grid must be quiet before plugins run. A commit fires per keystroke, and coalescing
/// them is a bigger win than the threading — the threading only stops one slow plugin dropping a
/// frame; this stops thirty runs being started at all.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// How long typing must pause before plugins are asked what could go in the cell. Shorter than
/// [`DEBOUNCE`], because a suggestion the user has already typed past is worthless, while a
/// diagnostic that lands a moment late is not.
const SUGGEST_DEBOUNCE: Duration = Duration::from_millis(120);

/// What the last [`reload`] loaded, and whatever run is in flight.
#[derive(Default)]
struct Plugins {
    loaded: Vec<Arc<LuaPlugin>>,
    /// Dropping this cancels the publish that would have followed, so assigning a fresh run is the
    /// whole staleness scheme — the same trick `TablePanel`'s autosave task uses.
    run: Option<Task<()>>,
    /// Found on disk but switched off, so the Settings page can offer them back.
    off: Vec<SharedString>,
    suggesting: Option<Task<()>>,
    /// Bumped per suggestion request and re-read before publishing. Dropping the task is not enough
    /// on its own: the background half may already be past the point of no return when a newer
    /// keystroke arrives, and an answer for `Fi` must not land after the user typed `Film`.
    generation: u64,
}
impl Global for Plugins {}

/// Where a user drops a plugin. Created on demand by [`open_plugins_folder`], since a menu item
/// that opens nothing is worse than no menu item.
pub fn plugins_dir() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("plugins"))
}

/// Where `qrate.storage` lands: beside the app's own data, never in the project file. What a plugin
/// caches is about this machine — a verdict fetched from a server, a downloaded list — and a
/// project file is a thing people commit and hand to each other.
fn storage_path(id: &str) -> Option<PathBuf> {
    // A plugin id is a file stem the user chose, so it reaches here as a path component. Anything
    // that could climb out of the folder is refused rather than sanitised, because a plugin named
    // `..` is a bug worth seeing and not one worth quietly renaming.
    if id.is_empty() || id.contains(['/', '\\', ':']) || id.starts_with('.') {
        log::warn!("plugin {id:?} has a name that cannot be a file, so it gets no storage");
        return None;
    }
    settings::data_dir().map(|dir| dir.join("plugin-storage").join(format!("{id}.json")))
}

/// What a plugin cached last time it ran. A missing or unreadable file reads as nothing cached,
/// which is always safe: a cache that cannot be read is a cache that has to be refilled.
fn read_storage(id: &str) -> Json {
    let Some(path) = storage_path(id) else {
        return Json::Null;
    };
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
            log::warn!(
                "{id}: discarding unreadable storage at {}: {err}",
                path.display()
            );
            Json::Null
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Json::Null,
        Err(err) => {
            log::warn!("{id}: could not read storage at {}: {err}", path.display());
            Json::Null
        }
    }
}

/// Write out whatever a plugin changed while it ran, and nothing when it changed nothing.
///
/// Called from the background half of a run, never the UI thread: a cache that grows to a few
/// thousand entries is a file write, and a file write is not a thing to do while a frame is due.
fn flush_storage(plugin: &LuaPlugin) {
    let Some(value) = plugin.take_storage() else {
        return;
    };
    let id = plugin.id();
    let Some(path) = storage_path(&id) else {
        return;
    };
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| serde_json::to_string(&value).map_err(std::io::Error::other))
        .and_then(|text| fs::write(&path, text));
    if let Err(err) = written {
        log::warn!("{id}: could not save storage to {}: {err}", path.display());
    }
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
    // in its descriptor and its stored object is keyed by whatever it ends up called. Grants and
    // the enable switch cannot wait for that: both decide whether the plugin runs at all, so they
    // are keyed by the name on disk, which is also the only name a user has to go on before a
    // plugin has successfully loaded.
    let (mut loaded, mut off) = (Vec::new(), Vec::new());
    for (id, source, modules) in discover() {
        let state = settings::plugins::state(&id, cx);
        // Switched-off plugins are remembered by name and nothing else: the Settings page has to be
        // able to offer one back, and a VM is exactly what must not be built to do that.
        if !state.enabled {
            off.push(SharedString::from(id));
            continue;
        }
        let env = Env {
            modules,
            granted: state.granted,
            storage: read_storage(&id),
        };
        loaded.push(Arc::new(LuaPlugin::load(&id, &source, env)));
    }

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

    let column_maps = loaded
        .iter()
        .filter_map(|plugin| Some((plugin.name(), plugin.column_map()?.clone())))
        .collect();

    cx.set_global(MenuContributions(contributions));
    cx.set_global(BarContributions(bar));
    cx.set_global(ColumnMapContributions(column_maps));
    cx.set_global(PluginHooks {
        invoke,
        suggest,
        forget_suggestions: clear_suggestions,
    });
    cx.set_global(AsyncValidators {
        run: validate_async,
    });
    // The log, not the Problems panel: a syntax error is the plugin author's problem, and the
    // panel is the archivist's list of what is wrong with their data. Help ▸ Copy Debug Info
    // carries this out to whoever can fix it.
    for plugin in &loaded {
        if let Some(err) = plugin.load_error() {
            log::error!("{} failed to load: {err}", plugin.name());
        }
    }

    let plugins = cx.default_global::<Plugins>();
    plugins.loaded = loaded;
    plugins.off = off;
    refresh_scoped(cx);
}

/// Every loaded plugin as `(name, why it failed)`, for the Help ▸ Copy Debug Info dump.
pub fn status(cx: &App) -> Vec<(SharedString, Option<String>)> {
    cx.try_global::<Plugins>().map_or(Vec::new(), |plugins| {
        plugins
            .loaded
            .iter()
            .map(|plugin| (plugin.name(), plugin.load_error().map(str::to_string)))
            .chain(
                plugins
                    .off
                    .iter()
                    .map(|id| (id.clone(), Some("switched off".to_string()))),
            )
            .collect()
    })
}

/// One plugin as the Settings ▸ Plugins page lists it.
pub struct Listing {
    /// The name on disk, which is what its enable switch and its grants are keyed by. Not the
    /// descriptor's `name`: a plugin that fails to load has no descriptor, and switching a broken
    /// plugin off has to work.
    pub id: SharedString,
    pub description: Option<SharedString>,
    pub load_error: Option<String>,
    /// What it asked to be allowed to do. Whether the user agreed is read live from
    /// [`settings::plugins::state`], so a switch never draws itself from a stale copy.
    pub permissions: Vec<SharedString>,
}

/// Every plugin on disk, running or not, in a stable order.
pub fn listing(cx: &App) -> Vec<Listing> {
    let Some(plugins) = cx.try_global::<Plugins>() else {
        return Vec::new();
    };
    let mut listing: Vec<Listing> = plugins
        .loaded
        .iter()
        .map(|plugin| Listing {
            id: plugin.id(),
            description: plugin.description(),
            load_error: plugin.load_error().map(str::to_string),
            permissions: plugin.permissions().to_vec(),
        })
        .chain(plugins.off.iter().map(|id| Listing {
            id: id.clone(),
            description: None,
            load_error: None,
            // Unknown until it loads — nothing has read its descriptor. Turning it back on is the
            // only thing its row needs to offer.
            permissions: Vec::new(),
        }))
        .collect();
    listing.sort_by(|a, b| a.id.cmp(&b.id));
    listing
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
    // Re-read here rather than only on a settings write: this is where it is about to be used, and
    // the alternative is an observer in a crate that has no business watching app settings.
    let subdelimiter = settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx);
    for plugin in &plugins {
        plugin.set_app_settings(subdelimiter.clone());
    }
    let columns = columns.to_vec();

    let columns_asked = columns.len();
    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(DEBOUNCE).await;
        let started = std::time::Instant::now();
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
                        flush_storage(plugin);
                        (name, items, plugin.take_bar_updates())
                    })
                    .collect::<Vec<_>>()
            })
            .await;

        // The per-plugin, per-column numbers are in `plugin::timed`; this is the one that says
        // whether a whole pass is what the user is feeling.
        log::debug!(
            "validated {columns_asked} columns with {} plugins in {:.1?}",
            found.len(),
            started.elapsed()
        );
        cx.update(|cx| {
            for (name, items, updates) in found {
                Diagnostics::set(&Source::Validator(name.clone()), DATASET_MAIN, items, cx);
                apply_bar_updates(&name, updates, cx);
            }
        });
    });
    cx.default_global::<Plugins>().run = Some(task);
}

/// Ask every plugin what could go in the cell being typed in, and publish the answers.
///
/// Reached through [`PluginHooks`], so `table` can ask without depending on this crate. The shape is
/// nvim-lint's: debounce, run, then re-check the generation before publishing — dropping the task
/// alone would not do, because the background half may already be past the point where cancelling
/// it takes effect when the next keystroke arrives.
fn suggest(ctx: &CommandContext, cx: &mut App) {
    let plugins = cx
        .try_global::<Plugins>()
        .map_or(Vec::new(), |plugins| plugins.loaded.clone());
    if plugins.is_empty() {
        return;
    }
    let generation = {
        let plugins = cx.default_global::<Plugins>();
        plugins.generation += 1;
        plugins.generation
    };
    // Each plugin's own column bucket is resolved here, on the UI thread: the request goes to every
    // plugin at once, so the caller cannot say which bucket belongs to which, and the run itself has
    // no `App` to look one up from.
    let asking: Vec<(Arc<LuaPlugin>, CommandContext)> = plugins
        .into_iter()
        .map(|plugin| {
            let mut ctx = ctx.clone();
            ctx.column_settings = ctx
                .column_key
                .as_ref()
                .and_then(|key| {
                    settings::columns::get(key, cx)
                        .plugins
                        .get(plugin.name().as_ref())
                        .cloned()
                })
                .unwrap_or(Json::Null);
            (plugin, ctx)
        })
        .collect();
    let ctx = ctx.clone();

    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(SUGGEST_DEBOUNCE).await;
        let (asked, items) = cx
            .background_spawn(async move {
                let mut items: Vec<SharedString> = Vec::new();
                for (plugin, ctx) in &asking {
                    match plugin.suggest(ctx) {
                        Ok(found) => items.extend(found),
                        // A plugin that throws mid-suggestion is the author's bug, and there is
                        // nowhere in a completion list to say so.
                        Err(err) => log::error!("{}: {err}", plugin.id()),
                    }
                    flush_storage(plugin);
                }
                items.dedup();
                (ctx, items)
            })
            .await;

        cx.update(|cx| publish_suggestions(generation, &asked, items, cx));
    });
    cx.default_global::<Plugins>().suggesting = Some(task);
}

/// Show what a run came back with, unless a newer request has since been made.
///
/// The guard is the whole point of the generation counter: dropping the task cancels the *timer*,
/// but a run already past its debounce keeps going, and its answer would otherwise land on top of
/// the newer one — the list for `Fi` replacing the list for `Film`.
fn publish_suggestions(
    generation: u64,
    asked: &CommandContext,
    items: Vec<SharedString>,
    cx: &mut App,
) {
    if cx.default_global::<Plugins>().generation != generation {
        return;
    }
    cx.set_global(Suggestions {
        column_key: asked.column_key.clone(),
        row: asked.row,
        items,
    });
}

/// Forget whatever was offered, and make sure no answer still in flight can put it back. Called
/// when the editor closes: a completion list outliving the cell it belongs to would hang over
/// whatever the user does next.
pub fn clear_suggestions(cx: &mut App) {
    let plugins = cx.default_global::<Plugins>();
    plugins.generation += 1;
    plugins.suggesting = None;
    cx.set_global(Suggestions::default());
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

/// Hand every loaded plugin the current project- and user-scope objects and the app-wide values it
/// is not allowed to have its own opinion about, so a write takes effect on the next `validate`
/// without reloading the VMs.
pub fn refresh_scoped(cx: &mut App) {
    let plugins = cx
        .try_global::<Plugins>()
        .map_or(Vec::new(), |plugins| plugins.loaded.clone());
    let subdelimiter = settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx);
    for plugin in plugins {
        let name = plugin.name();
        plugin.set_scoped(
            settings::plugins::project(&name, cx),
            settings::plugins::user(&name, cx),
        );
        plugin.set_app_settings(subdelimiter.clone());
    }
}

pub fn open_plugins_folder() {
    let Some(dir) = plugins_dir() else { return };
    if let Err(err) =
        fs::create_dir_all(&dir).and_then(|()| settings::os_open::open_in_default_app(&dir))
    {
        log::error!("failed to open the plugins folder: {err}");
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
                async move {
                    let ran = found.command(&command, &ctx);
                    flush_storage(&found);
                    ran
                }
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
                Err(err) => log::error!("{plugin}: {err}"),
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

/// A plugin as it was found on disk: its id, its entry source, and the modules beside it.
type Discovered = (String, String, Vec<(String, String)>);

/// Every plugin under the searched roots. A plugin is either a
/// `<name>.lua` file or a `<name>/init.lua` folder — the folder form for anything that outgrows one
/// file — and either way the name on disk is the id until the descriptor overrides it.
///
/// Reading the files here rather than in [`LuaPlugin`] keeps the VM ignorant of the filesystem, so
/// a plugin can be tested from a string.
fn discover() -> Vec<Discovered> {
    let mut found: Vec<Discovered> = Vec::new();
    for dir in search_paths() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            // Absent is the normal case for a user who has never installed one; unreadable is not.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                log::warn!("skipping plugin folder {}: {err}", dir.display());
                continue;
            }
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
            if found.iter().any(|(seen, _, _)| seen == &id) {
                continue;
            }
            match fs::read_to_string(&file) {
                Ok(source) => found.push((id, source, modules(&path))),
                // A `<name>/` with no `init.lua` is the usual cause, and vanishing without a word
                // is what makes it hard to spot.
                Err(err) => log::warn!("skipping plugin {id}: {} — {err}", file.display()),
            }
        }
    }
    found
}

/// Every other `.lua` file beside a folder plugin's `init.lua`, as `(name, source)` — what its
/// `require` can reach. A single-file plugin has none, and neither form searches below one level.
fn modules(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_string_lossy().into_owned();
            if name == "init" || path.extension().is_none_or(|ext| ext != "lua") {
                return None;
            }
            fs::read_to_string(&path)
                .inspect_err(|err| log::warn!("skipping module {}: {err}", path.display()))
                .ok()
                .map(|source| (name, source))
        })
        .collect()
}

fn search_paths() -> Vec<PathBuf> {
    // The working directory comes second so the repo's own plugins load from `cargo run` without a
    // copy step. Drop it once there is an installer.
    plugins_dir()
        .into_iter()
        .chain([PathBuf::from("plugins")])
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{Plugins, clear_suggestions, publish_suggestions};
    use gpui::{SharedString, TestAppContext};
    use plugin_api::{CommandContext, Suggestions};

    fn asked(row: usize) -> CommandContext {
        CommandContext {
            column_key: Some("c0".into()),
            row: Some(row),
            ..Default::default()
        }
    }

    fn offered(cx: &mut TestAppContext) -> Vec<SharedString> {
        cx.update(|cx| {
            cx.try_global::<Suggestions>()
                .map(|s| s.items.clone())
                .unwrap_or_default()
        })
    }

    /// The race the generation counter exists for: a run that is already past its debounce keeps
    /// going when the next keystroke drops its task, and its answer must not land on top of the
    /// newer one.
    #[gpui::test]
    fn a_suggestion_from_a_superseded_request_is_dropped(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let generation = cx.default_global::<Plugins>().generation;
            publish_suggestions(generation, &asked(1), vec!["Film".into()], cx);
        });
        assert_eq!(offered(cx), vec![SharedString::from("Film")]);

        cx.update(|cx| {
            let stale = cx.default_global::<Plugins>().generation;
            // Whatever a newer request does — here, the editor closing — moves the counter on.
            clear_suggestions(cx);
            publish_suggestions(stale, &asked(1), vec!["Video".into()], cx);
        });
        assert!(
            offered(cx).is_empty(),
            "the late answer put its list back after the editor closed"
        );
    }
}

//! One Lua plugin: one virtual machine, implementing the same [`ColumnValidator`] trait a
//! compiled-in Rust validator does.
//!
//! That is the whole trick. The host is not a new subsystem sitting beside the diagnostics store —
//! it publishes into it under the same replace-by-source rule, so the Problems panel and the
//! in-cell squiggle work with no change. Only *when* it publishes differs: a plugin may block on a
//! server, so it answers off the UI thread and its findings land a moment later.
//!
//! Settings cross as opaque JSON in both directions. The host never reads into a plugin's object,
//! which is what lets a plugin add a knob without a qrate release.
//!
//! Every shared field is a `Mutex` rather than a `Cell`/`RefCell` because a `LuaPlugin` is held in
//! an `Arc` and run on the background executor — see `validate_async` in the parent module.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use diagnostics::{ColumnInfo, ColumnValidator, Severity};
use gpui::SharedString;
use mlua::{Function, Lua, LuaOptions, LuaSerdeExt as _, SerializeOptions, StdLib, Table, VmState};
use plugin_api::{
    Bar, BarAction, BarItem, ColumnMapSpec, CommandContext, MenuItem, MenuTarget, SettingKind,
    SettingScope, SettingSpec, Side,
};
use serde_json::Value as Json;

/// Enough for a vocabulary list or a compiled pattern set, far short of a runaway allocation.
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// How long one call into Lua may run before the interrupt hook kills it. A plugin that spins
/// forever would otherwise wedge the UI thread with no way out but Task Manager.
///
/// This is a runaway-loop backstop, not a performance budget: it was 250ms, which a legitimate
/// validator over a few thousand distinct values reached on its own, and being cut off mid-column
/// makes a plugin look broken rather than slow. Two seconds still kills an infinite loop before
/// anyone reaches for Task Manager. Being *slow* is reported by [`timed`] instead.
const DEADLINE: Duration = Duration::from_secs(2);

/// How long one call into a plugin may take before it is worth telling somebody about. A validator
/// runs per column on every edit, so anything at this scale is felt as the grid stuttering.
const SLOW: Duration = Duration::from_millis(150);

/// Time one call into a plugin and record it.
///
/// At debug normally and warn when it is slow, so a laggy build can be diagnosed from an ordinary
/// bug report's log tail: which plugin, which entry point, and how long. Without the label there is
/// no way to tell a slow server from a slow script from a slow host conversion.
fn timed<T>(plugin: &str, what: &str, run: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = run();
    let elapsed = started.elapsed();
    if elapsed >= SLOW {
        log::warn!("{plugin}: {what} took {elapsed:.1?}");
    } else {
        log::debug!("{plugin}: {what} took {elapsed:.1?}");
    }
    value
}

/// The host owns the timeout, not the plugin, so a hung server cannot pin the UI thread longer
/// than this. The interrupt hook cannot help here — it only runs between Lua instructions, and a
/// blocking host call is one instruction.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// A validator runs per column edit, so a plugin that fetches on every call would hammer a server
/// as fast as the user types. Sized for the first pass over a wide sheet — a batched vocabulary
/// check spends one request per ~50 distinct values, and everything after that comes from the
/// plugin's own cache — and still far short of a flood.
const HTTP_BUDGET: u32 = 120;
const HTTP_WINDOW: Duration = Duration::from_secs(60);

/// The descriptor shape this build understands. A plugin written against a later one is refused
/// rather than run half-understood; every version this host has ever known keeps working, the way
/// Zed keeps every old WIT world in the tree.
const API_VERSION: u64 = 1;

/// The one permission that exists. Declared by a plugin, granted by the user, checked here.
pub const PERMISSION_NET: &str = "net";

/// What a command asked the host to store, by scope. `None` means "leave that scope alone";
/// `Some` replaces the plugin's whole object there, which is the same replace-the-set rule
/// diagnostics use and saves inventing merge semantics.
#[derive(Default, Debug, PartialEq)]
pub struct Writes {
    pub column: Option<Json>,
    pub project: Option<Json>,
    pub user: Option<Json>,
}

/// Everything the host settles before a plugin's first line runs: what it may `require`, what it is
/// allowed to reach, and what it stored the last time it ran.
#[derive(Default)]
pub struct Env {
    pub modules: Vec<(String, String)>,
    pub granted: Vec<String>,
    pub storage: Json,
}

struct Loaded {
    lua: Lua,
    /// Overrides the file-derived id when the descriptor declares one.
    name: Option<SharedString>,
    description: Option<SharedString>,
    validate: Option<Function>,
    on_command: Option<Function>,
    suggest: Option<Function>,
    menu: Vec<MenuItem>,
    settings: Vec<SettingSpec>,
    bar: Vec<BarItem>,
    column_map: Option<ColumnMapSpec>,
    /// What the descriptor asked for, which is what the Settings page offers to grant. Kept apart
    /// from what was actually granted: the plugin declares, the user decides.
    permissions: Vec<SharedString>,
}

/// What a host function writes into while a plugin runs. Every field is an `Arc<Mutex<_>>` rather
/// than a `Cell`/`RefCell` because the closures holding them run on the background executor — see
/// `validate_async` in the parent module.
struct Shared {
    /// Read by the interrupt hook, and pushed forward before every call into Lua.
    deadline: Arc<Mutex<Instant>>,
    /// `(item id, new text)` a plugin asked for through `qrate.status.set`, drained by the host
    /// once the call returns. A buffer rather than a channel because the answer already crosses
    /// back to the UI thread with the call's result.
    bar: Arc<Mutex<Vec<(SharedString, SharedString)>>>,
    /// `qrate.storage`, and whether it changed since the host last wrote it out. Held rather than
    /// written through on every `set`, because a plugin caching one verdict per value would
    /// otherwise rewrite the whole file per row.
    storage: Arc<Mutex<(Json, bool)>>,
}

pub struct LuaPlugin {
    /// The file or folder the plugin was found in, and its identity unless the descriptor renames
    /// it. Kept even when the descriptor does, so a plugin that fails to load still reports under
    /// the name its author can find on disk.
    id: SharedString,
    shared: Shared,
    /// `(project, user)`, held here because `validate` runs without an `App` to read them from.
    /// Behind a lock so a write can refresh the copy in place — reloading the VM to pick up a
    /// changed setting would re-run every `init.lua` on every keystroke in the Settings window.
    scoped: Mutex<(Json, Json)>,
    /// The sub-delimiter the table filters by, handed over for the same reason `scoped` is: a
    /// plugin has no `App` to read it from, and a plugin that split cells differently from the
    /// grid would disagree with what the user can see.
    app: Mutex<SharedString>,
    /// A load failure is kept rather than dropped, so a broken plugin can still report itself.
    state: Result<Loaded, String>,
}

impl LuaPlugin {
    /// Build a plugin from its source text, the id its file or folder gives it, and the environment
    /// the host settled for it. Taking the sources rather than a path is what lets the tests cover
    /// every path without touching the filesystem.
    pub fn load(id: &str, source: &str, env: Env) -> Self {
        let shared = Shared {
            deadline: Arc::new(Mutex::new(Instant::now())),
            bar: Arc::new(Mutex::new(Vec::new())),
            storage: Arc::new(Mutex::new((env.storage.clone(), false))),
        };
        Self {
            id: id.to_string().into(),
            state: build(id, source, &env, &shared).map_err(|err| err.to_string()),
            scoped: Mutex::new((Json::Null, Json::Null)),
            app: Mutex::new(SharedString::default()),
            shared,
        }
    }

    /// This plugin's storage if it changed since the last call, and nothing when it did not — so
    /// the host writes a file only when there is something new in it.
    pub fn take_storage(&self) -> Option<Json> {
        let mut storage = self.shared.storage.lock().unwrap();
        std::mem::replace(&mut storage.1, false).then(|| storage.0.clone())
    }

    /// The name on disk. Unlike [`ColumnValidator::name`] this never changes with the descriptor,
    /// which is what host-owned state — grants, the enable switch, the storage file — is keyed by.
    pub fn id(&self) -> SharedString {
        self.id.clone()
    }

    /// What the descriptor asked to be allowed to do.
    pub fn permissions(&self) -> &[SharedString] {
        self.state
            .as_ref()
            .map_or(&[], |l| l.permissions.as_slice())
    }

    pub fn column_map(&self) -> Option<&ColumnMapSpec> {
        self.state.as_ref().ok().and_then(|l| l.column_map.as_ref())
    }

    /// What this plugin would put in the cell being typed in. Empty when it offers nothing, which
    /// is every plugin that declares no `suggest`.
    pub fn suggest(&self, ctx: &CommandContext) -> Result<Vec<SharedString>, String> {
        let loaded = self.state.as_ref().map_err(String::clone)?;
        let Some(suggest) = loaded.suggest.as_ref() else {
            return Ok(Vec::new());
        };
        let lua = &loaded.lua;
        let table = lua
            .create_table()
            .and_then(|table| {
                table.set("column", ctx.column.as_ref().map(SharedString::as_ref))?;
                table.set("row", ctx.row.map(|r| r + 1))?;
                table.set("prefix", ctx.argument.as_ref().map(SharedString::as_ref))?;
                table.set("settings", self.settings_table(lua, &ctx.column_settings)?)?;
                Ok(table)
            })
            .map_err(|err: mlua::Error| err.to_string())?;

        self.arm();
        let found: Vec<String> =
            timed(&self.id, "suggest", || suggest.call(table)).map_err(|err| err.to_string())?;
        Ok(found.into_iter().map(SharedString::from).collect())
    }

    pub fn bar(&self) -> &[BarItem] {
        self.state.as_ref().map_or(&[], |l| l.bar.as_slice())
    }

    /// Why this plugin did not load, if it did not. The only route to the private `state`, which
    /// is what lets `reload` file the failure and the debug dump list it.
    pub fn load_error(&self) -> Option<&str> {
        self.state.as_ref().err().map(String::as_str)
    }

    /// Every `qrate.status.set` since the last call, and empty afterwards.
    pub fn take_bar_updates(&self) -> Vec<(SharedString, SharedString)> {
        std::mem::take(&mut self.shared.bar.lock().unwrap())
    }

    /// Every call into Lua restarts the compute budget the interrupt hook enforces.
    fn arm(&self) {
        *self.shared.deadline.lock().unwrap() = Instant::now() + DEADLINE;
    }

    /// The plugin's own one-line description, shown on its Settings page.
    pub fn description(&self) -> Option<SharedString> {
        self.state.as_ref().ok().and_then(|l| l.description.clone())
    }

    pub fn menu(&self) -> &[MenuItem] {
        self.state.as_ref().map_or(&[], |l| l.menu.as_slice())
    }

    pub fn settings(&self) -> &[SettingSpec] {
        self.state.as_ref().map_or(&[], |l| l.settings.as_slice())
    }

    /// Replace the project- and user-scope copies after something wrote to them.
    pub fn set_scoped(&self, project: Json, user: Json) {
        *self.scoped.lock().unwrap() = (project, user);
    }

    /// Everything the app decides about how cells are read, which is not the plugin's to store —
    /// currently the sub-delimiter, so a plugin splits a cell the same way the table does.
    pub fn set_app_settings(&self, subdelimiter: SharedString) {
        *self.app.lock().unwrap() = subdelimiter;
    }

    /// Run a contributed menu command and return what it wants stored.
    pub fn command(&self, command: &SharedString, ctx: &CommandContext) -> Result<Writes, String> {
        let loaded = self.state.as_ref().map_err(String::clone)?;
        let Some(on_command) = loaded.on_command.as_ref() else {
            return Err(format!("has no `on_command`, so `{command}` does nothing"));
        };
        self.call_command(loaded, on_command, command, ctx)
            .map_err(|err| err.to_string())
    }

    fn call_command(
        &self,
        loaded: &Loaded,
        on_command: &Function,
        command: &SharedString,
        ctx: &CommandContext,
    ) -> mlua::Result<Writes> {
        let lua = &loaded.lua;
        let table = lua.create_table()?;
        table.set("column", ctx.column.as_ref().map(SharedString::as_ref))?;
        table.set("row", ctx.row.map(|r| r + 1))?;
        table.set("argument", ctx.argument.as_ref().map(SharedString::as_ref))?;
        table.set(
            "values",
            lua.create_sequence_from(ctx.values.iter().map(SharedString::as_ref))?,
        )?;
        table.set("settings", self.settings_table(lua, &ctx.column_settings)?)?;

        self.arm();
        let written: Option<Table> = timed(&self.id, &format!("command {command}"), || {
            on_command.call((command.as_ref(), table))
        })?;
        let Some(written) = written else {
            return Ok(Writes::default());
        };
        Ok(Writes {
            column: json_field(lua, &written, "column")?,
            project: json_field(lua, &written, "project")?,
            user: json_field(lua, &written, "user")?,
        })
    }

    /// `{ column = …, project = …, user = …, app = … }` — this plugin's own objects, nobody else's,
    /// and the handful of app-wide values a plugin has to agree with rather than restate.
    fn settings_table(&self, lua: &Lua, column: &Json) -> mlua::Result<Table> {
        let table = lua.create_table()?;
        let scoped = self.scoped.lock().unwrap();
        let app = lua.create_table()?;
        app.set("subdelimiter", self.app.lock().unwrap().as_ref())?;
        table.set("app", app)?;
        table.set("column", to_lua(lua, column)?)?;
        table.set("project", to_lua(lua, &scoped.0)?)?;
        table.set("user", to_lua(lua, &scoped.1)?)?;
        Ok(table)
    }

    fn call_validate(
        &self,
        loaded: &Loaded,
        validate: &Function,
        column: &ColumnInfo,
        values: &[SharedString],
    ) -> mlua::Result<Vec<(usize, Severity, SharedString)>> {
        let lua = &loaded.lua;
        let info = lua.create_table()?;
        info.set("name", column.name)?;
        info.set("data_type", column.data_type)?;
        let cells = lua.create_sequence_from(values.iter().map(SharedString::as_ref))?;
        let bucket = column
            .settings
            .plugins
            .get(self.name().as_ref())
            .unwrap_or(&Json::Null);
        let settings = self.settings_table(lua, bucket)?;

        self.arm();
        let found: Vec<Table> = timed(&self.id, &format!("validate {}", column.name), || {
            validate.call((info, cells, settings))
        })?;

        found
            .into_iter()
            .map(|item| {
                let row: usize = item.get("row")?;
                let severity: Option<String> = item.get("severity")?;
                let message: String = item.get("message")?;
                Ok((
                    // Lua arrays are 1-based, so `values[row]` on that side is `values[row - 1]`
                    // here. `saturating_sub` keeps a plugin's off-by-one from panicking.
                    row.saturating_sub(1),
                    severity.map_or(Severity::Error, |key| Severity::from_key(&key)),
                    message.into(),
                ))
            })
            .collect()
    }
}

impl ColumnValidator for LuaPlugin {
    fn name(&self) -> SharedString {
        self.state
            .as_ref()
            .ok()
            .and_then(|loaded| loaded.name.clone())
            .unwrap_or_else(|| self.id.clone())
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: &[SharedString],
    ) -> Vec<(usize, Severity, SharedString)> {
        // A plugin that never loaded reports nothing here; `reload` logged that failure once
        // instead of once per column.
        let Ok(loaded) = self.state.as_ref() else {
            return Vec::new();
        };
        let Some(validate) = loaded.validate.as_ref() else {
            return Vec::new();
        };

        match self.call_validate(loaded, validate, column, values) {
            Ok(found) => found,
            // Logged, not returned: a plugin that throws is broken code, and this list is what is
            // wrong with the archivist's data. Reporting it here also had to invent a row.
            Err(err) => {
                log::error!("{}: {err}", self.id);
                Vec::new()
            }
        }
    }
}

/// A missing object reaches Lua as an empty table rather than `nil`, so a plugin can index it
/// without guarding every read.
fn to_lua(lua: &Lua, value: &Json) -> mlua::Result<mlua::Value> {
    match value {
        Json::Null => Ok(mlua::Value::Table(lua.create_table()?)),
        other => lua.to_value(other),
    }
}

fn json_field(lua: &Lua, table: &Table, key: &str) -> mlua::Result<Option<Json>> {
    let value: mlua::Value = table.get(key)?;
    if value.is_nil() {
        return Ok(None);
    }
    lua.from_value(value).map(Some)
}

fn build(id: &str, source: &str, env: &Env, shared: &Shared) -> mlua::Result<Loaded> {
    // Luau has no `io` and no `package` to remove, and its `debug` is already cut to two
    // functions. Leaving out `OS` and `COROUTINE` costs a plugin its clock and its ability to
    // yield, so every capability one ever gets has to arrive as a host function the host can
    // refuse.
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH,
        LuaOptions::default(),
    )?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    install_qrate(&lua, id, env, shared)?;
    install_print(&lua, id)?;
    install_require(&lua, id, &env.modules)?;
    lua.sandbox(true)?;
    lua.set_interrupt({
        let deadline = shared.deadline.clone();
        move |_| {
            if Instant::now() > *deadline.lock().unwrap() {
                Err(mlua::Error::runtime("ran too long and was stopped"))
            } else {
                Ok(VmState::Continue)
            }
        }
    });

    *shared.deadline.lock().unwrap() = Instant::now() + DEADLINE;
    // Named, or mlua labels the chunk with this file and line — so a plugin's syntax error would
    // point at qrate's source instead of the author's.
    let descriptor: Table = lua.load(source).set_name(id).eval()?;

    // Checked before anything is read off the descriptor: a plugin written against a shape this
    // build does not know would otherwise be half-loaded, and the half that parsed would run.
    let declared = descriptor.get::<Option<u64>>("api_version")?.unwrap_or(1);
    if declared > API_VERSION {
        return Err(mlua::Error::runtime(format!(
            "needs plugin API version {declared}, and this build of qrate speaks {API_VERSION}"
        )));
    }

    let validate: Option<Function> = descriptor.get("validate")?;
    let on_command: Option<Function> = descriptor.get("on_command")?;
    let suggest: Option<Function> = descriptor.get("suggest")?;
    let menu = match descriptor.get::<Option<Vec<Table>>>("menu")? {
        Some(items) => items
            .into_iter()
            .map(menu_item)
            .collect::<mlua::Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let settings = match descriptor.get::<Option<Vec<Table>>>("settings")? {
        Some(items) => items
            .into_iter()
            .map(setting_spec)
            .collect::<mlua::Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let bar = match descriptor.get::<Option<Vec<Table>>>("bar")? {
        Some(items) => items
            .into_iter()
            .map(bar_item)
            .collect::<mlua::Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let permissions: Vec<SharedString> = descriptor
        .get::<Option<Vec<String>>>("permissions")?
        .unwrap_or_default()
        .into_iter()
        .map(Into::into)
        .collect();
    if let Some(unknown) = permissions.iter().find(|p| p.as_ref() != PERMISSION_NET) {
        return Err(mlua::Error::runtime(format!(
            "asks for a permission this build does not know: {unknown:?}"
        )));
    }
    let column_map = descriptor
        .get::<Option<Table>>("column_map")?
        .map(column_map_spec)
        .transpose()?;

    if validate.is_none() && on_command.is_none() && suggest.is_none() {
        return Err(mlua::Error::runtime(
            "the returned table has none of `validate`, `on_command` or `suggest`",
        ));
    }
    Ok(Loaded {
        lua,
        // An empty `name` is no name at all, so the file it came from stays the identity rather
        // than the plugin becoming unaddressable.
        name: descriptor
            .get::<Option<String>>("name")?
            .filter(|name| !name.is_empty())
            .map(Into::into),
        description: descriptor
            .get::<Option<String>>("description")?
            .map(Into::into),
        validate,
        on_command,
        suggest,
        menu,
        settings,
        bar,
        column_map,
        permissions,
    })
}

/// Point Luau's `print` at the session log instead of a stdout nobody sees.
///
/// A packaged build has no console, so the built-in `print` is a debugging tool that works only
/// for whoever runs the app from a terminal. Tagged with the plugin's id, since the log
/// interleaves every plugin's output with the host's.
fn install_print(lua: &Lua, id: &str) -> mlua::Result<()> {
    let id = id.to_string();
    let print = lua.create_function(move |_, args: mlua::Variadic<String>| {
        log::info!("[{id}] {}", args.join("\t"));
        Ok(())
    })?;
    lua.globals().set("print", print)
}

/// `require("<name>")` for a sibling `.lua` file in the plugin's own folder, and nothing else.
///
/// Luau ships no `package`, which is the point — a plugin cannot reach the filesystem to decide for
/// itself what to load. The host reads the folder and hands the sources over, so a name resolves
/// only against those, and a module evaluates once however many times it is required.
fn install_require(lua: &Lua, id: &str, modules: &[(String, String)]) -> mlua::Result<()> {
    let id = id.to_string();
    let sources: HashMap<String, String> = modules.iter().cloned().collect();
    let loaded = lua.create_table()?;
    let require = lua.create_function(move |lua, name: String| {
        if let Some(value) = loaded.get::<Option<mlua::Value>>(name.as_str())? {
            return Ok(value);
        }
        let source = sources
            .get(&name)
            .ok_or_else(|| mlua::Error::runtime(format!("{id} has no module named {name:?}")))?;
        let value: mlua::Value = lua.load(source).set_name(format!("{id}/{name}")).eval()?;
        loaded.set(name, value.clone())?;
        Ok(value)
    })?;
    lua.globals().set("require", require)
}

/// The whole `qrate` global, installed before `sandbox` freezes the global table.
///
/// `qrate.http.get(url [, { auth = { username, password } }])` -> `{ status, body }`, or
/// `nil, message`. Returning the failure rather than raising it is what lets a plugin report an
/// unreachable server as a finding instead of as its own crash. Rate limited per VM, so one
/// plugin's loop cannot spend another's budget.
///
/// `qrate.json.decode(text)` -> a table, or `nil, message` — the same shape, since a server that
/// answers with an error page rather than JSON is a thing to report, not to crash on.
///
/// `qrate.status.set(id, text)` retitles one of the plugin's own declared bar items. It buffers
/// rather than applies, because a plugin runs off the UI thread and the bar is not reachable from
/// there.
///
/// `qrate.storage.get(key)` / `.set(key, value)` is the plugin's own cache, kept beside the app's
/// data rather than in the project file — a cached answer from a server is about this machine, not
/// about the archivist's project, and a project file is a thing people commit.
///
/// `http` is installed **only** when the user has granted `net`. Absent, a plugin's `qrate.http`
/// is nil; the stub installed in its place answers with the same `nil, message` shape everything
/// else uses, so an ungranted plugin reports a refusal instead of raising an error about indexing
/// nil.
//
// ponytail: GET only, no allowlist. Add `post` when a plugin needs to write rather than read.
fn install_qrate(lua: &Lua, id: &str, env: &Env, shared: &Shared) -> mlua::Result<()> {
    let deadline = shared.deadline.clone();
    let id = id.to_string();
    let budget = Cell::new((Instant::now(), 0u32));
    let get = lua.create_function({
        let id = id.clone();
        move |lua, (url, options): (String, Option<Table>)| {
            let (started, spent) = budget.get();
            let (started, spent) = if started.elapsed() > HTTP_WINDOW {
                (Instant::now(), 0)
            } else {
                (started, spent)
            };
            budget.set((started, spent + 1));
            if spent >= HTTP_BUDGET {
                return Ok((
                    mlua::Value::Nil,
                    mlua::Value::String(lua.create_string("rate limited")?),
                ));
            }
            // Basic auth is here rather than in the plugin because Luau has no base64 and no way to
            // build a header — not because the host has an opinion about whose credentials these are.
            // Where they came from is the plugin's business; a blank pair is the same as none, so a
            // plugin needs one code path whether or not the user has filled them in.
            let credentials = options
                .map(|options| options.get::<Option<Table>>("auth"))
                .transpose()?
                .flatten()
                .map(|auth| {
                    Ok::<_, mlua::Error>((
                        auth.get::<Option<String>>("username")?.unwrap_or_default(),
                        auth.get::<Option<String>>("password")?.unwrap_or_default(),
                    ))
                })
                .transpose()?
                .filter(|(username, password)| !username.is_empty() && !password.is_empty());

            let request = http_client().get(&url);
            let request = match credentials {
                Some((username, password)) => request.basic_auth(username, Some(password)),
                None => request,
            };
            // Logged whole rather than sampled: a plugin that looks slow is usually waiting on somebody
            // else's server, and the URL is what separates "the site is slow" from "we ask too often".
            let response = timed(&id, &format!("GET {url}"), || request.send());
            // Waiting on a server is not the script running long, so the compute budget restarts here
            // rather than the interrupt hook killing the plugin the moment the response lands.
            *deadline.lock().unwrap() = Instant::now() + DEADLINE;
            match response {
                Err(err) => Ok((
                    mlua::Value::Nil,
                    mlua::Value::String(lua.create_string(err.to_string())?),
                )),
                Ok(response) => {
                    let table = lua.create_table()?;
                    table.set("status", response.status().as_u16())?;
                    table.set("body", response.text().unwrap_or_default())?;
                    Ok((mlua::Value::Table(table), mlua::Value::Nil))
                }
            }
        }
    })?;
    // ponytail: basic auth only, and the secret sits in the app's settings database rather than the
    // OS keychain. Reach for `keyring` when a password stored at rest becomes the weak link.
    //
    // ponytail: the grant is a switch on the Settings page, not a prompt at first load — nothing in
    // the app opens a modal yet, and a first-launch storm of them is worse than a switch this
    // message points at. Build the prompt when a second permission makes the page too indirect.
    let refused = lua.create_function(|lua, _: mlua::MultiValue| {
        Ok((
            mlua::Value::Nil,
            mlua::Value::String(lua.create_string(
                "network access is off for this plugin — turn it on in Settings ▸ Plugins",
            )?),
        ))
    })?;

    let set = lua.create_function({
        let bar = shared.bar.clone();
        move |_, (id, text): (String, String)| {
            bar.lock().unwrap().push((id.into(), text.into()));
            Ok(())
        }
    })?;

    // Both directions convert the *whole* value, so reading a cache of ten thousand verdicts costs
    // ten thousand conversions — on every call. Worth logging, because the fix is on the plugin's
    // side (keep it in a local across calls) and nothing else would ever point there.
    let stored = lua.create_function({
        let (storage, id) = (shared.storage.clone(), id.to_string());
        move |lua, key: String| {
            let storage = storage.lock().unwrap();
            let value = storage.0.get(&key).cloned().unwrap_or(Json::Null);
            let what = format!("storage.get({key}) over {} entries", entries(&value));
            timed(&id, &what, || {
                lua.to_value_with(
                    &value,
                    SerializeOptions::new().serialize_unit_to_null(false),
                )
            })
        }
    })?;
    let store = lua.create_function({
        let (storage, id) = (shared.storage.clone(), id.to_string());
        move |lua, (key, value): (String, mlua::Value)| {
            let value: Json = timed(&id, &format!("storage.set({key})"), || {
                lua.from_value(value)
            })?;
            let mut storage = storage.lock().unwrap();
            if !storage.0.is_object() {
                storage.0 = Json::Object(Default::default());
            }
            storage.0[&key] = value;
            storage.1 = true;
            Ok(())
        }
    })?;

    let decode = lua.create_function(decode_json)?;

    let http = lua.create_table()?;
    http.set(
        "get",
        match env.granted.iter().any(|p| p == PERMISSION_NET) {
            true => get,
            false => refused,
        },
    )?;
    let status = lua.create_table()?;
    status.set("set", set)?;
    let json = lua.create_table()?;
    json.set("decode", decode)?;
    let storage = lua.create_table()?;
    storage.set("get", stored)?;
    storage.set("set", store)?;
    let qrate = lua.create_table()?;
    qrate.set("http", http)?;
    qrate.set("status", status)?;
    qrate.set("json", json)?;
    qrate.set("storage", storage)?;
    lua.globals().set("qrate", qrate)
}

/// How big a stored value is, in the terms a plugin author thinks in. Only ever used in a log line,
/// which is why it counts one level rather than walking the whole tree.
fn entries(value: &Json) -> usize {
    match value {
        Json::Object(map) => map.len(),
        Json::Array(items) => items.len(),
        _ => 0,
    }
}

/// JSON's `null` reaches Lua as `nil` rather than as mlua's null userdata, so a plugin reads a
/// missing field and an explicitly null one the same way.
fn decode_json(lua: &Lua, text: String) -> mlua::Result<(mlua::Value, mlua::Value)> {
    match serde_json::from_str::<Json>(&text) {
        Ok(value) => Ok((
            lua.to_value_with(
                &value,
                SerializeOptions::new().serialize_unit_to_null(false),
            )?,
            mlua::Value::Nil,
        )),
        Err(err) => Ok((
            mlua::Value::Nil,
            mlua::Value::String(lua.create_string(err.to_string())?),
        )),
    }
}

/// One client for every plugin: building one spins up a fresh runtime and thread, and doing that
/// per request showed up as the cost of a check rather than the server's.
fn http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_default()
    })
}

fn bar_item(item: Table) -> mlua::Result<BarItem> {
    let bar: String = item.get("bar")?;
    let side: String = item.get("side")?;
    Ok(BarItem {
        id: item.get::<String>("id")?.into(),
        bar: Bar::from_key(&bar)
            .ok_or_else(|| mlua::Error::runtime(format!("unknown bar {bar:?}")))?,
        side: Side::from_key(&side)
            .ok_or_else(|| mlua::Error::runtime(format!("unknown bar side {side:?}")))?,
        text: item.get::<String>("text")?.into(),
        tooltip: item.get::<Option<String>>("tooltip")?.map(Into::into),
        left: bar_action(item.get("left")?)?,
        right: bar_action(item.get("right")?)?,
    })
}

/// `{ command = "…" }` or `{ menu = { { label, command }, … } }`. Declaring both is a plugin bug
/// worth naming, since silently preferring one hides half of what the author wrote.
fn bar_action(action: Option<Table>) -> mlua::Result<Option<BarAction>> {
    let Some(action) = action else {
        return Ok(None);
    };
    let command: Option<String> = action.get("command")?;
    let menu: Option<Vec<Table>> = action.get("menu")?;
    match (command, menu) {
        (Some(_), Some(_)) => Err(mlua::Error::runtime(
            "a bar action is either a `command` or a `menu`, not both",
        )),
        (Some(command), None) => Ok(Some(BarAction::Command(command.into()))),
        (None, Some(entries)) => entries
            .into_iter()
            .map(|entry| {
                Ok((
                    entry.get::<String>("label")?.into(),
                    entry.get::<String>("command")?.into(),
                ))
            })
            .collect::<mlua::Result<Vec<_>>>()
            .map(|entries| Some(BarAction::Menu(entries))),
        (None, None) => Ok(None),
    }
}

fn menu_item(item: Table) -> mlua::Result<MenuItem> {
    let label: String = item.get("label")?;
    let target: String = item.get("target")?;
    let command: String = item.get("command")?;
    let target = MenuTarget::from_key(&target)
        .ok_or_else(|| mlua::Error::runtime(format!("unknown menu target {target:?}")))?;
    Ok(MenuItem {
        label: label.into(),
        target,
        command: command.into(),
        requires_settings: item
            .get::<Option<bool>>("requires_settings")?
            .unwrap_or(false),
    })
}

fn column_map_spec(item: Table) -> mlua::Result<ColumnMapSpec> {
    Ok(ColumnMapSpec {
        key: item.get::<String>("key")?.into(),
        label: item.get::<String>("label")?.into(),
        description: item.get::<Option<String>>("description")?.map(Into::into),
        options: item.get::<String>("options")?.into(),
        refresh: item.get::<String>("refresh")?.into(),
        multiple: item.get::<Option<bool>>("multiple")?.unwrap_or(false),
    })
}

fn setting_spec(item: Table) -> mlua::Result<SettingSpec> {
    let key: String = item.get("key")?;
    let label: String = item.get("label")?;
    let scope: String = item.get("scope")?;
    let kind: String = item.get("type")?;
    let spec = SettingSpec {
        key: key.into(),
        label: label.into(),
        description: item.get::<Option<String>>("description")?.map(Into::into),
        scope: SettingScope::from_key(&scope)
            .ok_or_else(|| mlua::Error::runtime(format!("unknown setting scope {scope:?}")))?,
        kind: SettingKind::from_key(&kind)
            .ok_or_else(|| mlua::Error::runtime(format!("unknown setting type {kind:?}")))?,
    };
    // Project scope is the `.qrate` file, which gets committed and handed around. A password does
    // not go in one, and refusing here means no plugin author can put one there by accident.
    if spec.kind == SettingKind::Password && spec.scope == SettingScope::Project {
        return Err(mlua::Error::runtime(format!(
            "setting {:?} is a password in project scope; passwords are user scope only, or they \
             end up in the shared project file",
            spec.key
        )));
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use crate::plugin::HTTP_BUDGET;
    use crate::{Env, LuaPlugin, PERMISSION_NET, Writes};
    use diagnostics::{ColumnInfo, ColumnValidator, Severity};
    use gpui::SharedString;
    use plugin_api::{Bar, BarAction, CommandContext, MenuTarget, SettingKind, SettingScope, Side};
    use serde_json::{Value as Json, json};
    use settings::columns::ColumnSettings;

    /// Loaded with `net` granted, since most of these predate the permission and are about
    /// something else. `an_ungranted_plugin_cannot_reach_the_network` covers the other case.
    fn plugin(source: &str) -> LuaPlugin {
        LuaPlugin::load("test", source, granted())
    }

    fn granted() -> Env {
        Env {
            granted: vec![PERMISSION_NET.to_string()],
            ..Default::default()
        }
    }

    /// The Islandora plugin as it sits beside a dev checkout.
    ///
    /// Read at runtime, not `include_str!`'d: the plugin is its own repository now, so it is not
    /// guaranteed to be here at all. Every test that uses this is `#[ignore]`d for the same reason
    /// — a test that silently passes when its subject is missing is worse than no test.
    fn islandora(file: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/islandora")
            .join(file);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} is not checked out here: {err}", path.display()))
    }

    fn islandora_plugin(storage: Json) -> LuaPlugin {
        let modules = ["api", "cache", "match"]
            .map(|name| (name.to_string(), islandora(&format!("{name}.lua"))))
            .to_vec();
        LuaPlugin::load(
            "islandora",
            &islandora("init.lua"),
            Env {
                modules,
                granted: vec![PERMISSION_NET.to_string()],
                storage,
            },
        )
    }

    fn check(source: &str, values: &[&str]) -> Vec<(usize, Severity, SharedString)> {
        check_with(plugin(source), Json::Null, values)
    }

    fn check_with(
        plugin: LuaPlugin,
        bucket: Json,
        values: &[&str],
    ) -> Vec<(usize, Severity, SharedString)> {
        let mut settings = ColumnSettings::default();
        if !bucket.is_null() {
            settings.plugins.insert("test".into(), bucket);
        }
        let column = ColumnInfo {
            name: "Country",
            data_type: "Text",
            settings: &settings,
        };
        let values: Vec<SharedString> = values.iter().map(|v| SharedString::from(*v)).collect();
        plugin.validate(&column, &values)
    }

    const FLAG_BAD: &str = r#"
        return {
          validate = function(column, values)
            local found = {}
            for row, value in ipairs(values) do
              if value == "bad" then
                found[#found + 1] = { row = row, message = column.name .. " is bad" }
              end
            end
            return found
          end,
        }
    "#;

    #[test]
    fn a_plugins_findings_arrive_addressed_by_zero_based_row() {
        let found = check(FLAG_BAD, &["ok", "bad", "ok", "bad"]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, 1, "Lua's row 2 is row 1 here");
        assert_eq!(found[1].0, 3);
        assert_eq!(
            found[0].1,
            Severity::Error,
            "a missing severity reads as one"
        );
        assert!(found[0].2.contains("Country"));
    }

    /// The URL never resolves, so this exercises the budget alone — a refusal costs a request too,
    /// which is what stops a plugin retrying a dead server in a loop.
    #[test]
    fn http_stops_answering_once_a_plugin_spends_its_budget() {
        // Built from the constant rather than a literal, so raising the budget cannot leave this
        // test quietly passing without ever reaching the limit.
        let source = format!(
            r#"
            return {{
              validate = function(_, _)
                local first, last
                for i = 1, {} do
                  local _, err = qrate.http.get("nonsense")
                  if i == 1 then first = err end
                  last = err
                end
                return {{ {{ row = 1, message = first }}, {{ row = 2, message = last }} }}
              end,
            }}
        "#,
            HTTP_BUDGET + 1
        );
        let found = check(&source, &["x", "y"]);
        assert_ne!(found[0].2, "rate limited");
        assert_eq!(found[1].2, "rate limited");
    }

    #[test]
    fn severity_comes_back_through_the_same_spelling_a_project_file_uses() {
        let source = r#"
            return {
              validate = function(_, values)
                return {
                  { row = 1, severity = "warning", message = "a" },
                  { row = 2, severity = "wat", message = "b" },
                }
              end,
            }
        "#;
        let found = check(source, &["x", "y"]);
        assert_eq!(found[0].1, Severity::Warning);
        assert_eq!(found[1].1, Severity::Note, "an unknown spelling degrades");
    }

    const ECHO_SETTINGS: &str = r#"
        return {
          validate = function(_, _, settings)
            return { {
              row = 1,
              message = table.concat(settings.column.allowed or {}, "|")
                .. "/" .. tostring(settings.project.mode)
                .. "/" .. tostring(settings.user.mode),
            } }
          end,
        }
    "#;

    /// Also pins that a scope handed over after construction is visible without the VM being
    /// rebuilt — or every keystroke in a Settings-window text field would reload every plugin.
    #[test]
    fn a_plugin_sees_its_own_object_in_all_three_scopes() {
        let plugin = plugin(ECHO_SETTINGS);
        plugin.set_scoped(json!({ "mode": "strict" }), json!({ "mode": "loose" }));
        let found = check_with(plugin, json!({ "allowed": ["Film", "Video"] }), &["x"]);
        assert_eq!(found[0].2, "Film|Video/strict/loose");
    }

    /// Nothing stored has to reach Lua as an empty table, not `nil`, or every plugin needs a guard
    /// on every settings read.
    #[test]
    fn unset_scopes_read_as_empty_tables() {
        let found = check(ECHO_SETTINGS, &["x"]);
        assert_eq!(found[0].2, "/nil/nil");
    }

    #[test]
    fn a_plugin_only_ever_sees_its_own_bucket() {
        let mut settings = ColumnSettings::default();
        settings
            .plugins
            .insert("someone-else".into(), json!({ "allowed": ["secret"] }));
        let column = ColumnInfo {
            name: "Country",
            data_type: "Text",
            settings: &settings,
        };
        let found = plugin(ECHO_SETTINGS).validate(&column, &["x".into()]);
        assert_eq!(
            found[0].2, "/nil/nil",
            "another plugin's object is invisible"
        );
    }

    const RESTRICT: &str = r#"
        return {
          menu = {
            { label = "Restrict", target = "column", command = "restrict" },
            { label = "Clear", target = "column", command = "clear", requires_settings = true },
          },
          on_command = function(command, ctx)
            if command == "clear" then return { column = {} } end
            return { column = { allowed = ctx.values }, project = { touched = true } }
          end,
        }
    "#;

    #[test]
    fn menu_entries_are_read_off_the_descriptor() {
        let plugin = plugin(RESTRICT);
        let menu = plugin.menu();
        assert_eq!(menu.len(), 2);
        assert_eq!(menu[0].label, "Restrict");
        assert_eq!(menu[0].target, MenuTarget::Column);
        assert!(!menu[0].requires_settings);
        assert!(
            menu[1].requires_settings,
            "a Clear entry needs something to clear"
        );
    }

    #[test]
    fn declared_settings_are_read_off_the_descriptor() {
        let source = r#"
            return {
              settings = {
                { key = "strict", label = "Strict", type = "switch", scope = "user" },
                { key = "note", label = "Note", type = "text", scope = "project",
                  description = "why" },
              },
              validate = function() return {} end,
            }
        "#;
        let plugin = plugin(source);
        let specs = plugin.settings();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].key, "strict");
        assert_eq!(specs[0].kind, SettingKind::Switch);
        assert_eq!(specs[0].scope, SettingScope::User);
        assert_eq!(specs[0].description, None);
        assert_eq!(specs[1].scope, SettingScope::Project);
        assert_eq!(specs[1].description.as_deref(), Some("why"));
    }

    const BAR: &str = r#"
        return {
          bar = {
            { id = "conn", bar = "status", side = "right", text = "**Islandora**",
              tooltip = "connection",
              left = { command = "check" },
              right = { menu = { { label = "Check now", command = "check" },
                                 { label = "Clear", command = "stop" } } } },
          },
          on_command = function(command)
            qrate.status.set("conn", "ran " .. command)
            return nil
          end,
        }
    "#;

    #[test]
    fn bar_items_are_read_off_the_descriptor() {
        let plugin = plugin(BAR);
        let bar = plugin.bar();
        assert_eq!(bar.len(), 1);
        assert_eq!(bar[0].bar, Bar::Status);
        assert_eq!(bar[0].side, Side::Right);
        assert_eq!(bar[0].tooltip.as_deref(), Some("connection"));
        assert!(matches!(&bar[0].left, Some(BarAction::Command(c)) if c == "check"));
        let Some(BarAction::Menu(entries)) = &bar[0].right else {
            panic!("the right button declared a menu: {:?}", bar[0].right);
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1], ("Clear".into(), "stop".into()));
    }

    /// The text a plugin sets mid-run is buffered rather than applied, because the bar is not
    /// reachable from the thread a plugin runs on.
    #[test]
    fn status_updates_are_buffered_until_the_host_drains_them() {
        let plugin = plugin(BAR);
        plugin.command(&"check".into(), &ctx(&[])).unwrap();

        let updates = plugin.take_bar_updates();
        assert_eq!(updates, vec![("conn".into(), "ran check".into())]);
        assert!(
            plugin.take_bar_updates().is_empty(),
            "draining twice must not replay the same update"
        );
    }

    #[test]
    fn an_unknown_bar_side_stops_the_plugin_loading() {
        let source = r#"
            return {
              bar = { { id = "x", bar = "status", side = "middle", text = "x" } },
              validate = function() return {} end,
            }
        "#;
        let plugin = plugin(source);
        assert!(
            plugin
                .load_error()
                .is_some_and(|err| err.contains("middle")),
            "{:?}",
            plugin.load_error()
        );
    }

    #[test]
    fn an_unknown_setting_type_stops_the_plugin_loading() {
        let source = r#"
            return {
              settings = { { key = "k", label = "L", type = "slider", scope = "user" } },
              validate = function() return {} end,
            }
        "#;
        let plugin = plugin(source);
        assert!(
            plugin
                .load_error()
                .is_some_and(|err| err.contains("slider")),
            "{:?}",
            plugin.load_error()
        );
    }

    /// The descriptor may rename the plugin, and that name is its identity everywhere — including
    /// which column bucket it reads, which is what this asserts by way of `ECHO_SETTINGS`.
    #[test]
    fn a_declared_name_overrides_the_one_the_file_gave_it() {
        let source = r#"
            return {
              name = "renamed",
              description = "why",
              validate = function(_, _, settings)
                return { { row = 1, message = table.concat(settings.column.allowed or {}, "|") } }
              end,
            }
        "#;
        let plugin = LuaPlugin::load("on-disk", source, Env::default());
        assert_eq!(plugin.name(), "renamed");
        assert_eq!(plugin.description().as_deref(), Some("why"));

        let mut settings = ColumnSettings::default();
        settings
            .plugins
            .insert("renamed".into(), json!({ "allowed": ["Film"] }));
        let column = ColumnInfo {
            name: "Country",
            data_type: "Text",
            settings: &settings,
        };
        let found = plugin.validate(&column, &["x".into()]);
        assert_eq!(found[0].2, "Film");
    }

    #[test]
    fn an_unnamed_plugin_keeps_the_name_its_file_gave_it() {
        for source in [
            r#"return { name = "", validate = function() return {} end }"#,
            r#"return { validate = function() return {} end }"#,
            "return { oops",
        ] {
            assert_eq!(
                LuaPlugin::load("on-disk", source, Env::default()).name(),
                "on-disk"
            );
        }
    }

    fn ctx(values: &[&str]) -> CommandContext {
        CommandContext {
            column: Some("Country".into()),
            column_key: Some("c0".into()),
            column_settings: Json::Null,
            row: None,
            values: values.iter().map(|v| SharedString::from(*v)).collect(),
            argument: None,
        }
    }

    #[test]
    fn a_command_returns_writes_the_host_applies_rather_than_calling_back() {
        let written = plugin(RESTRICT)
            .command(&"restrict".into(), &ctx(&["Film", "Video"]))
            .expect("the command runs");
        assert_eq!(
            written,
            Writes {
                column: Some(json!({ "allowed": ["Film", "Video"] })),
                project: Some(json!({ "touched": true })),
                user: None,
            }
        );
    }

    /// Clearing stores an empty object, which is exactly what `requires_settings` reads as "there
    /// is nothing here" — so the Clear entry hides itself without a second concept.
    #[test]
    fn clearing_writes_an_empty_object() {
        let written = plugin(RESTRICT)
            .command(&"clear".into(), &ctx(&[]))
            .expect("the command runs");
        assert!(!settings::plugins::is_set(written.column.as_ref().unwrap()));
    }

    /// Every failure mode is kept and readable rather than becoming a panic or silence — which is
    /// the difference between debuggable and not while authoring a plugin. It reaches the author
    /// through the log, not the Problems panel, so a broken plugin contributes no findings.
    #[test]
    fn a_plugin_that_will_not_load_reports_itself() {
        for source in [
            "return { validate = function(", // syntax error
            "return { }",                    // neither hook
            "return 42",                     // not a table
        ] {
            let plugin = plugin(source);
            assert!(plugin.load_error().is_some(), "{source:?}");
            assert!(check(source, &["x"]).is_empty(), "{source:?}");
        }
    }

    #[test]
    fn a_plugin_that_errors_mid_run_is_survived_rather_than_reported() {
        let source = r#"return { validate = function() error("boom") end }"#;
        assert!(check(source, &["x"]).is_empty());
    }

    /// A menu-only plugin is legitimate, and must not be reported as broken every run.
    #[test]
    fn a_plugin_without_validate_reports_nothing() {
        assert!(check(RESTRICT, &["x"]).is_empty());
    }

    /// Port 1 refuses immediately, so this stays offline and fast while still pinning the shape a
    /// plugin has to handle: no response, and a message rather than a raised error.
    ///
    /// Aliased at the top of the chunk the way a Neovim plugin writes `local api = vim.api`, which
    /// also pins that the global is readable while the descriptor itself is still being evaluated.
    #[test]
    fn an_unreachable_host_comes_back_as_nil_and_a_message() {
        let source = r#"
            local http = qrate.http
            return {
              validate = function()
                local response, err = http.get("http://127.0.0.1:1/")
                return { { row = 1, message = tostring(response) .. "/" .. tostring(err ~= nil) } }
              end,
            }
        "#;
        let found = check(source, &["x"]);
        assert_eq!(found[0].2, "nil/true");
    }

    /// A folder plugin's modules evaluate once each and see the `qrate` global, and a name the host
    /// was never handed fails by saying so rather than by reaching for a file.
    #[test]
    fn require_resolves_only_the_modules_the_host_handed_over() {
        let modules = [
            ("count".to_string(), "return { n = 0 }".to_string()),
            (
                "greet".to_string(),
                r#"return function(w) return "hi " .. w end"#.to_string(),
            ),
        ];
        let source = r#"
            local count = require("count")
            local greet = require("greet")
            return {
              validate = function(_, _)
                count.n = count.n + 1
                local ok, err = pcall(function() require("elsewhere") end)
                return {
                  { row = 1, message = greet("there") .. "/" .. require("count").n },
                  { row = 2, message = tostring(ok) .. "/" .. tostring(string.find(tostring(err), "elsewhere") ~= nil) },
                }
              end,
            }
        "#;
        let plugin = LuaPlugin::load(
            "test",
            source,
            Env {
                modules: modules.to_vec(),
                ..Default::default()
            },
        );
        assert_eq!(plugin.load_error(), None);
        let found = check_with(plugin, Json::Null, &["x", "y"]);
        assert_eq!(found[0].2, "hi there/1", "the same table comes back twice");
        assert_eq!(found[1].2, "false/true");
    }

    /// Decoding is the only way a plugin reads what `http.get` hands back, and bad JSON is a server
    /// answering with an error page — reported, not raised.
    #[test]
    fn json_decodes_to_tables_and_reports_what_it_cannot_read() {
        let source = r#"
            return {
              validate = function(_, _)
                local body = qrate.json.decode('{"data":[{"name":"Film"}],"next":null}')
                local bad, err = qrate.json.decode("<html>")
                return {
                  { row = 1, message = body.data[1].name .. "/" .. tostring(body.next) },
                  { row = 2, message = tostring(bad) .. "/" .. tostring(err ~= nil) },
                }
              end,
            }
        "#;
        let found = check(source, &["x", "y"]);
        assert_eq!(
            found[0].2, "Film/nil",
            "a JSON null reads as nil, not userdata"
        );
        assert_eq!(found[1].2, "nil/true");
    }

    #[test]
    fn a_plugin_written_against_a_later_api_refuses_to_load() {
        let source = r#"return { api_version = 9, validate = function() return {} end }"#;
        let err = plugin(source).load_error().unwrap_or_default().to_string();
        assert!(err.contains('9') && err.contains('1'), "{err:?}");
    }

    #[test]
    fn an_unknown_permission_stops_the_plugin_loading() {
        let source = r#"
            return {
              permissions = { "net", "disk" },
              validate = function() return {} end,
            }
        "#;
        assert!(
            plugin(source)
                .load_error()
                .is_some_and(|err| err.contains("disk")),
            "{:?}",
            plugin(source).load_error()
        );
    }

    const REACH: &str = r#"
        return {
          permissions = { "net" },
          validate = function()
            local response, err = qrate.http.get("http://127.0.0.1:1/")
            return { { row = 1, message = tostring(response) .. "/" .. tostring(err) } }
          end,
        }
    "#;

    /// Ungranted, the refusal comes back through the same `nil, message` channel every other
    /// failure uses — so a plugin reports it rather than dying on an index into nil.
    #[test]
    fn an_ungranted_plugin_cannot_reach_the_network() {
        let ungranted = check_with(
            LuaPlugin::load("test", REACH, Env::default()),
            Json::Null,
            &["x"],
        );
        assert!(
            ungranted[0].2.contains("Settings ▸ Plugins"),
            "{}",
            ungranted[0].2
        );

        // Granted, the same call reaches the network and fails on its own terms — port 1 refuses
        // immediately, so this stays offline.
        let granted = check(REACH, &["x"]);
        assert!(granted[0].2.starts_with("nil/"), "{}", granted[0].2);
        assert!(
            !granted[0].2.contains("Settings ▸ Plugins"),
            "{}",
            granted[0].2
        );
    }

    /// What a plugin caches has to outlive its VM, or a term checked once would be checked again on
    /// every launch. The host writes the file; this pins the handover at both ends.
    #[test]
    fn storage_is_handed_back_only_when_it_changed_and_survives_a_reload() {
        let source = r#"
            return {
              validate = function()
                local seen = qrate.storage.get("seen") or {}
                local was = seen.Film
                seen.Film = true
                qrate.storage.set("seen", seen)
                return { { row = 1, message = tostring(was) } }
              end,
            }
        "#;
        let settings = ColumnSettings::default();
        let column = ColumnInfo {
            name: "Country",
            data_type: "Text",
            settings: &settings,
        };
        let run = |plugin: &LuaPlugin| plugin.validate(&column, &["x".into()])[0].2.clone();

        let plugin = plugin(source);
        assert!(
            plugin.take_storage().is_none(),
            "nothing has run, so there is nothing to write"
        );
        assert_eq!(run(&plugin), "nil", "a fresh VM has nothing cached");
        assert_eq!(
            plugin.take_storage(),
            Some(json!({ "seen": { "Film": true } })),
            "what the host writes out"
        );
        assert!(
            plugin.take_storage().is_none(),
            "draining twice must not rewrite the same file"
        );

        // Handed back to a new VM the way a restart does.
        let reloaded = LuaPlugin::load(
            "test",
            source,
            Env {
                storage: json!({ "seen": { "Film": true } }),
                ..Default::default()
            },
        );
        assert_eq!(run(&reloaded), "true", "the cache came back");
    }

    /// The Islandora plugin over a pre-filled cache — everything in it except the fetching, which
    /// is the one part that needs a server.
    ///
    /// Ignored because the plugin is its own repository; run it after touching either side:
    ///
    /// ```text
    /// cargo test -p plugin-host -- --ignored
    /// ```
    #[test]
    #[ignore = "needs the islandora plugin checked out beside qrate"]
    fn the_islandora_plugin_checks_a_column_against_its_mapped_vocabularies() {
        const SERVER: &str = "https://islandora.example.org";
        let load = islandora_plugin;

        // Everything the column holds is already cached, so nothing here touches the network: the
        // point of the cache is that a re-validate after one edit costs no requests at all.
        let cached = json!({
            format!("terms:{SERVER}|subject"): { "Film": true, "Pottery": false },
            format!("terms:{SERVER}|genre"): { "Film": false, "Pottery": true },
        });

        let checked = |vocabularies: Json| {
            let plugin = load(cached.clone());
            assert_eq!(plugin.load_error(), None);
            plugin.set_scoped(json!({ "server": SERVER }), Json::Null);
            // The app's sub-delimiter, not the plugin's — it has none of its own any more.
            plugin.set_app_settings(";".into());
            let mut settings = ColumnSettings::default();
            settings
                .plugins
                .insert("islandora".into(), json!({ "vocabularies": vocabularies }));
            let column = ColumnInfo {
                name: "Subject",
                data_type: "Text",
                settings: &settings,
            };
            let values = ["Film; Pottery", "Film", ""];
            plugin.validate(&column, &values.map(SharedString::from))
        };

        let one = checked(json!(["subject"]));
        assert_eq!(one.len(), 1, "{one:?}");
        assert_eq!(one[0].0, 0, "a sub-delimited cell is checked term by term");
        assert_eq!(one[0].1, Severity::Error);
        assert_eq!(
            one[0].2,
            r#""Pottery" is not a term in the Islandora subject vocabulary"#
        );

        // Mapped to both, every value is held by one of them — the union is the rule, not the
        // intersection, or a column split across two vocabularies could never be clean.
        assert!(
            checked(json!(["subject", "genre"])).is_empty(),
            "{:?}",
            checked(json!(["subject", "genre"]))
        );

        // An unmapped column is every column the user has not touched, and must cost nothing.
        assert!(checked(Json::Null).is_empty());
    }

    /// The same plugin against a real Islandora, which is the half the offline tests cannot reach:
    /// that the URLs `api.lua` builds are ones Drupal answers, and that a name the site does not
    /// hold really is absent from the reply rather than merely unmarked.
    ///
    /// Ignored, so CI never depends on somebody's server being up. Run it by hand after touching
    /// `api.lua`:
    ///
    /// ```text
    /// cargo test -p plugin-host -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "hits a live Islandora"]
    fn the_islandora_plugin_agrees_with_a_real_server() {
        const SERVER: &str = "https://isle.dvnl.work";
        let plugin = islandora_plugin(Json::Null);
        plugin.set_scoped(json!({ "server": SERVER }), Json::Null);

        let mut settings = ColumnSettings::default();
        settings.plugins.insert(
            "islandora".into(),
            json!({ "vocabularies": ["resource_types"] }),
        );
        let column = ColumnInfo {
            name: "Type",
            data_type: "Text",
            settings: &settings,
        };
        let values = ["Text", "Nonsense Value", "Still Image"];
        let found = plugin.validate(&column, &values.map(SharedString::from));

        assert_eq!(found.len(), 1, "only the invented term is wrong: {found:?}");
        assert!(found[0].2.contains("Nonsense Value"), "{}", found[0].2);
        assert!(
            plugin.take_storage().is_some(),
            "the verdicts were cached, or every edit would re-ask the server"
        );

        let ask = |prefix: &str| {
            plugin
                .suggest(&CommandContext {
                    column: Some("Type".into()),
                    column_key: Some("c0".into()),
                    column_settings: json!({ "vocabularies": ["resource_types"] }),
                    row: Some(0),
                    values: Vec::new(),
                    argument: Some(prefix.into()),
                })
                .expect("suggesting does not raise")
        };

        let offered = ask("St");
        assert!(
            offered.iter().any(|item| item == "Still Image"),
            "prefix search came back with {offered:?}"
        );

        // Typing on must not cost another round trip: every longer prefix is a subset of what the
        // stem already fetched, and narrowing it happens in Lua. Timed rather than counted because
        // the difference is three orders of magnitude — anything near a network round trip here
        // means the memo stopped working and every keystroke is hitting the server again.
        let started = std::time::Instant::now();
        let narrowed = ask("Still");
        let elapsed = started.elapsed();
        assert!(
            narrowed.iter().any(|item| item == "Still Image"),
            "{narrowed:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(20),
            "narrowing a fetched stem took {elapsed:?}, so it went back to the server"
        );
    }

    /// The completion list is only ever offered for what the user is actually typing, and asking
    /// costs a request — so a prefix too short to narrow anything must not reach the network. Port
    /// 1 refuses instantly, so a slip here fails loudly rather than hanging.
    #[test]
    #[ignore = "needs the islandora plugin checked out beside qrate"]
    fn the_islandora_plugin_stays_quiet_until_there_is_a_prefix_worth_asking_about() {
        let plugin = islandora_plugin(Json::Null);
        plugin.set_scoped(json!({ "server": "http://127.0.0.1:1" }), Json::Null);

        let ask = |prefix: &str, mapped: Json| {
            plugin
                .suggest(&CommandContext {
                    column: Some("Subject".into()),
                    column_key: Some("c0".into()),
                    column_settings: json!({ "vocabularies": mapped }),
                    row: Some(0),
                    values: Vec::new(),
                    argument: Some(prefix.into()),
                })
                .expect("suggesting does not raise")
        };

        assert!(
            ask("F", json!(["subject"])).is_empty(),
            "one letter is not a question"
        );
        assert!(
            ask("Film", Json::Null).is_empty(),
            "an unmapped column offers nothing"
        );
        // Mapped and long enough, it does ask — and gets nothing, because nothing is listening.
        assert!(ask("Film", json!(["subject"])).is_empty());
    }

    /// If the interrupt hook is not wired up this test hangs rather than failing, which is exactly
    /// what it is here to prevent happening to the app. Returning at all is the assertion; the
    /// "ran too long" message goes to the log, since a runaway loop is the author's bug.
    #[test]
    fn a_runaway_plugin_is_cut_off() {
        let source = r#"return { validate = function() while true do end end }"#;
        assert!(check(source, &["x"]).is_empty());
    }
}

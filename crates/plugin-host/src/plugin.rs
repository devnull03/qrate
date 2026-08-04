//! One Lua plugin: one virtual machine, implementing the same [`ColumnValidator`] trait a
//! compiled-in Rust validator does.
//!
//! That is the whole trick. The host is not a new subsystem sitting beside the validator
//! registry — it is another entry *in* it, so the diagnostics store, the replace-by-source
//! invalidation, the Problems panel, and the in-cell squiggle all work with no change.
//!
//! Settings cross as opaque JSON in both directions. The host never reads into a plugin's object,
//! which is what lets a plugin add a knob without a qrate release.
//
// ponytail: `validate` runs Lua on the UI thread inside `Validators::run`. Move plugins onto a
// worker with a request generation counter when one is slow enough to drop a frame.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use diagnostics::{ColumnInfo, ColumnValidator, Severity};
use gpui::SharedString;
use mlua::{Function, Lua, LuaOptions, LuaSerdeExt as _, StdLib, Table, VmState};
use plugin_api::{CommandContext, MenuItem, MenuTarget, SettingKind, SettingScope, SettingSpec};
use serde_json::Value as Json;

/// Enough for a vocabulary list or a compiled pattern set, far short of a runaway allocation.
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// How long one call into Lua may run before the interrupt hook kills it. A plugin that spins
/// forever would otherwise wedge the UI thread with no way out but Task Manager.
const DEADLINE: Duration = Duration::from_millis(250);

/// The host owns the timeout, not the plugin, so a hung server cannot pin the UI thread longer
/// than this. The interrupt hook cannot help here — it only runs between Lua instructions, and a
/// blocking host call is one instruction.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// A validator runs per column edit, so a plugin that fetches on every call would hammer a server
/// as fast as the user types. Generous for a check-the-server plugin, far short of a flood.
const HTTP_BUDGET: u32 = 30;
const HTTP_WINDOW: Duration = Duration::from_secs(60);

/// What a command asked the host to store, by scope. `None` means "leave that scope alone";
/// `Some` replaces the plugin's whole object there, which is the same replace-the-set rule
/// diagnostics use and saves inventing merge semantics.
#[derive(Default, Debug, PartialEq)]
pub struct Writes {
    pub column: Option<Json>,
    pub project: Option<Json>,
    pub user: Option<Json>,
}

struct Loaded {
    lua: Lua,
    /// Overrides the file-derived id when the descriptor declares one.
    name: Option<SharedString>,
    description: Option<SharedString>,
    validate: Option<Function>,
    on_command: Option<Function>,
    menu: Vec<MenuItem>,
    settings: Vec<SettingSpec>,
}

pub struct LuaPlugin {
    /// The file or folder the plugin was found in, and its identity unless the descriptor renames
    /// it. Kept even when the descriptor does, so a plugin that fails to load still reports under
    /// the name its author can find on disk.
    id: SharedString,
    /// Shared with the interrupt hook, and pushed forward before every call into Lua.
    deadline: Rc<Cell<Instant>>,
    /// `(project, user)`, held here because `validate` runs without an `App` to read them from.
    /// Behind a `RefCell` so a write can refresh the copy in place — reloading the VM to pick up a
    /// changed setting would re-run every `init.lua` on every keystroke in the Settings window.
    scoped: RefCell<(Json, Json)>,
    /// A load failure is kept rather than dropped, so a broken plugin can still report itself.
    state: Result<Loaded, String>,
}

impl LuaPlugin {
    /// Build a plugin from its source text and the id its file or folder gives it. Taking the
    /// source rather than a path is what lets the tests cover every path without touching the
    /// filesystem.
    pub fn load(id: &str, source: &str) -> Self {
        let deadline = Rc::new(Cell::new(Instant::now()));
        Self {
            id: id.to_string().into(),
            state: build(source, &deadline).map_err(|err| err.to_string()),
            scoped: RefCell::new((Json::Null, Json::Null)),
            deadline,
        }
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
        *self.scoped.borrow_mut() = (project, user);
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
        table.set("column", ctx.column.as_ref())?;
        table.set("row", ctx.row.map(|r| r + 1))?;
        table.set(
            "values",
            lua.create_sequence_from(ctx.values.iter().map(SharedString::as_ref))?,
        )?;
        table.set("settings", self.settings_table(lua, &ctx.column_settings)?)?;

        self.deadline.set(Instant::now() + DEADLINE);
        let written: Option<Table> = on_command.call((command.as_ref(), table))?;
        let Some(written) = written else {
            return Ok(Writes::default());
        };
        Ok(Writes {
            column: json_field(lua, &written, "column")?,
            project: json_field(lua, &written, "project")?,
            user: json_field(lua, &written, "user")?,
        })
    }

    /// `{ column = …, project = …, user = … }` — this plugin's own objects and nothing else's.
    fn settings_table(&self, lua: &Lua, column: &Json) -> mlua::Result<Table> {
        let table = lua.create_table()?;
        let scoped = self.scoped.borrow();
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

        self.deadline.set(Instant::now() + DEADLINE);
        let found: Vec<Table> = validate.call((info, cells, settings))?;

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
        let result = self
            .state
            .as_ref()
            .map_err(String::clone)
            .and_then(|loaded| {
                let Some(validate) = loaded.validate.as_ref() else {
                    return Ok(Vec::new());
                };
                self.call_validate(loaded, validate, column, values)
                    .map_err(|err| err.to_string())
            });

        match result {
            Ok(found) => found,
            // ponytail: a plugin's own failure is filed at row 0 because `ColumnValidator` returns
            // `usize`, not `Option<usize>`. Widen the trait once something else needs a
            // dataset-level finding. Visible and wrong beats invisible.
            Err(err) => vec![(0, Severity::Error, format!("{}: {err}", self.id).into())],
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

fn build(source: &str, deadline: &Rc<Cell<Instant>>) -> mlua::Result<Loaded> {
    // Luau has no `io` and no `package` to remove, and its `debug` is already cut to two
    // functions. Leaving out `OS` and `COROUTINE` costs a plugin its clock and its ability to
    // yield, so every capability one ever gets has to arrive as a host function the host can
    // refuse.
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH,
        LuaOptions::default(),
    )?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    install_http(&lua, deadline)?;
    lua.sandbox(true)?;
    lua.set_interrupt({
        let deadline = deadline.clone();
        move |_| {
            if Instant::now() > deadline.get() {
                Err(mlua::Error::runtime("ran too long and was stopped"))
            } else {
                Ok(VmState::Continue)
            }
        }
    });

    deadline.set(Instant::now() + DEADLINE);
    let descriptor: Table = lua.load(source).eval()?;
    let validate: Option<Function> = descriptor.get("validate")?;
    let on_command: Option<Function> = descriptor.get("on_command")?;
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
    if validate.is_none() && on_command.is_none() {
        return Err(mlua::Error::runtime(
            "the returned table has neither `validate` nor `on_command`",
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
        menu,
        settings,
    })
}

/// `qrate.http.get(url)` -> `{ status, body }`, or `nil, message`. Returning the failure rather
/// than raising it is what lets a plugin report an unreachable server as a finding instead of as
/// its own crash. Installed before `sandbox`, which freezes the global table. Rate limited per VM,
/// so one plugin's loop cannot spend another's budget.
//
// ponytail: GET only, no allowlist, no cache — the network permission prompt is ASNT-59's job.
// Add `post` and storage when a plugin needs to write rather than check.
fn install_http(lua: &Lua, deadline: &Rc<Cell<Instant>>) -> mlua::Result<()> {
    let deadline = deadline.clone();
    let budget = Cell::new((Instant::now(), 0u32));
    let get = lua.create_function(move |lua, url: String| {
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
        let response = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .and_then(|client| client.get(&url).send());
        // Waiting on a server is not the script running long, so the compute budget restarts here
        // rather than the interrupt hook killing the plugin the moment the response lands.
        deadline.set(Instant::now() + DEADLINE);
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
    })?;
    let http = lua.create_table()?;
    http.set("get", get)?;
    let qrate = lua.create_table()?;
    qrate.set("http", http)?;
    lua.globals().set("qrate", qrate)
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

fn setting_spec(item: Table) -> mlua::Result<SettingSpec> {
    let key: String = item.get("key")?;
    let label: String = item.get("label")?;
    let scope: String = item.get("scope")?;
    let kind: String = item.get("type")?;
    Ok(SettingSpec {
        key: key.into(),
        label: label.into(),
        description: item.get::<Option<String>>("description")?.map(Into::into),
        scope: SettingScope::from_key(&scope)
            .ok_or_else(|| mlua::Error::runtime(format!("unknown setting scope {scope:?}")))?,
        kind: SettingKind::from_key(&kind)
            .ok_or_else(|| mlua::Error::runtime(format!("unknown setting type {kind:?}")))?,
    })
}

#[cfg(test)]
mod tests {
    use crate::{LuaPlugin, Writes};
    use diagnostics::{ColumnInfo, ColumnValidator, Severity};
    use gpui::SharedString;
    use plugin_api::{CommandContext, MenuTarget, SettingKind, SettingScope};
    use serde_json::{Value as Json, json};
    use settings::columns::ColumnSettings;

    fn plugin(source: &str) -> LuaPlugin {
        LuaPlugin::load("test", source)
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
        let source = r#"
            return {
              validate = function(_, _)
                local first, last
                for i = 1, 31 do
                  local _, err = qrate.http.get("nonsense")
                  if i == 1 then first = err end
                  last = err
                end
                return { { row = 1, message = first }, { row = 2, message = last } }
              end,
            }
        "#;
        let found = check(source, &["x", "y"]);
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

    #[test]
    fn an_unknown_setting_type_stops_the_plugin_loading() {
        let source = r#"
            return {
              settings = { { key = "k", label = "L", type = "slider", scope = "user" } },
              validate = function() return {} end,
            }
        "#;
        let found = check(source, &["x"]);
        assert_eq!(found.len(), 1);
        assert!(found[0].2.contains("slider"), "{:?}", found[0].2);
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
        let plugin = LuaPlugin::load("on-disk", source);
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
            assert_eq!(LuaPlugin::load("on-disk", source).name(), "on-disk");
        }
    }

    fn ctx(values: &[&str]) -> CommandContext {
        CommandContext {
            column: "Country".into(),
            column_key: "c0".into(),
            column_settings: Json::Null,
            row: None,
            values: values.iter().map(|v| SharedString::from(*v)).collect(),
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

    /// Every failure mode lands in the Problems panel as one row-0 error rather than a panic or
    /// silence — which is the difference between debuggable and not while authoring a plugin.
    #[test]
    fn a_plugin_that_will_not_load_reports_itself() {
        for source in [
            "return { validate = function(", // syntax error
            "return { }",                    // neither hook
            "return 42",                     // not a table
        ] {
            let found = check(source, &["x"]);
            assert_eq!(found.len(), 1, "{source:?}");
            assert_eq!(found[0].0, 0);
            assert_eq!(found[0].1, Severity::Error);
            assert!(found[0].2.starts_with("test: "), "{:?}", found[0].2);
        }
    }

    #[test]
    fn a_plugin_that_errors_mid_run_reports_itself_and_does_not_panic() {
        let source = r#"return { validate = function() error("boom") end }"#;
        let found = check(source, &["x"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, Severity::Error);
        assert!(found[0].2.contains("boom"));
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

    /// If the interrupt hook is not wired up this test hangs rather than failing, which is exactly
    /// what it is here to prevent happening to the app.
    #[test]
    fn a_runaway_plugin_is_cut_off() {
        let source = r#"return { validate = function() while true do end end }"#;
        let found = check(source, &["x"]);
        assert_eq!(found.len(), 1);
        assert!(found[0].2.contains("stopped"), "{:?}", found[0].2);
    }
}

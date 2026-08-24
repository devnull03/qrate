//! One-shot, read-only Luau programs for the agent bridge.
//!
//! This deliberately does not load through the user plugin registry. Each call gets a fresh VM,
//! an immutable snapshot userdata, no ambient I/O, and fixed resource/output budgets.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::SharedString;
use mlua::{
    Function, Lua, LuaOptions, LuaSerdeExt as _, StdLib, UserData, UserDataMethods, VmState,
};
use serde_json::Value as Json;

pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
pub const DEADLINE: Duration = Duration::from_secs(2);
pub const MAX_OUTPUT: usize = 16 * 1024;
const MAX_LOGS: usize = 20;
const MAX_LOG_BYTES: usize = 4 * 1024;
const MAX_TEXT_READ: usize = 64 * 1024;
const MAX_TEXT_FILE: u64 = 1024 * 1024;
const MAX_ERROR: usize = 512;

#[derive(Clone)]
pub struct AgentDiagnostic {
    pub row: Option<usize>,
    pub column: Option<String>,
    pub severity: String,
    pub source: String,
    pub message: String,
}

#[derive(Clone)]
pub struct AgentSnapshot {
    pub revision: u64,
    pub columns: Vec<SharedString>,
    pub rows: Vec<Vec<SharedString>>,
    pub selected_rows: Vec<usize>,
    pub diagnostics: Vec<AgentDiagnostic>,
    /// Resolved by qrate. Scripts never receive these paths; host methods are the only access.
    pub files: Vec<Option<PathBuf>>,
}

#[derive(Debug)]
pub struct AgentProgramOutput {
    pub value: Json,
    pub logs: Vec<String>,
    pub elapsed_ms: u64,
    pub output_bytes: usize,
}

#[derive(Debug)]
pub struct AgentProgramError {
    pub kind: &'static str,
    pub detail: String,
    pub elapsed_ms: u64,
}

impl std::fmt::Display for AgentProgramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

fn bounded_error(kind: &'static str, error: impl ToString, started: Instant) -> AgentProgramError {
    let mut detail = error.to_string().replace('\\', "/");
    if let Some(index) = detail.find("agent-scratch") {
        detail = detail[index..].to_string();
    }
    detail.truncate(MAX_ERROR);
    AgentProgramError {
        kind,
        detail,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

#[derive(Clone)]
struct Snapshot(Arc<AgentSnapshot>);

impl UserData for Snapshot {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("revision", |_, this, ()| Ok(this.0.revision));
        methods.add_method("row_count", |_, this, ()| Ok(this.0.rows.len()));
        methods.add_method("columns", |lua, this, ()| {
            lua.create_sequence_from(this.0.columns.iter().map(|value| value.as_ref()))
        });
        methods.add_method("selected_rows", |lua, this, ()| {
            lua.create_sequence_from(this.0.selected_rows.iter().copied())
        });
        methods.add_method("row", |lua, this, row: usize| {
            let values = this
                .0
                .rows
                .get(row)
                .ok_or_else(|| mlua::Error::runtime("row out of range"))?;
            let result = lua.create_table()?;
            result.set("index", row)?;
            for (column, value) in this.0.columns.iter().zip(values) {
                result.set(column.as_str(), value.as_str())?;
            }
            Ok(result)
        });
        methods.add_method("cell", |_, this, (row, column): (usize, String)| {
            let index = this
                .0
                .columns
                .iter()
                .position(|name| name == &column)
                .ok_or_else(|| mlua::Error::runtime("unknown column"))?;
            Ok(this
                .0
                .rows
                .get(row)
                .and_then(|values| values.get(index))
                .map(ToString::to_string))
        });
        methods.add_method("diagnostics", |lua, this, ()| {
            let result = lua.create_table()?;
            for (index, diagnostic) in this.0.diagnostics.iter().enumerate() {
                let item = lua.create_table()?;
                item.set("row", diagnostic.row)?;
                item.set("column", diagnostic.column.as_deref())?;
                item.set("severity", diagnostic.severity.as_str())?;
                item.set("source", diagnostic.source.as_str())?;
                item.set("message", diagnostic.message.as_str())?;
                result.set(index + 1, item)?;
            }
            Ok(result)
        });
        methods.add_method("file_info", |lua, this, row: usize| {
            let path = this
                .0
                .files
                .get(row)
                .and_then(Option::as_ref)
                .ok_or_else(|| mlua::Error::runtime("row has no linked file"))?;
            let metadata = fs::metadata(path).map_err(mlua::Error::external)?;
            let result = lua.create_table()?;
            result.set(
                "extension",
                path.extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default(),
            )?;
            result.set("bytes", metadata.len())?;
            result.set("has_pdf_text", preview::has_text_layer(path))?;
            Ok(result)
        });
        methods.add_method(
            "read_text",
            |_, this, (row, offset, limit): (usize, Option<usize>, Option<usize>)| {
                let path = this
                    .0
                    .files
                    .get(row)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| mlua::Error::runtime("row has no linked file"))?;
                if preview::has_text(path) {
                    return Err(mlua::Error::runtime("use pdf_search for PDF text"));
                }
                let metadata = fs::metadata(path).map_err(mlua::Error::external)?;
                if metadata.len() > MAX_TEXT_FILE {
                    return Err(mlua::Error::runtime(
                        "linked text file exceeds the 1 MiB policy limit",
                    ));
                }
                let mut bytes = Vec::with_capacity(metadata.len() as usize);
                File::open(path)
                    .and_then(|file| file.take(MAX_TEXT_FILE + 1).read_to_end(&mut bytes))
                    .map_err(mlua::Error::external)?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| mlua::Error::runtime("linked file is not UTF-8 text"))?;
                let offset = offset.unwrap_or(0).min(text.len());
                let mut start = offset;
                while !text.is_char_boundary(start) {
                    start += 1;
                }
                let requested = limit.unwrap_or(4096).min(MAX_TEXT_READ);
                let mut end = (start + requested).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                Ok(text[start..end].to_string())
            },
        );
        methods.add_method(
            "pdf_search",
            |lua, this, (row, needle, limit): (usize, String, Option<usize>)| {
                let path = this
                    .0
                    .files
                    .get(row)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| mlua::Error::runtime("row has no linked file"))?;
                let result = lua.create_table()?;
                for (index, found) in preview::search(path, &needle)
                    .into_iter()
                    .take(limit.unwrap_or(20).min(50))
                    .enumerate()
                {
                    let item = lua.create_table()?;
                    item.set("page", found.page)?;
                    let mut line = found.line;
                    line.truncate(512);
                    item.set("text", line)?;
                    result.set(index + 1, item)?;
                }
                Ok(result)
            },
        );
    }
}

fn lua(started: Instant) -> Result<Lua, AgentProgramError> {
    let deadline = Arc::new(Mutex::new(started + DEADLINE));
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|error| bounded_error("compile_error", error, started))?;
    lua.set_memory_limit(MEMORY_LIMIT)
        .map_err(|error| bounded_error("compile_error", error, started))?;
    lua.set_interrupt(move |_| {
        if Instant::now() > *deadline.lock().unwrap() {
            Err(mlua::Error::runtime("ran too long and was stopped"))
        } else {
            Ok(VmState::Continue)
        }
    });
    let math: mlua::Table = lua
        .globals()
        .get("math")
        .map_err(|error| bounded_error("compile_error", error, started))?;
    math.set("random", mlua::Value::Nil)
        .map_err(|error| bounded_error("compile_error", error, started))?;
    math.set("randomseed", mlua::Value::Nil)
        .map_err(|error| bounded_error("compile_error", error, started))?;
    lua.sandbox(true)
        .map_err(|error| bounded_error("compile_error", error, started))?;
    Ok(lua)
}

pub fn validate_agent_program(source: &str) -> Result<(), AgentProgramError> {
    let started = Instant::now();
    let lua = lua(started)?;
    let _: Function = lua
        .load(source)
        .set_name("agent-scratch")
        .eval()
        .map_err(|error| bounded_error("compile_error", error, started))?;
    Ok(())
}

pub fn run_agent_program(
    source: &str,
    args: &Json,
    snapshot: AgentSnapshot,
) -> Result<AgentProgramOutput, AgentProgramError> {
    let started = Instant::now();
    let logs = Arc::new(Mutex::new(Vec::<String>::new()));
    let lua = lua(started)?;
    lua.globals()
        .set(
            "print",
            lua.create_function({
                let logs = logs.clone();
                move |_, values: mlua::MultiValue| {
                    let mut line = values
                        .iter()
                        .map(|value| format!("{value:?}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    line.truncate(MAX_LOG_BYTES);
                    let mut logs = logs.lock().unwrap();
                    if logs.len() < MAX_LOGS {
                        logs.push(line);
                    }
                    Ok(())
                }
            })
            .map_err(|error| bounded_error("runtime_error", error, started))?,
        )
        .map_err(|error| bounded_error("runtime_error", error, started))?;

    let function: Function = lua
        .load(source)
        .set_name("agent-scratch")
        .eval()
        .map_err(|error| bounded_error("runtime_error", error, started))?;
    let argument = lua
        .to_value(args)
        .map_err(|error| bounded_error("runtime_error", error, started))?;
    let value: mlua::Value = function
        .call((Snapshot(Arc::new(snapshot)), argument))
        .map_err(|error| bounded_error("runtime_error", error, started))?;
    let value: Json = lua
        .from_value(value)
        .map_err(|error| bounded_error("runtime_error", error, started))?;
    let output_bytes = serde_json::to_vec(&value).map_or(MAX_OUTPUT + 1, |bytes| bytes.len());
    if output_bytes > MAX_OUTPUT {
        return Err(bounded_error(
            "output_too_large",
            "program output exceeds 16 KiB; return a smaller projection",
            started,
        ));
    }
    Ok(AgentProgramOutput {
        value,
        logs: Arc::try_unwrap(logs).map_or_else(
            |logs| logs.lock().unwrap().clone(),
            |logs| logs.into_inner().unwrap(),
        ),
        elapsed_ms: started.elapsed().as_millis() as u64,
        output_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> AgentSnapshot {
        AgentSnapshot {
            revision: 7,
            columns: vec!["Title".into()],
            rows: vec![vec!["Harvest".into()], vec!["Wharf".into()]],
            selected_rows: vec![1],
            diagnostics: Vec::new(),
            files: vec![None, None],
        }
    }

    #[test]
    fn program_reads_only_the_snapshot_api() {
        let output = run_agent_program(
            "return function(q, args) return { revision=q:revision(), title=q:cell(args.row, 'Title') } end",
            &serde_json::json!({"row": 1}), snapshot(),
        ).unwrap();
        assert_eq!(
            output.value,
            serde_json::json!({"revision": 7, "title": "Wharf"})
        );
    }

    #[test]
    fn ambient_io_is_absent() {
        let error = run_agent_program(
            "return function() return io.open('x') end",
            &Json::Null,
            snapshot(),
        )
        .unwrap_err();
        assert!(error.detail.contains("io"));
    }
}

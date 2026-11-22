//! Agent-written checks the archivist adopted: a saved scratch program that keeps running with the
//! project's other checks after the agent is gone, in the same read-only sandbox.

use ai::agent::{RequestError, TableRevision};
use capability::{Capability, Effect, Exposure, Invocation};
use diagnostics::GroupFix;
use diagnostics::{ColumnSnapshot, DATASET_MAIN, Location, Source};
use gpui::{App, Global, SharedString, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::agent::AgentProgram;

use super::table;

/// Where adopted checks are kept in the project.
const CHECKS_KEY: &str = "agent_checks";
/// Findings one check may publish per run.
const MAX_FINDINGS: usize = 500;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct AdoptedCheck {
    pub name: String,
    /// The agent that wrote it.
    pub agent: String,
    pub hash: String,
    #[schemars(skip)]
    #[serde(default)]
    pub source: String,
}

fn adopted(cx: &App) -> Vec<AdoptedCheck> {
    cx.try_global::<settings::project::CurrentProject>()
        .and_then(|project| {
            project
                .data
                .values
                .get(CHECKS_KEY)
                .map(|value| value.text())
        })
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(checks: &[AdoptedCheck], cx: &mut App) {
    settings::project::CurrentProject::set_text(
        CHECKS_KEY,
        serde_json::to_string(checks).unwrap_or_default().into(),
        cx,
    );
}

fn source_name(name: &str) -> SharedString {
    format!("check: {name}").into()
}

/// The check names published last run, so a removed check's findings are cleared too.
#[derive(Default)]
struct Published(Vec<SharedString>);
impl Global for Published {}

/// Run every adopted check against the live table, off the UI thread, and publish each one's
/// findings under its own source. Registered as an asynchronous validator.
fn run(_: &[ColumnSnapshot], cx: &mut App) {
    let idle = adopted(cx).is_empty()
        && cx
            .try_global::<Published>()
            .is_none_or(|published| published.0.is_empty());
    // Validation can start inside a table update; read the table once that update is over.
    if !idle {
        cx.defer(run_now);
    }
}

fn run_now(cx: &mut App) {
    let checks = adopted(cx);
    let names: Vec<SharedString> = checks
        .iter()
        .map(|check| source_name(&check.name))
        .collect();
    for gone in std::mem::replace(&mut cx.default_global::<Published>().0, names)
        .into_iter()
        .filter(|name| !checks.iter().any(|check| source_name(&check.name) == *name))
    {
        diagnostics::Diagnostics::set(&Source::Validator(gone), DATASET_MAIN, Vec::new(), cx);
    }
    let Ok(state) = table(cx) else {
        return;
    };
    let delegate = state.read(cx).delegate();
    let revision = TableRevision(delegate.values_generation());
    let snapshot = crate::agent::program_snapshot(revision, delegate, cx);
    for check in checks {
        let snapshot = snapshot.clone();
        let source = source_name(&check.name);
        diagnostics::SourceActions::register(&source, retire_actions, cx);
        let task = cx.background_executor().spawn(async move {
            plugin_host::run_agent_program(
                &check.source,
                &serde_json::json!({"mode": "check"}),
                snapshot,
            )
        });
        cx.spawn(async move |cx| {
            let output = task.await;
            cx.update(|cx| {
                let Ok(state) = table(cx) else {
                    return;
                };
                let delegate = state.read(cx).delegate();
                // A newer run will publish for the newer table.
                if delegate.values_generation() != revision.0 {
                    return;
                }
                let findings = match output {
                    Ok(output) => findings(&output.value, &source, delegate),
                    Err(error) => {
                        log::warn!("adopted check {source} failed: {}", error.detail);
                        vec![failure(&error.detail, &source)]
                    }
                };
                diagnostics::Diagnostics::set(
                    &Source::Validator(source),
                    DATASET_MAIN,
                    findings,
                    cx,
                );
            })
        })
        .detach();
    }
}

fn failure(detail: &str, source: &SharedString) -> diagnostics::Diagnostic {
    diagnostics::Diagnostic {
        location: Location {
            dataset: DATASET_MAIN.into(),
            row: None,
            row_id: None,
            column: None,
        },
        severity: diagnostics::Severity::Warning,
        source: Source::Validator(source.clone()),
        message: format!("This check could not run: {detail}").into(),
        group: None,
        filed: None,
    }
}

/// A check returns a list of `{row, column?, severity?, message}`.
fn findings(
    value: &serde_json::Value,
    source: &SharedString,
    delegate: &crate::QrateTableDelegate,
) -> Vec<diagnostics::Diagnostic> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_FINDINGS)
        .filter_map(|finding| {
            let message = finding["message"]
                .as_str()?
                .chars()
                .take(512)
                .collect::<String>();
            let row = finding["row"]
                .as_u64()
                .map(|row| row as usize)
                .filter(|row| *row < delegate.row_count());
            let column = finding["column"]
                .as_str()
                .filter(|column| delegate.data_col(column).is_some())
                .map(|column| SharedString::from(column.to_owned()));
            Some(diagnostics::Diagnostic {
                location: Location {
                    dataset: DATASET_MAIN.into(),
                    row,
                    row_id: row.and_then(|row| delegate.row_id(row)),
                    column,
                },
                severity: match finding["severity"].as_str() {
                    Some("error") => diagnostics::Severity::Error,
                    Some("note") => diagnostics::Severity::Note,
                    _ => diagnostics::Severity::Warning,
                },
                source: Source::Validator(source.clone()),
                message: message.into(),
                group: None,
                filed: None,
            })
        })
        .collect()
}

/// The Problems panel's action on an adopted check's findings: stop running it.
fn retire_actions(source: &str, _: &App) -> Vec<GroupFix> {
    let Some(name) = source.strip_prefix("check: ").map(str::to_owned) else {
        return Vec::new();
    };
    vec![GroupFix::action(
        format!("Stop running the check “{name}”"),
        move |cx| {
            let mut checks = adopted(cx);
            checks.retain(|check| check.name != name);
            save(&checks, cx);
            crate::revalidate_now(cx);
        },
    )]
}

/// Make the program you saved a project check.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct AdoptCheck {
    pub revision: TableRevision,
    /// Shown in the Problems panel, at most 64 characters. Adopting a name you used before
    /// replaces that check.
    pub name: String,
    /// Shown to the archivist: what the check looks for. At most 512 bytes.
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CheckList {
    pub checks: Vec<AdoptedCheck>,
}

pub struct AdoptCheckCap;

impl Capability for AdoptCheckCap {
    const NAME: &'static str = "adopt_check";
    const SUMMARY: &'static str = "Ask the archivist to keep the program you saved (program_save) as one of the project's checks. It must return a list of {row, column, severity, message} when args.mode is \"check\", and it runs in the same read-only sandbox whenever the table changes.";
    const GROUP: &'static str = "review";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Confirm;
    type Input = AdoptCheck;
    type Output = CheckList;

    fn revision(input: &AdoptCheck) -> Option<TableRevision> {
        Some(input.revision)
    }

    fn validate(input: &AdoptCheck) -> Result<(), RequestError> {
        let name = input.name.trim();
        match name.is_empty() || name.chars().count() > 64 {
            true => Err(RequestError::EditRefused(
                "a check needs a name of 1 to 64 characters".into(),
            )),
            false => Ok(()),
        }
    }

    fn reason(input: &AdoptCheck) -> Option<&str> {
        Some(&input.reason)
    }

    fn label(input: &AdoptCheck, _: &App) -> String {
        format!("Add the check “{}”", input.name.trim())
    }

    fn details(_: &AdoptCheck, cx: &App) -> Vec<String> {
        let Some(program) = cx.try_global::<AgentProgram>() else {
            return vec!["No program is saved.".into()];
        };
        let lines: Vec<&str> = program.source.lines().collect();
        std::iter::once("It runs this program, which can read the table but change nothing:".into())
            .chain(lines.iter().take(30).map(|line| format!("    {line}")))
            .chain((lines.len() > 30).then(|| format!("    … {} more lines", lines.len() - 30)))
            .collect()
    }

    fn outcome(output: &CheckList) -> String {
        format!("{} check(s) adopted", output.checks.len())
    }

    fn run(
        input: AdoptCheck,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<CheckList, RequestError>> {
        Task::ready((|| {
            let program = cx
                .try_global::<AgentProgram>()
                .ok_or(RequestError::ProgramUnavailable)?;
            let check = AdoptedCheck {
                name: input.name.trim().to_owned(),
                agent: call.agent().unwrap_or("archivist").to_owned(),
                hash: format!("{:x}", Sha256::digest(program.source.as_bytes())),
                source: program.source.clone(),
            };
            let mut checks = adopted(cx);
            checks.retain(|existing| existing.name != check.name);
            checks.push(check);
            save(&checks, cx);
            crate::revalidate_now(cx);
            log::info!("{}: {}", call.agent().unwrap_or("archivist"), call.label);
            Ok(CheckList { checks })
        })())
    }
}

pub struct ChecksCap;

impl Capability for ChecksCap {
    const NAME: &'static str = "checks";
    const SUMMARY: &'static str =
        "List the agent-written checks this project runs, with who wrote them.";
    const GROUP: &'static str = "review";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = ai::agent::NoInput;
    type Output = CheckList;

    fn label(_: &ai::agent::NoInput, _: &App) -> String {
        String::new()
    }

    fn run(
        _: ai::agent::NoInput,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<CheckList, RequestError>> {
        Task::ready(Ok(CheckList {
            checks: adopted(cx),
        }))
    }
}

pub(super) fn register(cx: &mut App) {
    capability::register::<AdoptCheckCap>(cx);
    capability::register::<ChecksCap>(cx);
    diagnostics::AsyncValidators::register("agent checks", run, cx);
}

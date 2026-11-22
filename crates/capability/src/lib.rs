//! What the app can do, declared once so the archivist, agents and generated docs share it.
//!
//! A feature is a [`Capability`]: a name, typed input and output with JSON Schemas, and two
//! required decisions, [`Capability::EXPOSURE`] and [`Capability::EFFECT`]. Neither has a default,
//! so a feature cannot be added without deciding whether an agent may use it and what it does to
//! the project. [`dispatch`] then enforces the effect for every call — the revision check, the
//! confirmation dialog, read grants, the one-undo-step rule and the agent's edit budget — so no
//! feature re-implements those rules and none can forget one. The catalog ([`catalog`]) is what
//! the CLI, the Pi extension and the reference docs are generated from.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use ai::agent::{MAX_MESSAGE_LEN, NoInput, RequestError, TableRevision};
use gpui::{App, Global, SharedString, Task};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use settings::history::Origin;

/// Who may use a capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    Agent,
    /// Only the archivist. `why` is published in the catalog, so withholding a feature from
    /// agents is a reviewed decision rather than an oversight.
    ArchivistOnly {
        why: &'static str,
    },
}

/// What a capability does to the project, which decides what [`dispatch`] requires of a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Changes nothing.
    Read,
    /// Publishes a draft only the archivist can apply. Must name the revision it read.
    Stage,
    /// Changes the project at once, as one undo step credited to the caller. Must name the
    /// revision it read.
    Edit,
    /// An [`Effect::Edit`] an agent may only make once the archivist confirms it, shown with the
    /// agent's [`Capability::reason`].
    Confirm,
    /// Leaves the project.
    Reach(Reach),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Files outside the project, behind the archivist's read grants.
    Files,
    /// A network host, behind the archivist's per-source permission.
    Network,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Stage => "stage",
            Effect::Edit => "edit",
            Effect::Confirm => "confirm",
            Effect::Reach(Reach::Files) => "reach_files",
            Effect::Reach(Reach::Network) => "reach_network",
        }
    }

    fn changes_project(self) -> bool {
        matches!(self, Effect::Edit | Effect::Confirm)
    }

    fn needs_revision(self) -> bool {
        matches!(self, Effect::Stage | Effect::Edit | Effect::Confirm)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    Archivist,
    /// The name an agent gave itself. A label, never proof: grants that matter are not keyed on it.
    Agent(SharedString),
}

/// One call, as a capability's [`Capability::run`] sees it.
pub struct Invocation {
    pub caller: Caller,
    /// [`Capability::label`] for this input: what History and the agent panel call the change.
    pub label: String,
    archivist: Origin,
}

impl Invocation {
    pub fn agent(&self) -> Option<&str> {
        match &self.caller {
            Caller::Agent(agent) => Some(agent),
            Caller::Archivist => None,
        }
    }

    /// The History origin for a change this call makes: credited to the agent with this call's
    /// label, or [`Capability::archivist_origin`] when the archivist made it.
    pub fn origin(&self) -> Origin {
        match &self.caller {
            Caller::Agent(agent) => Origin::Agent {
                agent: agent.to_string(),
                fix: self.label.clone(),
            },
            Caller::Archivist => self.archivist.clone(),
        }
    }
}

pub trait Capability: 'static {
    /// The wire name, such as `rename_column`. Public through the CLI: never rename one.
    const NAME: &'static str;
    /// One sentence for the catalog, starting with a verb.
    const SUMMARY: &'static str;
    /// Groups the catalog: `project`, `table`, `stage`, `files`, `history`, `media`, …
    const GROUP: &'static str;
    const EXPOSURE: Exposure;
    const EFFECT: Effect;
    /// The `result` tag of the answer.
    const RESULT: &'static str = Self::NAME;
    /// For [`Effect::Confirm`]: whether the dialog's button deletes something.
    const DESTRUCTIVE: bool = false;
    /// The input quotes every cell it judged, and the capability drops the ones that changed, so a
    /// call read at an older revision is still sound. The revision is still required.
    const CHECKS_OWN_CELLS: bool = false;

    type Input: DeserializeOwned + JsonSchema;
    type Output: Serialize + JsonSchema;

    /// The revision the input was read at. Required for staging and edits: [`dispatch`] refuses a
    /// call to one that returns `None`.
    fn revision(_input: &Self::Input) -> Option<TableRevision> {
        None
    }

    fn validate(_input: &Self::Input) -> Result<(), RequestError> {
        Ok(())
    }

    /// What History, the agent panel and a confirmation dialog call this change.
    fn label(input: &Self::Input, cx: &App) -> String;

    /// For [`Effect::Confirm`]: the agent's reason, shown to the archivist.
    fn reason(_input: &Self::Input) -> Option<&str> {
        None
    }

    /// For [`Effect::Confirm`]: an A2UI surface the agent sent to show in the dialog.
    fn surface(_input: &Self::Input) -> Option<Value> {
        None
    }

    /// For [`Effect::Confirm`]: a few lines naming what will change, such as row titles.
    fn details(_input: &Self::Input, _cx: &App) -> Vec<String> {
        Vec::new()
    }

    /// For [`Effect::Reach`] with [`Reach::Network`]: where the call goes, named for the
    /// archivist, such as "Library of Congress Subject Headings (id.loc.gov)". The first call to
    /// each place asks them.
    fn destination(_input: &Self::Input) -> Option<String> {
        None
    }

    /// Every [`GrantedPath`] in the input, so [`dispatch`] can check each one before `run`.
    fn paths(_input: &mut Self::Input) -> Vec<&mut GrantedPath> {
        Vec::new()
    }

    /// The History origin when the archivist makes this change from a menu.
    fn archivist_origin() -> Origin {
        Origin::Structure
    }

    /// A short note on the answer for the agent panel: sizes, never archive data.
    fn outcome(_output: &Self::Output) -> String {
        "answered".into()
    }

    fn run(
        input: Self::Input,
        call: &Invocation,
        cx: &mut App,
    ) -> Task<Result<Self::Output, RequestError>>;
}

/// A path from an agent, usable only once [`dispatch`] has checked it is inside the project or
/// covered by one of the archivist's read grants.
///
/// The check cannot be skipped: [`GrantedPath::path`] fails closed until it has passed, and only
/// this crate can mark it passed.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GrantedPath {
    raw: String,
    #[serde(skip)]
    resolved: Option<PathBuf>,
}

impl GrantedPath {
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn path(&self) -> Result<&Path, RequestError> {
        self.resolved
            .as_deref()
            .ok_or_else(|| RequestError::FileAccessRequired(self.raw.clone()))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn checked(path: PathBuf) -> Self {
        Self {
            raw: path.display().to_string(),
            resolved: Some(path),
        }
    }
}

impl JsonSchema for GrantedPath {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "GrantedPath".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "A file path: relative to the project folder, or absolute. Outside the project it must be covered by a read grant (request_access)."
        })
    }
}

/// What a confirmation dialog shows.
pub struct Confirmation {
    pub agent: SharedString,
    /// Whether the button deletes something.
    pub destructive: bool,
    /// An A2UI `createSurface` message the agent sent to show alongside its reason, such as a table
    /// of what will change. Drawn inside a frame marked as the agent's; the buttons stay qrate's.
    pub surface: Option<Value>,
    pub title: String,
    pub reason: String,
    pub details: Vec<String>,
}

/// The project state [`dispatch`] needs, provided by the crate that owns the table.
pub trait Host: 'static {
    /// The table's revision, or `None` when no table is open.
    fn revision(&self, cx: &App) -> Option<TableRevision>;
    /// A mark in the undo history to settle back to.
    fn history_mark(&self, cx: &App) -> usize;
    /// Fold every undo step pushed since `mark` into one.
    fn settle_history(&self, mark: usize, cx: &mut App);
    /// Ask the archivist; resolves once they answer. One question at a time.
    fn confirm(&self, ask: Confirmation, cx: &mut App) -> Task<Result<(), RequestError>>;
    /// Resolve `raw` against the project, refusing a path outside it that no read grant held by
    /// `agent` covers. Never asks.
    fn resolve_path(
        &self,
        agent: Option<&str>,
        raw: &str,
        cx: &App,
    ) -> Result<PathBuf, RequestError>;
}

struct HostHandle(Rc<dyn Host>);
impl Global for HostHandle {}

pub fn set_host(host: impl Host, cx: &mut App) {
    cx.set_global(HostHandle(Rc::new(host)));
}

/// The places on the network the archivist let each agent reach, until qrate closes.
#[derive(Default)]
struct NetworkGrants(std::collections::HashSet<(SharedString, String)>);
impl Global for NetworkGrants {}

/// Changes one agent may make per minute. Generous for a person-paced agent; it stops a runaway
/// script, not real work.
pub const EDITS_PER_MINUTE: usize = 120;

#[derive(Default)]
struct Budgets(HashMap<SharedString, VecDeque<Instant>>);
impl Global for Budgets {}

fn spend(agent: &SharedString, cx: &mut App) -> Result<(), RequestError> {
    let now = Instant::now();
    let budgets = cx.default_global::<Budgets>();
    let spent = budgets.0.entry(agent.clone()).or_default();
    while spent
        .front()
        .is_some_and(|at| now.duration_since(*at) > Duration::from_secs(60))
    {
        spent.pop_front();
    }
    if spent.len() >= EDITS_PER_MINUTE {
        return Err(RequestError::RateLimited);
    }
    spent.push_back(now);
    Ok(())
}

/// One capability in the catalog.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Descriptor {
    pub name: String,
    pub group: String,
    pub summary: String,
    /// `read`, `stage`, `edit`, `confirm`, `reach_files` or `reach_network`.
    pub effect: String,
    /// `agent` or `archivist_only`.
    pub exposure: String,
    /// Why an `archivist_only` capability is withheld from agents.
    pub why: Option<String>,
    /// The `result` tag of an answer.
    pub result: String,
    /// JSON Schema of the parameters.
    pub input: Value,
    /// JSON Schema of the answer, without the `revision` and `result` every answer carries.
    pub output: Value,
}

fn schema<T: JsonSchema>() -> Value {
    let mut settings = schemars::generate::SchemaSettings::draft07();
    settings.inline_subschemas = true;
    let mut value = serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .unwrap_or_default();
    if let Some(object) = value.as_object_mut() {
        object.remove("$schema");
        object.remove("title");
    }
    value
}

trait Erased {
    fn describe(&self) -> Descriptor;
    fn call(&self, params: Value, caller: Caller, cx: &mut App) -> Dispatched;
}

struct Entry<C>(PhantomData<C>);

impl<C: Capability> Erased for Entry<C> {
    fn describe(&self) -> Descriptor {
        let (exposure, why) = match C::EXPOSURE {
            Exposure::Agent => ("agent", None),
            Exposure::ArchivistOnly { why } => ("archivist_only", Some(why.to_owned())),
        };
        Descriptor {
            name: C::NAME.into(),
            group: C::GROUP.into(),
            summary: C::SUMMARY.into(),
            effect: C::EFFECT.name().into(),
            exposure: exposure.into(),
            why,
            result: C::RESULT.into(),
            input: schema::<C::Input>(),
            output: schema::<C::Output>(),
        }
    }

    fn call(&self, params: Value, caller: Caller, cx: &mut App) -> Dispatched {
        let params = match params {
            Value::Null => json!({}),
            params => params,
        };
        let input = match serde_json::from_value::<C::Input>(params) {
            Ok(input) => input,
            Err(error) => {
                return Dispatched {
                    label: String::new(),
                    task: Task::ready(Err(RequestError::MalformedParams(error.to_string()))),
                };
            }
        };
        let label = C::label(&input, cx);
        let task = match now(invoke::<C>(input, caller, cx)) {
            Ok(output) => Task::ready(
                output
                    .map(|output| envelope(C::RESULT, &output, revision(cx), C::outcome(&output))),
            ),
            Err(task) => cx.spawn(async move |cx| {
                let output = task.await?;
                Ok(cx.update(|cx| envelope(C::RESULT, &output, revision(cx), C::outcome(&output))))
            }),
        };
        Dispatched { label, task }
    }
}

fn envelope<T: Serialize>(
    result: &str,
    output: &T,
    revision: Option<TableRevision>,
    outcome: String,
) -> Answer {
    let mut value = match serde_json::to_value(output).unwrap_or_default() {
        Value::Object(map) => map,
        other => serde_json::Map::from_iter([("value".to_owned(), other)]),
    };
    value.insert("result".into(), result.into());
    value.insert("revision".into(), revision.unwrap_or_default().0.into());
    Answer {
        value: Value::Object(value),
        outcome,
    }
}

fn revision(cx: &App) -> Option<TableRevision> {
    cx.try_global::<HostHandle>()?.0.revision(cx)
}

#[derive(Default)]
struct Registry(BTreeMap<&'static str, Rc<dyn Erased>>);
impl Global for Registry {}

/// Make `C` callable. Registering the same name twice is a bug: the second registration panics
/// in debug builds and is ignored in release.
pub fn register<C: Capability>(cx: &mut App) {
    let registry = cx.default_global::<Registry>();
    if registry.0.contains_key(C::NAME) {
        debug_assert!(false, "capability {} registered twice", C::NAME);
        return;
    }
    registry.0.insert(C::NAME, Rc::new(Entry::<C>(PhantomData)));
}

/// Every registered capability, sorted by name.
pub fn catalog(cx: &App) -> Vec<Descriptor> {
    cx.try_global::<Registry>()
        .map(|registry| registry.0.values().map(|entry| entry.describe()).collect())
        .unwrap_or_default()
}

/// A successful call: the JSON answer and a short note on it for the agent panel.
pub struct Answer {
    pub value: Value,
    pub outcome: String,
}

/// A call under way. `label` is empty when the parameters did not parse.
pub struct Dispatched {
    pub label: String,
    pub task: Task<Result<Answer, RequestError>>,
}

/// Call a capability by name with JSON parameters: the one way in for the CLI and agents.
pub fn dispatch(name: &str, params: Value, caller: Caller, cx: &mut App) -> Dispatched {
    match cx
        .try_global::<Registry>()
        .and_then(|registry| registry.0.get(name).cloned())
    {
        Some(entry) => entry.call(params, caller, cx),
        None => Dispatched {
            label: String::new(),
            task: Task::ready(Err(RequestError::UnknownCapability(name.to_owned()))),
        },
    }
}

/// Call `C` with typed input. Everything [`dispatch`] enforces happens here.
pub fn invoke<C: Capability>(
    mut input: C::Input,
    caller: Caller,
    cx: &mut App,
) -> Task<Result<C::Output, RequestError>> {
    let agent = match &caller {
        Caller::Agent(agent) => Some(agent.clone()),
        Caller::Archivist => None,
    };
    let prepared = (|| {
        if let (Exposure::ArchivistOnly { why }, Some(_)) = (C::EXPOSURE, &agent) {
            return Err(RequestError::ArchivistOnly(why.to_owned()));
        }
        C::validate(&input)?;
        let host = cx
            .try_global::<HostHandle>()
            .map(|host| host.0.clone())
            .ok_or(RequestError::ProjectUnavailable)?;
        match (C::revision(&input), C::EFFECT.needs_revision()) {
            (None, true) => {
                return Err(RequestError::MalformedParams("revision is required".into()));
            }
            (Some(judged), _) => match host.revision(cx) {
                None => return Err(RequestError::TableUnavailable),
                Some(current) if current != judged && !C::CHECKS_OWN_CELLS => {
                    return Err(RequestError::StaleRevision);
                }
                Some(_) => {}
            },
            (None, false) => {}
        }
        for path in C::paths(&mut input) {
            path.resolved = Some(host.resolve_path(agent.as_deref(), &path.raw, cx)?);
        }
        let reason = match (C::EFFECT, &agent) {
            (Effect::Reach(Reach::Network), Some(agent)) => {
                let place = C::destination(&input).ok_or(RequestError::MalformedParams(
                    "no destination for a network call".into(),
                ))?;
                let granted = cx
                    .try_global::<NetworkGrants>()
                    .is_some_and(|grants| grants.0.contains(&(agent.clone(), place.clone())));
                (!granted).then(|| {
                    C::reason(&input)
                        .map(str::trim)
                        .filter(|reason| !reason.is_empty())
                        .unwrap_or("to answer your request")
                        .chars()
                        .take(MAX_MESSAGE_LEN)
                        .collect()
                })
            }
            (Effect::Confirm, Some(_)) => {
                let reason = C::reason(&input).unwrap_or_default().trim().to_owned();
                if reason.is_empty() {
                    return Err(RequestError::ReasonMissing);
                }
                if reason.len() > MAX_MESSAGE_LEN {
                    return Err(RequestError::ReasonTooLong);
                }
                Some(reason)
            }
            _ => None,
        };
        if let Some(agent) = &agent
            && (C::EFFECT.changes_project() || C::EFFECT == Effect::Reach(Reach::Network))
        {
            spend(agent, cx)?;
        }
        Ok((host, reason))
    })();
    let (host, reason) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return Task::ready(Err(error)),
    };
    let call = Invocation {
        caller,
        label: C::label(&input, cx),
        archivist: C::archivist_origin(),
    };
    let (Some(agent), Some(reason)) = (agent, reason) else {
        return run::<C>(input, call, host, cx);
    };
    let judged = C::revision(&input);
    let place = match C::EFFECT {
        Effect::Reach(Reach::Network) => C::destination(&input),
        _ => None,
    };
    let asked = host.confirm(
        Confirmation {
            agent: agent.clone(),
            destructive: C::DESTRUCTIVE,
            surface: C::surface(&input),
            title: match &place {
                Some(place) => format!("Look things up in {place}"),
                None => call.label.clone(),
            },
            reason,
            details: C::details(&input, cx),
        },
        cx,
    );
    // A refusal that needs no answer, such as another confirmation already waiting, comes back now.
    let asked = match now(asked) {
        Ok(Err(error)) => return Task::ready(Err(error)),
        Ok(Ok(())) => Task::ready(Ok(())),
        Err(asked) => asked,
    };
    cx.spawn(async move |cx| {
        asked.await?;
        let task = cx.update(|cx| {
            if let Some(place) = place {
                cx.default_global::<NetworkGrants>()
                    .0
                    .insert((agent, place));
            }
            // The table may have changed while the archivist read the dialog.
            if judged.is_some() && host.revision(cx) != judged {
                return Task::ready(Err(RequestError::StaleRevision));
            }
            run::<C>(input, call, host, cx)
        });
        task.await
    })
}

fn run<C: Capability>(
    input: C::Input,
    call: Invocation,
    host: Rc<dyn Host>,
    cx: &mut App,
) -> Task<Result<C::Output, RequestError>> {
    let mark = C::EFFECT.changes_project().then(|| host.history_mark(cx));
    let task = C::run(input, &call, cx);
    let Some(mark) = mark else {
        return task;
    };
    match now(task) {
        Ok(output) => {
            host.settle_history(mark, cx);
            Task::ready(output)
        }
        Err(task) => cx.spawn(async move |cx| {
            let output = task.await;
            cx.update(|cx| host.settle_history(mark, cx));
            output
        }),
    }
}

/// A task's output if it has already finished, so a call that completes at once is answered
/// without a hop through the executor; the task itself otherwise.
pub fn now<T: 'static>(task: Task<T>) -> Result<T, Task<T>> {
    if !task.is_ready() {
        return Err(task);
    }
    let mut task = std::pin::pin!(task);
    match std::future::Future::poll(
        task.as_mut(),
        &mut std::task::Context::from_waker(std::task::Waker::noop()),
    ) {
        std::task::Poll::Ready(output) => Ok(output),
        std::task::Poll::Pending => unreachable!("a ready task is pending"),
    }
}

/// The catalog itself, so an agent can discover every capability with one call.
pub struct Capabilities;

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CatalogOutput {
    pub capabilities: Vec<Descriptor>,
}

impl Capability for Capabilities {
    const NAME: &'static str = "capabilities";
    const SUMMARY: &'static str =
        "List every capability with its effect and the JSON Schemas of its parameters and answer.";
    const GROUP: &'static str = "project";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = NoInput;
    type Output = CatalogOutput;

    fn label(_: &NoInput, _: &App) -> String {
        String::new()
    }

    fn outcome(output: &CatalogOutput) -> String {
        format!("{} capabilities", output.capabilities.len())
    }

    fn run(_: NoInput, _: &Invocation, cx: &mut App) -> Task<Result<CatalogOutput, RequestError>> {
        Task::ready(Ok(CatalogOutput {
            capabilities: catalog(cx),
        }))
    }
}

pub fn init(cx: &mut App) {
    register::<Capabilities>(cx);
}

#[cfg(test)]
mod tests;

/// The catalog as the reference page `docs/cli/capabilities.md`, grouped and sorted so a change to
/// any capability shows as a diff to that page.
pub fn markdown(catalog: &[Descriptor]) -> String {
    let mut groups: BTreeMap<&str, Vec<&Descriptor>> = BTreeMap::new();
    for descriptor in catalog {
        groups
            .entry(&descriptor.group)
            .or_default()
            .push(descriptor);
    }
    let mut out = String::from(
        "# Capability reference\n\n\
         <!-- Generated from qrate's capability catalog. Do not edit: run \
         `QRATE_BLESS=1 cargo test -p app capability_reference` after changing a capability. -->\n\n\
         Everything an agent can ask qrate to do. Call any of these with \
         `qrate agent call <name>` and the parameters as JSON on stdin; `qrate agent capabilities` \
         prints this list with full JSON Schemas. Every answer also carries `revision` and \
         `result`.\n\n\
         | Effect | What it means |\n|---|---|\n\
         | `read` | Changes nothing. |\n\
         | `stage` | A draft the archivist applies from qrate. Needs the `revision` you read. |\n\
         | `edit` | Changes the project at once, as one undo step credited to you. Needs the `revision`. |\n\
         | `confirm` | Waits for the archivist to confirm it in qrate, showing your `reason`. |\n\
         | `reach_files` / `reach_network` | Leaves the project, behind the archivist's permission. |\n",
    );
    for (group, descriptors) in groups {
        out.push_str(&format!("\n## {group}\n"));
        for descriptor in descriptors {
            out.push_str(&format!(
                "\n### `{}`\n\n{}\n\nEffect: `{}`. Answer: `{}`.",
                descriptor.name, descriptor.summary, descriptor.effect, descriptor.result
            ));
            if let Some(why) = &descriptor.why {
                out.push_str(&format!(" Archivist only: {why}."));
            }
            out.push('\n');
            let properties = descriptor.input["properties"].as_object();
            let required: Vec<&str> = descriptor.input["required"]
                .as_array()
                .map(|names| names.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let Some(properties) = properties.filter(|properties| !properties.is_empty()) else {
                out.push_str("\nNo parameters.\n");
                continue;
            };
            out.push_str("\n| Parameter | Type | Required | Description |\n|---|---|---|---|\n");
            for (name, schema) in properties {
                let kind = schema["type"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        schema["type"].as_array().map(|kinds| {
                            kinds
                                .iter()
                                .filter_map(Value::as_str)
                                .filter(|kind| *kind != "null")
                                .collect::<Vec<_>>()
                                .join(" or ")
                        })
                    })
                    .unwrap_or_else(|| "object".into());
                let description = schema["description"]
                    .as_str()
                    .unwrap_or_default()
                    .replace('\n', " ")
                    .replace('|', "\\|");
                out.push_str(&format!(
                    "| `{name}` | {kind} | {} | {description} |\n",
                    match required.contains(&name.as_str()) {
                        true => "yes",
                        false => "no",
                    }
                ));
            }
        }
    }
    out
}

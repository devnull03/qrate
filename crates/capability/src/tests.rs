use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use ai::agent::{NoInput, RequestError, TableRevision};
use gpui::{App, Task, TestAppContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Caller, Capability, Confirmation, EDITS_PER_MINUTE, Effect, Exposure, GrantedPath, Host,
    Invocation, Reach, catalog, dispatch, register, set_host,
};

#[derive(Default)]
struct State {
    revision: u64,
    pushed: usize,
    settled: Vec<usize>,
    approve: bool,
    asked: Vec<String>,
    ran: Vec<String>,
}

#[derive(Clone, Default)]
struct Mock(Rc<RefCell<State>>);

impl Host for Mock {
    fn revision(&self, _: &App) -> Option<TableRevision> {
        Some(TableRevision(self.0.borrow().revision))
    }

    fn history_mark(&self, _: &App) -> usize {
        self.0.borrow().pushed
    }

    fn settle_history(&self, mark: usize, _: &mut App) {
        self.0.borrow_mut().settled.push(mark);
    }

    fn confirm(&self, ask: Confirmation, _: &mut App) -> Task<Result<(), RequestError>> {
        let mut state = self.0.borrow_mut();
        state
            .asked
            .push(format!("{}: {} ({})", ask.agent, ask.title, ask.reason));
        Task::ready(match state.approve {
            true => Ok(()),
            false => Err(RequestError::Declined),
        })
    }

    fn resolve_path(&self, _: Option<&str>, raw: &str, _: &App) -> Result<PathBuf, RequestError> {
        match raw.starts_with("/granted/") {
            true => Ok(PathBuf::from(raw)),
            false => Err(RequestError::FileAccessRequired(raw.into())),
        }
    }
}

/// Parameters every test capability takes.
#[derive(Deserialize, JsonSchema)]
struct Input {
    revision: Option<TableRevision>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    path: Option<GrantedPath>,
}

#[derive(Serialize, JsonSchema)]
struct Output {
    ran: String,
}

thread_local! {
    static HOST: RefCell<Mock> = RefCell::new(Mock::default());
}

fn ran(name: &str, input: &Input) -> Task<Result<Output, RequestError>> {
    let path = match &input.path {
        Some(path) => match path.path() {
            Ok(path) => format!(" {}", path.display()),
            Err(error) => return Task::ready(Err(error)),
        },
        None => String::new(),
    };
    HOST.with(|host| host.borrow().0.borrow_mut().ran.push(name.into()));
    Task::ready(Ok(Output {
        ran: format!("{name}{path}"),
    }))
}

macro_rules! test_capability {
    ($ty:ident, $name:literal, $exposure:expr, $effect:expr) => {
        struct $ty;
        impl Capability for $ty {
            const NAME: &'static str = $name;
            const SUMMARY: &'static str = "A test capability.";
            const GROUP: &'static str = "test";
            const EXPOSURE: Exposure = $exposure;
            const EFFECT: Effect = $effect;
            type Input = Input;
            type Output = Output;

            fn revision(input: &Input) -> Option<TableRevision> {
                input.revision
            }

            fn label(_: &Input, _: &App) -> String {
                $name.into()
            }

            fn reason(input: &Input) -> Option<&str> {
                Some(&input.reason)
            }

            fn paths(input: &mut Input) -> Vec<&mut GrantedPath> {
                input.path.iter_mut().collect()
            }

            fn run(
                input: Input,
                _: &Invocation,
                _: &mut App,
            ) -> Task<Result<Output, RequestError>> {
                ran($name, &input)
            }
        }
    };
}

test_capability!(Look, "look", Exposure::Agent, Effect::Read);
test_capability!(Draft, "draft", Exposure::Agent, Effect::Stage);
test_capability!(Change, "change", Exposure::Agent, Effect::Edit);
test_capability!(Remove, "remove", Exposure::Agent, Effect::Confirm);
test_capability!(Open, "open", Exposure::Agent, Effect::Reach(Reach::Files));
test_capability!(
    Private,
    "private",
    Exposure::ArchivistOnly {
        why: "it changes how qrate itself behaves"
    },
    Effect::Edit
);

/// A capability that never runs: it only checks the catalog's schemas.
struct Typed;

#[derive(Deserialize, JsonSchema)]
struct TypedInput {
    /// The column to look at.
    column: String,
}

impl Capability for Typed {
    const NAME: &'static str = "typed";
    const SUMMARY: &'static str = "Check schemas.";
    const GROUP: &'static str = "test";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    const RESULT: &'static str = "typed_result";
    type Input = TypedInput;
    type Output = NoInput;

    fn label(input: &TypedInput, _: &App) -> String {
        input.column.clone()
    }

    fn run(_: TypedInput, _: &Invocation, _: &mut App) -> Task<Result<NoInput, RequestError>> {
        Task::ready(Ok(NoInput {}))
    }
}

fn setup(cx: &mut TestAppContext) -> Mock {
    let host = Mock::default();
    HOST.with(|cell| *cell.borrow_mut() = host.clone());
    cx.update(|cx| {
        set_host(host.clone(), cx);
        crate::init(cx);
        register::<Look>(cx);
        register::<Draft>(cx);
        register::<Change>(cx);
        register::<Remove>(cx);
        register::<Open>(cx);
        register::<Private>(cx);
        register::<Typed>(cx);
    });
    host
}

async fn call(
    cx: &mut TestAppContext,
    name: &str,
    params: Value,
    caller: Caller,
) -> Result<Value, RequestError> {
    let task = cx.update(|cx| dispatch(name, params, caller, cx).task);
    task.await.map(|answer| answer.value)
}

fn pi() -> Caller {
    Caller::Agent("pi".into())
}

#[gpui::test]
async fn an_answer_carries_the_revision_and_result_beside_the_output(cx: &mut TestAppContext) {
    let host = setup(cx);
    host.0.borrow_mut().revision = 7;
    let answer = call(cx, "look", json!(null), pi()).await.unwrap();
    assert_eq!(
        answer,
        json!({"ran": "look", "result": "look", "revision": 7})
    );
}

#[gpui::test]
async fn bad_parameters_and_unknown_names_are_refused(cx: &mut TestAppContext) {
    setup(cx);
    assert!(matches!(
        call(cx, "typed", json!({"column": 3}), pi()).await,
        Err(RequestError::MalformedParams(_))
    ));
    assert_eq!(
        call(cx, "nope", json!({}), pi()).await,
        Err(RequestError::UnknownCapability("nope".into()))
    );
}

#[gpui::test]
async fn drafts_and_edits_need_the_current_revision(cx: &mut TestAppContext) {
    let host = setup(cx);
    host.0.borrow_mut().revision = 3;
    assert!(matches!(
        call(cx, "draft", json!({}), pi()).await,
        Err(RequestError::MalformedParams(_))
    ));
    assert_eq!(
        call(cx, "change", json!({"revision": 2}), pi()).await,
        Err(RequestError::StaleRevision)
    );
    // A read may name a revision, and is held to it when it does.
    assert_eq!(
        call(cx, "look", json!({"revision": 2}), pi()).await,
        Err(RequestError::StaleRevision)
    );
    assert!(
        call(cx, "draft", json!({"revision": 3}), pi())
            .await
            .is_ok()
    );
    assert!(host.0.borrow().ran == ["draft"]);
}

#[gpui::test]
async fn an_edit_is_settled_into_one_undo_step(cx: &mut TestAppContext) {
    let host = setup(cx);
    host.0.borrow_mut().pushed = 5;
    call(cx, "change", json!({"revision": 0}), pi())
        .await
        .unwrap();
    call(cx, "look", json!({}), pi()).await.unwrap();
    assert_eq!(host.0.borrow().settled, [5]);
}

#[gpui::test]
async fn an_archivist_only_capability_refuses_agents(cx: &mut TestAppContext) {
    let host = setup(cx);
    assert_eq!(
        call(cx, "private", json!({"revision": 0}), pi()).await,
        Err(RequestError::ArchivistOnly(
            "it changes how qrate itself behaves".into()
        ))
    );
    assert!(
        call(cx, "private", json!({"revision": 0}), Caller::Archivist)
            .await
            .is_ok()
    );
    assert_eq!(host.0.borrow().ran, ["private"]);
}

#[gpui::test]
async fn a_confirmed_change_waits_for_the_archivist(cx: &mut TestAppContext) {
    let host = setup(cx);
    assert_eq!(
        call(cx, "remove", json!({"revision": 0, "reason": " "}), pi()).await,
        Err(RequestError::ReasonMissing)
    );
    assert_eq!(
        call(
            cx,
            "remove",
            json!({"revision": 0, "reason": "duplicate"}),
            pi()
        )
        .await,
        Err(RequestError::Declined)
    );
    assert!(host.0.borrow().ran.is_empty(), "declined, so nothing ran");

    host.0.borrow_mut().approve = true;
    call(
        cx,
        "remove",
        json!({"revision": 0, "reason": "duplicate"}),
        pi(),
    )
    .await
    .unwrap();
    // The archivist chose it from a menu: no dialog.
    call(cx, "remove", json!({"revision": 0}), Caller::Archivist)
        .await
        .unwrap();
    let state = host.0.borrow();
    assert_eq!(
        state.asked,
        ["pi: remove (duplicate)", "pi: remove (duplicate)"]
    );
    assert_eq!(state.ran, ["remove", "remove"]);
}

#[gpui::test]
async fn a_path_is_only_usable_once_checked(cx: &mut TestAppContext) {
    let host = setup(cx);
    assert_eq!(
        call(cx, "open", json!({"path": "/home/me/.ssh/id"}), pi()).await,
        Err(RequestError::FileAccessRequired("/home/me/.ssh/id".into()))
    );
    assert!(host.0.borrow().ran.is_empty());
    assert_eq!(
        call(cx, "open", json!({"path": "/granted/a.tif"}), pi())
            .await
            .unwrap()["ran"],
        "open /granted/a.tif"
    );

    // Constructed without the dispatcher, a path fails closed.
    let unchecked: GrantedPath = serde_json::from_value(json!("/granted/a.tif")).unwrap();
    assert!(unchecked.path().is_err());
}

#[gpui::test]
async fn a_runaway_agent_runs_out_of_edits(cx: &mut TestAppContext) {
    setup(cx);
    for _ in 0..EDITS_PER_MINUTE {
        call(cx, "change", json!({"revision": 0}), pi())
            .await
            .unwrap();
    }
    assert_eq!(
        call(cx, "change", json!({"revision": 0}), pi()).await,
        Err(RequestError::RateLimited)
    );
    // Another agent, and reads, are unaffected.
    call(
        cx,
        "change",
        json!({"revision": 0}),
        Caller::Agent("codex".into()),
    )
    .await
    .unwrap();
    call(cx, "look", json!({}), pi()).await.unwrap();
}

#[gpui::test]
async fn the_catalog_describes_every_capability(cx: &mut TestAppContext) {
    setup(cx);
    let catalog = cx.update(|cx| catalog(cx));
    let names: Vec<&str> = catalog.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "capabilities",
            "change",
            "draft",
            "look",
            "open",
            "private",
            "remove",
            "typed"
        ]
    );
    let typed = catalog.iter().find(|d| d.name == "typed").unwrap();
    assert_eq!(typed.result, "typed_result");
    assert_eq!(typed.input["properties"]["column"]["type"], "string");
    assert_eq!(
        typed.input["properties"]["column"]["description"],
        "The column to look at."
    );
    let private = catalog.iter().find(|d| d.name == "private").unwrap();
    assert_eq!(private.exposure, "archivist_only");
    assert!(private.why.is_some());
    // An agent reads the same catalog through a call.
    let listed = call(cx, "capabilities", json!({}), pi()).await.unwrap();
    assert_eq!(listed["capabilities"].as_array().unwrap().len(), 8);
}

//! Agent-written interface: an A2UI surface shown inside a dialog qrate owns, so an agent can lay
//! out a question, a choice or a short form for the archivist without being able to fake qrate's
//! own buttons. What the archivist clicks comes back to the agent as values, never as permission.

use std::time::Duration;

use a2ui_gpui_kit::{AgentMessage, Limits, Surface, SurfaceEvent, SurfaceState};
use ai::agent::RequestError;
use capability::{Capability, Effect, Exposure, Invocation};
use gpui::{
    AnyElement, App, AppContext as _, Entity, Global, IntoElement as _, ParentElement as _,
    SharedString, Styled as _, Task, div,
};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::{ActiveTheme as _, WindowExt as _, v_flex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::in_active_window;

/// Parse an agent's `createSurface` message into a surface qrate can draw, refusing anything the
/// renderer does not support before a dialog opens.
pub(super) fn surface_state(message: Value) -> Result<SurfaceState, RequestError> {
    let refused = |error: a2ui_gpui_kit::A2uiError| {
        RequestError::MalformedParams(format!("surface: {}", error.to_message()))
    };
    match AgentMessage::from_value(message).map_err(refused)? {
        AgentMessage::CreateSurface(create) => {
            SurfaceState::create(create, Limits::default()).map_err(refused)
        }
        _ => Err(RequestError::MalformedParams(
            "surface: send one createSurface message".into(),
        )),
    }
}

/// An agent's surface in a frame that says whose it is. The surface cannot draw outside it.
pub(super) fn framed(agent: &str, surface: Entity<Surface>, cx: &App) -> AnyElement {
    v_flex()
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(format!("From {agent}"))),
        )
        .child(surface)
        .into_any_element()
}

/// Ask the archivist something with a small interface of your own.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Ask {
    /// The dialog's title, at most 200 characters.
    pub question: String,
    /// One A2UI v1.0 `createSurface` message using the basic catalog: Text, Row, Column, Card,
    /// List, Divider, ChoicePicker, TextField, CheckBox and Button. A Button's action answers the
    /// question.
    pub surface: Value,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Answer {
    /// The clicked Button's action name.
    pub action: String,
    /// The values the action's context bound, as the archivist left them.
    pub context: serde_json::Map<String, Value>,
    /// The whole data model, when the surface asked for it with `sendDataModel`.
    pub data_model: Option<Value>,
}

/// The one question waiting for the archivist.
struct PendingAsk;
impl Global for PendingAsk {}

/// How long a question waits before it is given up.
const ASK_WAIT: Duration = Duration::from_secs(10 * 60);

pub struct AskCap;

impl Capability for AskCap {
    const NAME: &'static str = "ask";
    const SUMMARY: &'static str = "Ask the archivist a question with a small interface you lay out in A2UI v1.0 (text, choices, text fields, checkboxes, buttons, lists), shown in a qrate dialog marked as yours. Answers with the action they clicked and the values they entered. Their answer is information, never permission to change anything.";
    const GROUP: &'static str = "project";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = Ask;
    type Output = Answer;

    fn validate(input: &Ask) -> Result<(), RequestError> {
        match input.question.trim().is_empty() || input.question.chars().count() > 200 {
            true => Err(RequestError::MalformedParams(
                "question must be 1 to 200 characters".into(),
            )),
            false => Ok(()),
        }
    }

    fn label(input: &Ask, _: &App) -> String {
        format!("ask “{}”", input.question.trim())
    }

    fn outcome(output: &Answer) -> String {
        format!("answered with {}", output.action)
    }

    fn run(input: Ask, call: &Invocation, cx: &mut App) -> Task<Result<Answer, RequestError>> {
        let state = match surface_state(input.surface) {
            Ok(state) => state,
            Err(error) => return Task::ready(Err(error)),
        };
        if cx.has_global::<PendingAsk>() {
            return Task::ready(Err(RequestError::ConfirmPending));
        }
        if cx.active_window().is_none() {
            return Task::ready(Err(RequestError::NoWindow));
        }
        cx.set_global(PendingAsk);
        let send_model = state.send_data_model();
        let (answer, answers) = async_channel::bounded::<Option<Answer>>(3);
        let timer = cx.background_executor().timer(ASK_WAIT);
        let timeout = answer.clone();
        cx.background_executor()
            .spawn(async move {
                timer.await;
                let _ = timeout.try_send(None);
            })
            .detach();
        let agent = SharedString::from(call.agent().unwrap_or("archivist").to_owned());
        let title = SharedString::from(format!("{agent} asks: {}", input.question.trim()));
        in_active_window(cx, move |window, cx| {
            let surface = cx.new(|_| Surface::new(state));
            let clicked = answer.clone();
            cx.subscribe(&surface, move |surface, event: &SurfaceEvent, cx| {
                if let SurfaceEvent::Action(action) = event {
                    let data_model =
                        send_model.then(|| surface.read(cx).state().data_model().clone());
                    let _ = clicked.try_send(Some(Answer {
                        action: action.name.clone(),
                        context: action.context.clone(),
                        data_model,
                    }));
                    in_active_window(cx, |window, cx| window.close_all_dialogs(cx));
                }
            })
            .detach();
            let dismissed = answer.clone();
            window.open_alert_dialog(cx, move |alert, _, cx| {
                let dismissed = dismissed.clone();
                alert
                    .title(title.clone())
                    .description(framed(&agent, surface.clone(), cx))
                    .button_props(DialogButtonProps::default().ok_text("Dismiss"))
                    .on_ok(move |_, _, _| {
                        let _ = dismissed.try_send(None);
                        true
                    })
                    .on_close({
                        let answer = answer.clone();
                        move |_, _, _| {
                            let _ = answer.try_send(None);
                        }
                    })
            });
        });
        cx.spawn(async move |cx| {
            let answer = answers.recv().await.ok().flatten();
            cx.update(|cx| {
                cx.remove_global::<PendingAsk>();
                answer.ok_or(RequestError::Declined)
            })
        })
    }
}

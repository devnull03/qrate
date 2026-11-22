//! Serde types for the A2UI v1.0 envelopes this renderer exchanges with an agent.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::error::{A2uiError, ErrorCode};

pub const VERSION: &str = "1.0";

/// The only catalog this renderer resolves components and functions against.
pub const BASIC_CATALOG_ID: &str =
    "https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSurface {
    pub surface_id: String,
    pub catalog_id: Option<String>,
    #[serde(default)]
    pub send_data_model: bool,
    #[serde(default)]
    pub components: Vec<Value>,
    pub data_model: Option<Map<String, Value>>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateComponents {
    pub surface_id: String,
    pub components: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDataModel {
    pub surface_id: String,
    pub path: Option<String>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteSurface {
    pub surface_id: String,
}

/// One agent-to-renderer message. Components stay raw JSON here; the surface types them so an
/// error can name the offending component's path.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentMessage {
    CreateSurface(CreateSurface),
    UpdateComponents(UpdateComponents),
    UpdateDataModel(UpdateDataModel),
    DeleteSurface(DeleteSurface),
}

impl AgentMessage {
    pub fn parse(json: &str) -> Result<Self, A2uiError> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| A2uiError::validation("", format!("Message is not valid JSON: {e}")))?;
        Self::from_value(value)
    }

    pub fn from_value(value: Value) -> Result<Self, A2uiError> {
        let Value::Object(mut envelope) = value else {
            return Err(A2uiError::validation("", "Message must be a JSON object."));
        };
        let surface_id = envelope
            .values()
            .find_map(|body| body.get("surfaceId")?.as_str().map(str::to_owned));
        let fail = |e: A2uiError| match &surface_id {
            Some(id) => e.with_surface(id.clone()),
            None => e,
        };
        match envelope.remove("version") {
            Some(Value::String(v)) if v == VERSION => {}
            _ => {
                return Err(fail(A2uiError::validation(
                    "/version",
                    format!("Expected version \"{VERSION}\"."),
                )));
            }
        }
        if envelope.len() != 1 {
            return Err(fail(A2uiError::validation(
                "",
                "Message must contain exactly one message type besides version.",
            )));
        }
        let Some((kind, body)) = envelope.into_iter().next() else {
            unreachable!("length checked above");
        };
        let typed = |e: serde_json::Error| {
            fail(A2uiError::validation(
                format!("/{kind}"),
                format!("Invalid {kind}: {e}"),
            ))
        };
        match kind.as_str() {
            "createSurface" => serde_json::from_value(body)
                .map(Self::CreateSurface)
                .map_err(typed),
            "updateComponents" => serde_json::from_value(body)
                .map(Self::UpdateComponents)
                .map_err(typed),
            "updateDataModel" => serde_json::from_value(body)
                .map(Self::UpdateDataModel)
                .map_err(typed),
            "deleteSurface" => serde_json::from_value(body)
                .map(Self::DeleteSurface)
                .map_err(typed),
            "callRendererFunction" => {
                let mut error = A2uiError::new(
                    ErrorCode::InvalidFunctionCall,
                    "/callRendererFunction",
                    "This renderer exposes no functions to agents.",
                );
                error.function_call_id = body
                    .get("functionCallId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Err(error)
            }
            other => Err(fail(A2uiError::validation(
                format!("/{other}"),
                format!("Unsupported message type \"{other}\"."),
            ))),
        }
    }

    pub fn surface_id(&self) -> &str {
        match self {
            Self::CreateSurface(m) => &m.surface_id,
            Self::UpdateComponents(m) => &m.surface_id,
            Self::UpdateDataModel(m) => &m.surface_id,
            Self::DeleteSurface(m) => &m.surface_id,
        }
    }
}

/// The renderer-to-agent `action` message, sent only after a user clicked a `Button`.
///
/// It reports what the user did and the values bound at that moment. It is a request from the
/// user's side of the surface, never an authorization: the agent must re-check anything it acts on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionMessage {
    pub name: String,
    pub surface_id: String,
    pub source_component_id: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    pub context: Map<String, Value>,
}

impl ActionMessage {
    pub fn to_message(&self) -> Value {
        json!({ "version": VERSION, "action": self })
    }
}

/// `SystemTime` as an RFC 3339 UTC timestamp with second precision.
pub fn utc_timestamp(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

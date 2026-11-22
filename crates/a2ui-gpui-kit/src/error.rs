use serde::Serialize;
use serde_json::{Value, json};

/// The `code` of a renderer-to-agent `error` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// A message or component does not validate against the subset this renderer accepts.
    ValidationFailed,
    /// A child is not allowed inside its container.
    UnallowedChild,
    /// The message conflicts with surface state (duplicate or unknown `surfaceId`).
    IntegrityError,
    /// A function call could not be evaluated.
    ExpressionError,
    /// An agent tried to invoke a renderer function; this renderer exposes none.
    InvalidFunctionCall,
}

/// Why a message was rejected. Every variant becomes the spec's client `error` message through
/// [`A2uiError::to_message`]; a rejected message never changes surface state.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{code:?} at {path}: {message}")]
pub struct A2uiError {
    pub code: ErrorCode,
    pub message: String,
    /// JSON Pointer into the offending agent message, `""` when it concerns the message as a whole.
    pub path: String,
    pub surface_id: Option<String>,
    pub function_call_id: Option<String>,
}

impl A2uiError {
    pub fn new(code: ErrorCode, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path: path.into(),
            surface_id: None,
            function_call_id: None,
        }
    }

    pub fn validation(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ValidationFailed, path, message)
    }

    pub fn with_surface(mut self, surface_id: impl Into<String>) -> Self {
        self.surface_id = Some(surface_id.into());
        self
    }

    /// The `{"version": "1.0", "error": {...}}` envelope to send back to the agent.
    pub fn to_message(&self) -> Value {
        let mut error = json!({ "code": self.code, "message": self.message });
        let validation = matches!(
            self.code,
            ErrorCode::ValidationFailed | ErrorCode::UnallowedChild
        );
        if validation {
            error["path"] = json!(self.path);
            error["surfaceId"] = json!(self.surface_id.as_deref().unwrap_or(""));
        } else if let Some(call) = &self.function_call_id {
            error["functionCallId"] = json!(call);
        } else {
            error["surfaceId"] = json!(self.surface_id.as_deref().unwrap_or(""));
        }
        json!({ "version": crate::protocol::VERSION, "error": error })
    }
}

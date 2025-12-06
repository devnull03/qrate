use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlite::Error),

    #[error("{0}")]
    DatabaseStr(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("AI provider error: {0}")]
    Provider(String),

    #[error("Project not open")]
    ProjectNotOpen,

    #[error("Invalid project file: {0}")]
    InvalidProject(String),
}

// Tauri commands must return a serializable result.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
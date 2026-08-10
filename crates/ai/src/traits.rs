use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::Result;

/// The image a row is reviewed against. `base64_data` is filled in only for a provider that
/// takes image bytes inline; a local one reads the path itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContext {
    pub path: PathBuf,
    pub base64_data: Option<String>,
    pub mime_type: Option<String>,
}

impl ImageContext {
    pub fn from_path(path: PathBuf) -> Self {
        Self {
            path,
            base64_data: None,
            mime_type: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub reason: Option<String>,
    pub confidence: Option<f32>,
    pub discrepancies: Vec<FieldDiscrepancy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDiscrepancy {
    pub field_id: String,
    pub expected: String,
    pub found: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerCapabilities {
    pub name: String,
    pub is_local: bool,
    pub supports_multimodal: bool,
    pub max_image_size: Option<usize>,
}

/// What the agent panel asks of a model: does this row describe this image, and where doesn't it.
///
/// `row_data` is the row as JSON — column name to value — so a provider needs no notion of grids.
#[async_trait]
pub trait DataReviewer: Send + Sync {
    async fn initialize(&self) -> Result<()>;
    async fn validate_row(
        &self,
        row_data: serde_json::Value,
        image_context: ImageContext,
    ) -> Result<ValidationResult>;
    fn capabilities(&self) -> ReviewerCapabilities;
}

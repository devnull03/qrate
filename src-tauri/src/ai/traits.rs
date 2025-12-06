use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowData(pub serde_json::Value);

impl RowData {
    pub fn new(value: serde_json::Value) -> Self {
        Self(value)
    }

    pub fn inner(&self) -> &serde_json::Value {
        &self.0
    }
}

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

    pub fn with_base64(mut self, data: String, mime_type: String) -> Self {
        self.base64_data = Some(data);
        self.mime_type = Some(mime_type);
        self
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
    pub supports_embeddings: bool,
    pub max_image_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterAssignment {
    pub data_point_index: usize,
    pub cluster_id: Option<usize>,
}

#[async_trait]
pub trait DataReviewer: Send + Sync {
    async fn initialize(&self) -> Result<()>;
    async fn validate_row(
        &self,
        row_data: RowData,
        image_context: ImageContext,
    ) -> Result<ValidationResult>;
    fn capabilities(&self) -> ReviewerCapabilities;
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_image(&self, image: ImageContext) -> Result<Vec<f32>>;
    fn embedding_dimensions(&self) -> usize;
}

pub trait Clusterer: Send + Sync {
    fn cluster(&self, vectors: &[Vec<f32>]) -> Result<Vec<ClusterAssignment>>;
    fn algorithm_name(&self) -> &str;
}

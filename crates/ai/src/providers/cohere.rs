use async_trait::async_trait;

use crate::error::{AppError, Result};
use crate::traits::{DataReviewer, ImageContext, ReviewerCapabilities, ValidationResult};

pub struct CohereProvider {
    api_key: String,
    client: reqwest::Client,
}

impl CohereProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("COHERE_API_KEY").map_err(|_| {
            AppError::Provider("COHERE_API_KEY environment variable not set".into())
        })?;
        Ok(Self::new(api_key))
    }
}

#[async_trait]
impl DataReviewer for CohereProvider {
    async fn initialize(&self) -> Result<()> {
        if self.api_key.is_empty() {
            return Err(AppError::Provider("Cohere API key is empty".into()));
        }
        Ok(())
    }

    async fn validate_row(
        &self,
        _row_data: serde_json::Value,
        _image_context: ImageContext,
    ) -> Result<ValidationResult> {
        todo!("Implement Cohere Command R+ validation")
    }

    fn capabilities(&self) -> ReviewerCapabilities {
        ReviewerCapabilities {
            name: "Cohere Command R+".to_string(),
            is_local: false,
            supports_multimodal: true,
            max_image_size: Some(20 * 1024 * 1024), // 20MB
        }
    }
}

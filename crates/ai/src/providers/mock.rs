use async_trait::async_trait;

use crate::error::Result;
use crate::traits::{
    DataReviewer, FieldDiscrepancy, ImageContext, ReviewerCapabilities, ValidationResult,
};

/// A reviewer that answers without a model, so the agent panel can be driven offline.
/// `should_validate` picks which of the two answers it gives.
pub struct MockProvider {
    pub should_validate: bool,
    pub confidence: f32,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self {
            should_validate: true,
            confidence: 0.95,
        }
    }
}

#[async_trait]
impl DataReviewer for MockProvider {
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }

    async fn validate_row(
        &self,
        row_data: serde_json::Value,
        _image_context: ImageContext,
    ) -> Result<ValidationResult> {
        let discrepancies = match self.should_validate {
            true => vec![],
            // The first field, so a failing answer points at something real in the row.
            false => row_data
                .as_object()
                .and_then(|obj| obj.iter().next())
                .map(|(key, value)| FieldDiscrepancy {
                    field_id: key.clone(),
                    expected: value.to_string(),
                    found: Some("mock_different_value".to_string()),
                    description: "Mock discrepancy for testing".to_string(),
                })
                .into_iter()
                .collect(),
        };

        Ok(ValidationResult {
            is_valid: self.should_validate,
            reason: Some(
                if self.should_validate {
                    "Mock validation passed"
                } else {
                    "Mock validation failed for testing"
                }
                .to_string(),
            ),
            confidence: Some(self.confidence),
            discrepancies,
        })
    }

    fn capabilities(&self) -> ReviewerCapabilities {
        ReviewerCapabilities {
            name: "Mock Provider".to_string(),
            is_local: true,
            supports_multimodal: true,
            max_image_size: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    async fn review(should_validate: bool) -> ValidationResult {
        MockProvider {
            should_validate,
            ..Default::default()
        }
        .validate_row(
            serde_json::json!({"field": "value"}),
            ImageContext::from_path(PathBuf::from("test.jpg")),
        )
        .await
        .unwrap()
    }

    /// A passing answer carries no discrepancies and a failing one names a field — the two shapes
    /// the panel has to render.
    #[tokio::test]
    async fn a_verdict_carries_discrepancies_only_when_it_fails() {
        let passed = review(true).await;
        assert!(passed.is_valid);
        assert!(passed.discrepancies.is_empty());

        let failed = review(false).await;
        assert!(!failed.is_valid);
        assert_eq!(failed.discrepancies[0].field_id, "field");
    }
}

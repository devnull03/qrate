use async_trait::async_trait;

use crate::error::Result;
use crate::traits::{
    ClusterAssignment, Clusterer, DataReviewer, Embedder, FieldDiscrepancy, ImageContext,
    ReviewerCapabilities, RowData, ValidationResult,
};

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

impl MockProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_validation(mut self, should_validate: bool) -> Self {
        self.should_validate = should_validate;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }
}

#[async_trait]
impl DataReviewer for MockProvider {
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }

    async fn validate_row(
        &self,
        row_data: RowData,
        _image_context: ImageContext,
    ) -> Result<ValidationResult> {
        let discrepancies = if self.should_validate {
            vec![]
        } else {
            if let Some(obj) = row_data.inner().as_object() {
                if let Some((key, value)) = obj.iter().next() {
                    vec![FieldDiscrepancy {
                        field_id: key.clone(),
                        expected: value.to_string(),
                        found: Some("mock_different_value".to_string()),
                        description: "Mock discrepancy for testing".to_string(),
                    }]
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };

        Ok(ValidationResult {
            is_valid: self.should_validate,
            reason: Some(if self.should_validate {
                "Mock validation passed".to_string()
            } else {
                "Mock validation failed for testing".to_string()
            }),
            confidence: Some(self.confidence),
            discrepancies,
        })
    }

    fn capabilities(&self) -> ReviewerCapabilities {
        ReviewerCapabilities {
            name: "Mock Provider".to_string(),
            is_local: true,
            supports_multimodal: true,
            supports_embeddings: true,
            max_image_size: None,
        }
    }
}

#[async_trait]
impl Embedder for MockProvider {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let mut embedding = vec![0.0f32; 1024];
        for (i, byte) in text.bytes().enumerate() {
            embedding[i % 1024] += (byte as f32) / 255.0;
        }
        let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for val in &mut embedding {
                *val /= magnitude;
            }
        }
        Ok(embedding)
    }

    async fn embed_image(&self, image: ImageContext) -> Result<Vec<f32>> {
        self.embed_text(&image.path.to_string_lossy()).await
    }

    fn embedding_dimensions(&self) -> usize {
        1024
    }
}

impl Clusterer for MockProvider {
    fn cluster(&self, vectors: &[Vec<f32>]) -> Result<Vec<ClusterAssignment>> {
        let assignments = vectors
            .iter()
            .enumerate()
            .map(|(i, vec)| {
                let cluster_id = if vec.is_empty() {
                    None
                } else {
                    let first_val = vec[0];
                    if first_val < -0.3 {
                        Some(0)
                    } else if first_val > 0.3 {
                        Some(2)
                    } else {
                        Some(1)
                    }
                };
                ClusterAssignment { data_point_index: i, cluster_id }
            })
            .collect();
        Ok(assignments)
    }

    fn algorithm_name(&self) -> &str {
        "MockClustering"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_mock_validation_passes() {
        let provider = MockProvider::new();
        let row = RowData::new(serde_json::json!({"field": "value"}));
        let image = ImageContext::from_path(PathBuf::from("test.jpg"));
        let result = provider.validate_row(row, image).await.unwrap();
        assert!(result.is_valid);
        assert!(result.discrepancies.is_empty());
    }

    #[tokio::test]
    async fn test_mock_validation_fails() {
        let provider = MockProvider::new().with_validation(false);
        let row = RowData::new(serde_json::json!({"field": "value"}));
        let image = ImageContext::from_path(PathBuf::from("test.jpg"));
        let result = provider.validate_row(row, image).await.unwrap();
        assert!(!result.is_valid);
        assert!(!result.discrepancies.is_empty());
    }

    #[tokio::test]
    async fn test_mock_embedding() {
        let provider = MockProvider::new();
        let embedding = provider.embed_text("test text").await.unwrap();
        assert_eq!(embedding.len(), 1024);
    }

    #[test]
    fn test_mock_clustering() {
        let provider = MockProvider::new();
        let vectors = vec![
            vec![-0.5, 0.0, 0.0],
            vec![0.0, 0.0, 0.0],
            vec![0.5, 0.0, 0.0],
        ];
        let assignments = provider.cluster(&vectors).unwrap();
        assert_eq!(assignments.len(), 3);
        assert_eq!(assignments[0].cluster_id, Some(0));
        assert_eq!(assignments[1].cluster_id, Some(1));
        assert_eq!(assignments[2].cluster_id, Some(2));
    }
}

use serde::{Deserialize, Serialize};

use crate::database::ColumnDef;

#[derive(Debug, Serialize, Deserialize)]
pub struct FileOpenResponse {
    pub path: String,
    pub columns: Vec<ColumnDef>,
    pub total_rows: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataResponse {
    pub rows: Vec<Vec<serde_json::Value>>,
    pub total: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CurrentStateResponse {
    pub is_file_open: bool,
    pub path: Option<String>,
    pub columns: Vec<ColumnDef>,
    pub total_rows: i64,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FilePathValidationResponse {
    pub valid: bool,
    pub resolved_path: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CsvPreviewResponse {
    pub headers: Vec<String>,
    pub first_row: Option<Vec<String>>,
}

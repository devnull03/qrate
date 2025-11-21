use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[derive(Debug, Serialize, Deserialize)]
struct CsvRow {
    #[serde(flatten)]
    data: HashMap<String, String>,
}

#[tauri::command]
fn load_csv(path: String) -> Result<Vec<HashMap<String, String>>, String> {
    let mut reader = csv::Reader::from_path(&path)
        .map_err(|e| format!("Failed to open CSV file: {}", e))?;

    let headers = reader
        .headers()
        .map_err(|e| format!("Failed to read CSV headers: {}", e))?
        .clone();

    let mut result = Vec::new();

    for record in reader.records() {
        let record = record.map_err(|e| format!("Failed to read CSV record: {}", e))?;
        let mut row = HashMap::new();

        for (header, value) in headers.iter().zip(record.iter()) {
            row.insert(header.to_string(), value.to_string());
        }

        result.push(row);
    }

    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs_pro::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![greet, load_csv])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri;

// =============================================================================
// ENUMS
// =============================================================================

/// Type of setting - global (app-wide) or project-specific
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingScope {
    /// Global settings persist across all projects (stored in app data via tauri-plugin-store)
    Global,
    /// Project settings are stored in the .qrate file's _settings table
    Project,
}

/// The data type of a setting value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingType {
    String,
    Number,
    Boolean,
    Select,
    Path,
    Pattern,
}

// =============================================================================
// SETTING DEFINITION
// =============================================================================

/// Static definition of a setting with metadata
#[derive(Debug, Clone)]
pub struct SettingDef {
    pub key: &'static str,
    pub scope: SettingScope,
    pub setting_type: SettingType,
    pub default_value: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    /// For number types: min value
    pub min: Option<f64>,
    /// For number types: max value
    pub max: Option<f64>,
    /// For select types: comma-separated options
    pub options: Option<&'static str>,
}

/// Serializable version of SettingDef for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingDefResponse {
    pub key: String,
    pub scope: String,
    pub setting_type: String,
    pub default_value: String,
    pub label: String,
    pub description: String,
    pub category: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub options: Option<Vec<String>>,
}

impl From<&SettingDef> for SettingDefResponse {
    fn from(def: &SettingDef) -> Self {
        Self {
            key: def.key.to_string(),
            scope: match def.scope {
                SettingScope::Global => "global".to_string(),
                SettingScope::Project => "project".to_string(),
            },
            setting_type: match def.setting_type {
                SettingType::String => "string".to_string(),
                SettingType::Number => "number".to_string(),
                SettingType::Boolean => "boolean".to_string(),
                SettingType::Select => "select".to_string(),
                SettingType::Path => "path".to_string(),
                SettingType::Pattern => "pattern".to_string(),
            },
            default_value: def.default_value.to_string(),
            label: def.label.to_string(),
            description: def.description.to_string(),
            category: def.category.to_string(),
            min: def.min,
            max: def.max,
            options: def
                .options
                .map(|o| o.split(',').map(|s| s.to_string()).collect()),
        }
    }
}

// =============================================================================
// GLOBAL SETTINGS DEFINITIONS
// =============================================================================

/// All global settings definitions
pub static GLOBAL_SETTINGS: &[SettingDef] = &[
    // Appearance
    SettingDef {
        key: "theme",
        scope: SettingScope::Global,
        setting_type: SettingType::Select,
        default_value: "system",
        label: "Theme",
        description: "Choose the application color theme",
        category: "Appearance",
        min: None,
        max: None,
        options: Some("light,dark,system"),
    },
    // Default values for new projects
    SettingDef {
        key: "defaultRowLimit",
        scope: SettingScope::Global,
        setting_type: SettingType::Number,
        default_value: "100",
        label: "Default Row Limit",
        description: "Default number of rows to load at a time for new projects",
        category: "Defaults",
        min: Some(50.0),
        max: Some(1000.0),
        options: None,
    },
    SettingDef {
        key: "defaultFilePathPattern",
        scope: SettingScope::Global,
        setting_type: SettingType::Pattern,
        default_value: "{files_folder}/{file_column}",
        label: "Default File Path Pattern",
        description: "Default pattern for locating files in new projects",
        category: "Defaults",
        min: None,
        max: None,
        options: None,
    },
    SettingDef {
        key: "defaultFileColumnName",
        scope: SettingScope::Global,
        setting_type: SettingType::String,
        default_value: "file",
        label: "Default File Column Name",
        description: "Default column name for file references in new projects",
        category: "Defaults",
        min: None,
        max: None,
        options: None,
    },
    // Behavior
    SettingDef {
        key: "autoSaveInterval",
        scope: SettingScope::Global,
        setting_type: SettingType::Number,
        default_value: "0",
        label: "Auto-Save Interval",
        description: "Interval for auto-saving changes in ms (0 to disable)",
        category: "Behavior",
        min: Some(0.0),
        max: Some(60000.0),
        options: None,
    },
    SettingDef {
        key: "confirmBeforeDelete",
        scope: SettingScope::Global,
        setting_type: SettingType::Boolean,
        default_value: "true",
        label: "Confirm Before Delete",
        description: "Show confirmation dialog before deleting rows",
        category: "Behavior",
        min: None,
        max: None,
        options: None,
    },
    SettingDef {
        key: "reopenLastProject",
        scope: SettingScope::Global,
        setting_type: SettingType::Boolean,
        default_value: "true",
        label: "Reopen Last Project",
        description: "Automatically reopen the last project on startup",
        category: "Behavior",
        min: None,
        max: None,
        options: None,
    },
    // Grid/Table
    SettingDef {
        key: "gridAlternateRowColors",
        scope: SettingScope::Global,
        setting_type: SettingType::Boolean,
        default_value: "true",
        label: "Alternate Row Colors",
        description: "Use alternating colors for grid rows",
        category: "Grid",
        min: None,
        max: None,
        options: None,
    },
];

// =============================================================================
// PROJECT SETTINGS DEFINITIONS
// =============================================================================

/// All project settings definitions (stored in .qrate file)
pub static PROJECT_SETTINGS: &[SettingDef] = &[
    SettingDef {
        key: "filesFolder",
        scope: SettingScope::Project,
        setting_type: SettingType::Path,
        default_value: "",
        label: "Files Folder",
        description: "Base folder containing all files referenced in this project",
        category: "Files",
        min: None,
        max: None,
        options: None,
    },
    SettingDef {
        key: "filePathPattern",
        scope: SettingScope::Project,
        setting_type: SettingType::Pattern,
        default_value: "{files_folder}/{file_column}",
        label: "File Path Pattern",
        description:
            "Pattern for locating files. Use {files_folder}, {file_column}, or any column name.",
        category: "Files",
        min: None,
        max: None,
        options: None,
    },
    SettingDef {
        key: "fileColumnName",
        scope: SettingScope::Project,
        setting_type: SettingType::String,
        default_value: "file",
        label: "File Column Name",
        description: "The column name containing file names in your CSV",
        category: "Files",
        min: None,
        max: None,
        options: None,
    },
    SettingDef {
        key: "defaultRowLimit",
        scope: SettingScope::Project,
        setting_type: SettingType::Number,
        default_value: "100",
        label: "Row Limit",
        description: "Number of rows to load at a time for this project",
        category: "Data",
        min: Some(50.0),
        max: Some(1000.0),
        options: None,
    },
    // Internal/view state settings
    SettingDef {
        key: "lastSelectedRowId",
        scope: SettingScope::Project,
        setting_type: SettingType::Number,
        default_value: "0",
        label: "Last Selected Row",
        description: "Internal: tracks the last selected row",
        category: "Internal",
        min: None,
        max: None,
        options: None,
    },
    SettingDef {
        key: "lastScrollPosition",
        scope: SettingScope::Project,
        setting_type: SettingType::Number,
        default_value: "0",
        label: "Last Scroll Position",
        description: "Internal: tracks the last scroll position",
        category: "Internal",
        min: None,
        max: None,
        options: None,
    },
];

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Get a setting definition by key from global settings
pub fn get_global_setting_def(key: &str) -> Option<&'static SettingDef> {
    GLOBAL_SETTINGS.iter().find(|s| s.key == key)
}

/// Get a setting definition by key from project settings
pub fn get_project_setting_def(key: &str) -> Option<&'static SettingDef> {
    PROJECT_SETTINGS.iter().find(|s| s.key == key)
}

/// Get all global settings with their default values as a HashMap
pub fn get_global_defaults() -> HashMap<String, String> {
    GLOBAL_SETTINGS
        .iter()
        .map(|s| (s.key.to_string(), s.default_value.to_string()))
        .collect()
}

/// Get all project settings with their default values as a HashMap
pub fn get_project_defaults() -> HashMap<String, String> {
    PROJECT_SETTINGS
        .iter()
        .map(|s| (s.key.to_string(), s.default_value.to_string()))
        .collect()
}

/// Get global setting categories
pub fn get_global_categories() -> Vec<&'static str> {
    let mut categories: Vec<&'static str> = GLOBAL_SETTINGS.iter().map(|s| s.category).collect();
    categories.sort();
    categories.dedup();
    categories
}

/// Get project setting categories (excluding Internal)
pub fn get_project_categories() -> Vec<&'static str> {
    let mut categories: Vec<&'static str> = PROJECT_SETTINGS
        .iter()
        .filter(|s| s.category != "Internal")
        .map(|s| s.category)
        .collect();
    categories.sort();
    categories.dedup();
    categories
}

/// Get all global settings in a specific category
pub fn get_global_settings_by_category(category: &str) -> Vec<&'static SettingDef> {
    GLOBAL_SETTINGS
        .iter()
        .filter(|s| s.category == category)
        .collect()
}

/// Get all project settings in a specific category
pub fn get_project_settings_by_category(category: &str) -> Vec<&'static SettingDef> {
    PROJECT_SETTINGS
        .iter()
        .filter(|s| s.category == category && s.category != "Internal")
        .collect()
}

/// Validate a setting value against its definition
pub fn validate_setting(value: &str, def: &SettingDef) -> Result<(), String> {
    match def.setting_type {
        SettingType::Number => {
            let num: f64 = value
                .parse()
                .map_err(|_| format!("{} must be a number", def.label))?;

            if let Some(min) = def.min {
                if num < min {
                    return Err(format!("{} must be at least {}", def.label, min));
                }
            }
            if let Some(max) = def.max {
                if num > max {
                    return Err(format!("{} must be at most {}", def.label, max));
                }
            }
            Ok(())
        }
        SettingType::Boolean => {
            if value != "true" && value != "false" && value != "0" && value != "1" {
                return Err(format!("{} must be true or false", def.label));
            }
            Ok(())
        }
        SettingType::Select => {
            if let Some(options) = def.options {
                let valid_options: Vec<&str> = options.split(',').collect();
                if !valid_options.contains(&value) {
                    return Err(format!(
                        "{} must be one of: {}",
                        def.label,
                        valid_options.join(", ")
                    ));
                }
            }
            Ok(())
        }
        SettingType::String | SettingType::Path | SettingType::Pattern => Ok(()),
    }
}
 
// =============================================================================
// DATABASE INITIALIZATION
// =============================================================================

use rusqlite::Connection;

/// Initialize project settings in the database with defaults
/// This should be called when creating a new .qrate file
pub fn init_project_settings(conn: &Connection) -> rusqlite::Result<()> {
    for setting in PROJECT_SETTINGS.iter() {
        conn.execute(
            "INSERT OR IGNORE INTO _settings (key, value) VALUES (?1, ?2)",
            rusqlite::params![setting.key, setting.default_value],
        )?;
    }
    Ok(())
}

/// Ensure all project settings exist (for migrations/upgrades)
/// This adds any new settings that weren't in older versions
pub fn ensure_project_settings(conn: &Connection) -> rusqlite::Result<()> {
    for setting in PROJECT_SETTINGS.iter() {
        conn.execute(
            "INSERT OR IGNORE INTO _settings (key, value) VALUES (?1, ?2)",
            rusqlite::params![setting.key, setting.default_value],
        )?;
    }
    Ok(())
}

/// Get all project settings from the database, using defaults for missing values
pub fn get_project_settings_with_defaults(
    conn: &Connection,
) -> rusqlite::Result<HashMap<String, String>> {
    let mut settings = get_project_defaults();

    let mut stmt = conn.prepare("SELECT key, value FROM _settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    for row in rows {
        let (key, value) = row?;
        settings.insert(key, value);
    }

    Ok(settings)
}

// =============================================================================
// TAURI COMMANDS
// =============================================================================

pub mod commands {
    use super::*;
    use crate::app_state::AppState;
    use crate::database;
    use tauri::State;

    /// Get all global settings definitions (schema)
    #[tauri::command]
    pub fn get_global_settings_schema() -> Vec<SettingDefResponse> {
        GLOBAL_SETTINGS
            .iter()
            .map(SettingDefResponse::from)
            .collect()
    }

    /// Get all project settings definitions (schema)
    #[tauri::command]
    pub fn get_project_settings_schema() -> Vec<SettingDefResponse> {
        PROJECT_SETTINGS
            .iter()
            .map(SettingDefResponse::from)
            .collect()
    }

    /// Get global settings default values
    #[tauri::command]
    pub fn get_global_settings_defaults() -> HashMap<String, String> {
        get_global_defaults()
    }

    /// Get project settings default values
    #[tauri::command]
    pub fn get_project_settings_defaults() -> HashMap<String, String> {
        get_project_defaults()
    }

    /// Validate a setting value
    #[tauri::command]
    pub fn validate_setting_value(key: String, value: String, scope: String) -> Result<(), String> {
        let def = if scope == "global" {
            get_global_setting_def(&key)
        } else {
            get_project_setting_def(&key)
        };

        match def {
            Some(d) => validate_setting(&value, d),
            None => Err(format!("Unknown setting: {}", key)),
        }
    }

    /// Get all settings for the current file
    #[tauri::command]
    pub fn get_project_settings(
        state: State<AppState>,
        path: String,
    ) -> Result<HashMap<String, String>, String> {
        let conn_arc = state
            .get_connection(&path)
            .ok_or_else(|| "File not open".to_string())?;

        let conn = conn_arc.lock().unwrap();

        database::get_all_settings(&conn).map_err(|e| format!("Failed to get settings: {}", e))
    }

    /// Set a single setting for the current file
    #[tauri::command]
    pub fn set_project_setting(
        state: State<AppState>,
        path: String,
        key: String,
        value: String,
    ) -> Result<(), String> {
        let conn_arc = state
            .get_connection(&path)
            .ok_or_else(|| "File not open".to_string())?;

        let conn = conn_arc.lock().unwrap();

        database::set_setting(&conn, &key, &value)
            .map_err(|e| format!("Failed to set setting: {}", e))
    }

    /// Set multiple settings for the current file
    #[tauri::command]
    pub fn set_project_settings(
        state: State<AppState>,
        path: String,
        settings: HashMap<String, String>,
    ) -> Result<(), String> {
        let conn_arc = state
            .get_connection(&path)
            .ok_or_else(|| "File not open".to_string())?;

        let conn = conn_arc.lock().unwrap();

        database::set_all_settings(&conn, &settings)
            .map_err(|e| format!("Failed to set settings: {}", e))
    }

    /// Get settings with defaults filled in for the current file
    #[tauri::command]
    pub fn get_project_settings_with_defaults_cmd(
        state: State<AppState>,
        path: String,
    ) -> Result<HashMap<String, String>, String> {
        let conn_arc = state
            .get_connection(&path)
            .ok_or_else(|| "File not open".to_string())?;

        let conn = conn_arc.lock().unwrap();

        get_project_settings_with_defaults(&conn)
            .map_err(|e| format!("Failed to get settings: {}", e))
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_defaults() {
        let defaults = get_global_defaults();
        assert_eq!(defaults.get("theme"), Some(&"system".to_string()));
        assert_eq!(defaults.get("defaultRowLimit"), Some(&"100".to_string()));
    }

    #[test]
    fn test_project_defaults() {
        let defaults = get_project_defaults();
        assert_eq!(defaults.get("filesFolder"), Some(&"".to_string()));
        assert_eq!(defaults.get("fileColumnName"), Some(&"file".to_string()));
    }

    #[test]
    fn test_validate_number() {
        let def = get_global_setting_def("defaultRowLimit").unwrap();
        assert!(validate_setting("100", def).is_ok());
        assert!(validate_setting("25", def).is_err()); // Below min
        assert!(validate_setting("2000", def).is_err()); // Above max
        assert!(validate_setting("abc", def).is_err()); // Not a number
    }

    #[test]
    fn test_validate_select() {
        let def = get_global_setting_def("theme").unwrap();
        assert!(validate_setting("dark", def).is_ok());
        assert!(validate_setting("light", def).is_ok());
        assert!(validate_setting("system", def).is_ok());
        assert!(validate_setting("invalid", def).is_err());
    }

    #[test]
    fn test_categories() {
        let global_cats = get_global_categories();
        assert!(global_cats.contains(&"Appearance"));
        assert!(global_cats.contains(&"Behavior"));

        let project_cats = get_project_categories();
        assert!(project_cats.contains(&"Files"));
        assert!(!project_cats.contains(&"Internal")); // Internal should be excluded
    }
}

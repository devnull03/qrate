use rusqlite::params;
use tauri::State;

use crate::app_state::AppState;

use super::types::{Annotation, CreateAnnotation, UpdateAnnotation};

/// Get all annotations for a file, optionally filtered by provider
#[tauri::command]
pub fn get_annotations(
    state: State<AppState>,
    path: String,
    provider: Option<String>,
) -> Result<Vec<Annotation>, String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let (query, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match &provider {
        Some(p) => (
            "SELECT id, provider, row_id, column_id, severity, message, created_at, updated_at, resolved, metadata
             FROM _annotations WHERE provider = ?1 ORDER BY created_at DESC".to_string(),
            vec![Box::new(p.clone()) as Box<dyn rusqlite::ToSql>],
        ),
        None => (
            "SELECT id, provider, row_id, column_id, severity, message, created_at, updated_at, resolved, metadata
             FROM _annotations ORDER BY created_at DESC".to_string(),
            vec![],
        ),
    };

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let annotations = stmt
        .query_map(params_refs.as_slice(), |row| {
            let metadata_str: Option<String> = row.get(9)?;
            let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());

            Ok(Annotation {
                id: row.get(0)?,
                provider: row.get(1)?,
                row_id: row.get(2)?,
                column_id: row.get(3)?,
                severity: row.get(4)?,
                message: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                resolved: row.get::<_, i32>(8)? != 0,
                metadata,
            })
        })
        .map_err(|e| format!("Failed to query annotations: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect annotations: {}", e))?;

    Ok(annotations)
}

/// Create a new annotation
#[tauri::command]
pub fn create_annotation(
    state: State<AppState>,
    path: String,
    annotation: CreateAnnotation,
) -> Result<Annotation, String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let severity = annotation.severity.unwrap_or_else(|| "info".to_string());
    let metadata_json = annotation
        .metadata
        .map(|m| serde_json::to_string(&m).unwrap_or_else(|_| "null".to_string()));

    conn.execute(
        "INSERT INTO _annotations (provider, row_id, column_id, severity, message, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            annotation.provider,
            annotation.row_id,
            annotation.column_id,
            severity,
            annotation.message,
            metadata_json,
        ],
    )
    .map_err(|e| format!("Failed to create annotation: {}", e))?;

    let id = conn.last_insert_rowid();

    let mut stmt = conn
        .prepare(
            "SELECT id, provider, row_id, column_id, severity, message, created_at, updated_at, resolved, metadata
             FROM _annotations WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let result = stmt
        .query_row([id], |row| {
            let metadata_str: Option<String> = row.get(9)?;
            let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());

            Ok(Annotation {
                id: row.get(0)?,
                provider: row.get(1)?,
                row_id: row.get(2)?,
                column_id: row.get(3)?,
                severity: row.get(4)?,
                message: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                resolved: row.get::<_, i32>(8)? != 0,
                metadata,
            })
        })
        .map_err(|e| format!("Failed to fetch created annotation: {}", e))?;

    Ok(result)
}

/// Update an existing annotation
#[tauri::command]
pub fn update_annotation(
    state: State<AppState>,
    path: String,
    id: i64,
    updates: UpdateAnnotation,
) -> Result<Annotation, String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let mut set_clauses = vec!["updated_at = datetime('now')".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(ref message) = updates.message {
        params.push(Box::new(message.clone()));
        set_clauses.push(format!("message = ?{}", params.len()));
    }

    if let Some(resolved) = updates.resolved {
        params.push(Box::new(if resolved { 1 } else { 0 }));
        set_clauses.push(format!("resolved = ?{}", params.len()));
    }

    if let Some(ref metadata) = updates.metadata {
        let metadata_json = serde_json::to_string(metadata).unwrap_or_else(|_| "null".to_string());
        params.push(Box::new(metadata_json));
        set_clauses.push(format!("metadata = ?{}", params.len()));
    }

    params.push(Box::new(id));
    let id_param_index = params.len();

    let query = format!(
        "UPDATE _annotations SET {} WHERE id = ?{}",
        set_clauses.join(", "),
        id_param_index
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&query, params_refs.as_slice())
        .map_err(|e| format!("Failed to update annotation: {}", e))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, provider, row_id, column_id, severity, message, created_at, updated_at, resolved, metadata
             FROM _annotations WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let result = stmt
        .query_row([id], |row| {
            let metadata_str: Option<String> = row.get(9)?;
            let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());

            Ok(Annotation {
                id: row.get(0)?,
                provider: row.get(1)?,
                row_id: row.get(2)?,
                column_id: row.get(3)?,
                severity: row.get(4)?,
                message: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                resolved: row.get::<_, i32>(8)? != 0,
                metadata,
            })
        })
        .map_err(|e| format!("Failed to fetch updated annotation: {}", e))?;

    Ok(result)
}

/// Delete an annotation by id
#[tauri::command]
pub fn delete_annotation(state: State<AppState>, path: String, id: i64) -> Result<(), String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let rows_affected = conn
        .execute("DELETE FROM _annotations WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete annotation: {}", e))?;

    if rows_affected == 0 {
        return Err(format!("Annotation with id {} not found", id));
    }

    Ok(())
}

/// Get annotations for a specific cell, row, or column
#[tauri::command]
pub fn get_annotations_at(
    state: State<AppState>,
    path: String,
    row_id: Option<i64>,
    column_id: Option<String>,
) -> Result<Vec<Annotation>, String> {
    let conn_arc = state
        .get_connection(&path)
        .ok_or_else(|| "File not open".to_string())?;

    let conn = conn_arc.lock().unwrap();

    let query = match (&row_id, &column_id) {
        (Some(_), Some(_)) => {
            "SELECT id, provider, row_id, column_id, severity, message, created_at, updated_at, resolved, metadata
             FROM _annotations WHERE row_id = ?1 AND column_id = ?2 ORDER BY created_at DESC"
        }
        (Some(_), None) => {
            "SELECT id, provider, row_id, column_id, severity, message, created_at, updated_at, resolved, metadata
             FROM _annotations WHERE row_id = ?1 AND column_id IS NULL ORDER BY created_at DESC"
        }
        (None, Some(_)) => {
            "SELECT id, provider, row_id, column_id, severity, message, created_at, updated_at, resolved, metadata
             FROM _annotations WHERE row_id IS NULL AND column_id = ?1 ORDER BY created_at DESC"
        }
        (None, None) => {
            return Err("Must provide either row_id or column_id".to_string());
        }
    };

    let mut stmt = conn
        .prepare(query)
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let annotations = match (&row_id, &column_id) {
        (Some(rid), Some(cid)) => stmt
            .query_map(params![rid, cid], |row| {
                let metadata_str: Option<String> = row.get(9)?;
                let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Annotation {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    row_id: row.get(2)?,
                    column_id: row.get(3)?,
                    severity: row.get(4)?,
                    message: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                    resolved: row.get::<_, i32>(8)? != 0,
                    metadata,
                })
            })
            .map_err(|e| format!("Failed to query annotations: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect annotations: {}", e))?,
        (Some(rid), None) => stmt
            .query_map(params![rid], |row| {
                let metadata_str: Option<String> = row.get(9)?;
                let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Annotation {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    row_id: row.get(2)?,
                    column_id: row.get(3)?,
                    severity: row.get(4)?,
                    message: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                    resolved: row.get::<_, i32>(8)? != 0,
                    metadata,
                })
            })
            .map_err(|e| format!("Failed to query annotations: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect annotations: {}", e))?,
        (None, Some(cid)) => stmt
            .query_map(params![cid], |row| {
                let metadata_str: Option<String> = row.get(9)?;
                let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Annotation {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    row_id: row.get(2)?,
                    column_id: row.get(3)?,
                    severity: row.get(4)?,
                    message: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                    resolved: row.get::<_, i32>(8)? != 0,
                    metadata,
                })
            })
            .map_err(|e| format!("Failed to query annotations: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect annotations: {}", e))?,
        (None, None) => unreachable!(),
    };

    Ok(annotations)
}

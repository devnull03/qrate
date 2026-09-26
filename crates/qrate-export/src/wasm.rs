//! Browser adapter for the shared project reader and format writers.

use std::io::Cursor;

use rusqlite::{Connection, MAIN_DB, OptionalExtension};
use wasm_bindgen::prelude::*;

use crate::export::{CslMapping, ExportComponent};
use crate::{ColumnType, QRATE_APPLICATION_ID, QRATE_SCHEMA_VERSION, read_dataset};

#[derive(serde::Deserialize)]
struct DescriptionLevel {
    key: String,
    label: String,
}

fn error(code: &str, detail: impl std::fmt::Display) -> JsError {
    JsError::new(&format!("{code}: {detail}"))
}

#[wasm_bindgen]
pub struct Project {
    name: Option<String>,
    headers: Vec<String>,
    row_ids: Vec<i64>,
    rows: Vec<Vec<String>>,
    structure: Vec<ExportComponent>,
    types: Vec<(String, String)>,
    mapping: Option<CslMapping>,
    sheet_notes: Vec<crate::SheetNote>,
}

#[wasm_bindgen]
impl Project {
    pub fn open(bytes: Vec<u8>) -> Result<Project, JsError> {
        if !bytes.starts_with(b"SQLite format 3\0") {
            return Err(error("not-qrate", "not a SQLite file"));
        }
        let mut conn = Connection::open_in_memory().map_err(|e| error("corrupt", e))?;
        let len = bytes.len();
        conn.deserialize_read_exact(MAIN_DB, Cursor::new(bytes), len, true)
            .map_err(|e| error("corrupt", e))?;
        let pragma = |name: &str| -> Result<i32, JsError> {
            conn.pragma_query_value(None, name, |row| row.get(0))
                .map_err(|e| error("corrupt", e))
        };
        if pragma("application_id")? != QRATE_APPLICATION_ID {
            return Err(error("not-qrate", "application_id does not match"));
        }
        if pragma("user_version")? > QRATE_SCHEMA_VERSION {
            return Err(error("newer", "saved by a newer qrate"));
        }
        let (headers, row_ids, mut rows) = read_dataset(&conn).map_err(|e| error("corrupt", e))?;
        if rows.is_empty() {
            return Err(error("empty", "no dataset rows"));
        }
        let setting = |key: &str| -> Result<Option<String>, JsError> {
            conn.query_row(
                "SELECT value FROM __settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| error("corrupt", e))
        };
        let declared: Vec<(String, String, String)> = conn
            .prepare("SELECT name, data_type, COALESCE(notes, '') FROM __columns")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                    .collect()
            })
            .map_err(|e| error("corrupt", e))?;
        let types: Vec<(String, String)> = headers
            .iter()
            .map(|header| {
                let ty = declared
                    .iter()
                    .find(|(name, _, _)| name == header)
                    .map(|(_, ty, _)| ty.clone())
                    .unwrap_or_default();
                (header.clone(), ty)
            })
            .collect();
        let structure: Vec<ExportComponent> = crate::read_row_structure(&conn)
            .map_err(|e| error("corrupt", e))?
            .into_iter()
            .map(|item| ExportComponent {
                row_id: item.row_id,
                parent_id: item.parent_id,
                level_key: item.level_key,
                source_path: item.source_path,
            })
            .collect();
        let profile = setting("description_profile")?.unwrap_or_default();
        let levels: Vec<(String, String)> = setting("description_levels")?
            .and_then(|raw| serde_json::from_str::<Vec<DescriptionLevel>>(&raw).ok())
            .map(|items| {
                items
                    .into_iter()
                    .map(|item| (item.key, item.label))
                    .collect()
            })
            .unwrap_or_else(|| {
                crate::description::profile_levels(&profile)
                    .iter()
                    .map(|(key, label)| ((*key).into(), (*label).into()))
                    .collect()
            });
        let kinds = types
            .iter()
            .map(|(name, ty)| (name.clone(), ColumnType::from_declared(ty)))
            .collect::<Vec<_>>();
        crate::project_structure_columns(
            &headers, &row_ids, &mut rows, &structure, &kinds, &levels,
        );
        let project_notes = crate::read_project_notes(&conn).map_err(|e| error("corrupt", e))?;
        let column_notes = declared
            .iter()
            .map(|(name, _, notes)| (name.clone(), notes.clone()))
            .collect::<Vec<_>>();
        let sheet_notes = crate::sheet_notes(&headers, &row_ids, &column_notes, &project_notes);
        let mapping = setting("csl_mapping")?.and_then(|raw| serde_json::from_str(&raw).ok());
        Ok(Project {
            name: setting("name")?.filter(|value| !value.trim().is_empty()),
            headers,
            row_ids,
            rows,
            structure,
            types,
            mapping,
            sheet_notes,
        })
    }

    pub fn name(&self) -> Option<String> {
        self.name.clone()
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn headers(&self) -> Vec<String> {
        self.headers.clone()
    }
    pub fn preview(&self, n: usize) -> JsValue {
        serde_wasm_bindgen::to_value(&self.rows[..n.min(self.rows.len())]).unwrap_or(JsValue::NULL)
    }
    pub fn csl_fields() -> Vec<String> {
        crate::CSL_FIELDS
            .iter()
            .map(|field| field.to_string())
            .collect()
    }
    pub fn csl_default(&self) -> JsValue {
        use serde::Serialize as _;
        self.mapping
            .clone()
            .unwrap_or_else(|| crate::derive_csl_mapping(&self.types))
            .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
            .unwrap_or(JsValue::NULL)
    }
    pub fn to_csv(&self) -> Result<Vec<u8>, JsError> {
        crate::csv_bytes(&self.headers, &self.rows, crate::CsvOptions::default())
            .map_err(|e| error("write", e))
    }
    pub fn to_xlsx(&self) -> Result<Vec<u8>, JsError> {
        crate::xlsx_bytes(&self.headers, &self.rows, &self.sheet_notes)
            .map_err(|e| error("write", e))
    }
    pub fn to_jsonld(&self) -> Result<Vec<u8>, JsError> {
        serde_json::to_vec_pretty(&crate::jsonld_hierarchy_value(
            &self.headers,
            &self.row_ids,
            &self.rows,
            &self.structure,
        ))
        .map_err(|e| error("write", e))
    }
    pub fn to_csl(&self, mapping: JsValue) -> Result<Vec<u8>, JsError> {
        let mapping: CslMapping =
            serde_wasm_bindgen::from_value(mapping).map_err(|e| error("mapping", e))?;
        serde_json::to_vec_pretty(&crate::csl_items(&self.headers, &self.rows, &mapping))
            .map_err(|e| error("write", e))
    }
    pub fn sheet_values(&self) -> JsValue {
        let mut values = vec![&self.headers];
        values.extend(self.rows.iter());
        serde_wasm_bindgen::to_value(&values).unwrap_or(JsValue::NULL)
    }

    pub fn sheet_note_requests(&self, sheet_id: i32) -> JsValue {
        use serde::Serialize as _;
        crate::sheet_note_request_body(i64::from(sheet_id), &self.sheet_notes)
            .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
            .unwrap_or(JsValue::NULL)
    }
}

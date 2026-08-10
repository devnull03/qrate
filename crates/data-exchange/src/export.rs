//! Writing the open project back out: CSV, JSON-LD, Zotero's CSL-JSON, or a ZIP of all three
//! plus the images.
//!
//! Everything here takes the grid as plain `headers` + `rows` — the same pair
//! `table::save_now` persists — so nothing in this file needs a window or a project handle.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use settings::columns::ColumnType;
use thiserror::Error;
use zip::write::SimpleFileOptions;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("We couldn't write the file — {0}")]
    Io(#[from] std::io::Error),
    #[error("We couldn't write the spreadsheet — {0}")]
    Csv(#[from] csv::Error),
    #[error("We couldn't build the archive — {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// The grid as CSV: the header row, then every row in table order.
fn csv_bytes(headers: &[String], rows: &[Vec<String>]) -> Result<Vec<u8>, csv::Error> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(headers)?;
    for row in rows {
        writer.write_record(row)?;
    }
    writer.flush()?;
    writer.into_inner().map_err(|e| e.into_error().into())
}

pub fn write_csv(path: &Path, headers: &[String], rows: &[Vec<String>]) -> Result<(), ExportError> {
    Ok(File::create(path)?.write_all(&csv_bytes(headers, rows)?)?)
}

pub fn write_json(path: &Path, value: &Value) -> Result<(), ExportError> {
    Ok(serde_json::to_writer_pretty(File::create(path)?, value).map_err(std::io::Error::from)?)
}

/// One node per row, keyed by the column headers verbatim. The headers are the vocabulary a
/// collection already uses, so `@vocab` resolves them rather than a mapping table nobody wrote —
/// a reader gets terms that match the spreadsheet they came from.
pub fn jsonld_value(headers: &[String], rows: &[Vec<String>]) -> Value {
    let graph: Vec<Value> = rows
        .iter()
        .map(|row| {
            let node: Map<String, Value> = headers
                .iter()
                .zip(row)
                .filter(|(_, cell)| !cell.trim().is_empty())
                .map(|(header, cell)| (header.clone(), Value::String(cell.clone())))
                .collect();
            Value::Object(node)
        })
        .collect();
    json!({ "@context": { "@vocab": "https://schema.org/" }, "@graph": graph })
}

/// Which column feeds which CSL field, by header name. Persisted per project so the dialog opens
/// on last time's answer.
pub type CslMapping = BTreeMap<String, String>;

/// The fields worth asking about. `type` is not among them: CSL types are a controlled vocabulary,
/// and a free-text column pointed at it would produce items Zotero refuses.
pub const CSL_FIELDS: [&str; 5] = ["id", "title", "author", "issued", "URL"];

/// A first guess from the declared column types, which is what the mapping dialog opens on.
/// `columns` is every header paired with the type the project declares for it.
pub fn derive_csl_mapping(columns: &[(String, ColumnType)]) -> CslMapping {
    let mut mapping = CslMapping::new();
    let mut put = |field: &str, header: &String| {
        mapping.entry(field.to_string()).or_insert(header.clone());
    };
    for (header, kind) in columns {
        match kind {
            ColumnType::Identifier => put("id", header),
            ColumnType::Date => put("issued", header),
            ColumnType::Url => put("URL", header),
            ColumnType::Text => put("title", header),
            _ => {}
        }
    }
    mapping
}

/// CSL-JSON: what Zotero's File ▸ Import reads. Every column the mapping doesn't claim is kept as
/// a `note` line, so an import never silently drops a curator's work.
pub fn csl_items(headers: &[String], rows: &[Vec<String>], mapping: &CslMapping) -> Value {
    let column_of = |field: &str| {
        mapping
            .get(field)
            .and_then(|name| headers.iter().position(|h| h == name))
    };
    let fields: Vec<(&str, Option<usize>)> = CSL_FIELDS
        .iter()
        .map(|field| (*field, column_of(field)))
        .collect();

    let items: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(ix, row)| {
            let cell = |at: Option<usize>| {
                at.and_then(|i| row.get(i))
                    .map(|c| c.trim())
                    .filter(|c| !c.is_empty())
            };
            let mut item = Map::new();
            item.insert("type".into(), "document".into());
            for (field, at) in &fields {
                let Some(value) = cell(*at) else { continue };
                item.insert(
                    (*field).into(),
                    match *field {
                        // A raw date, so an EDTF range or a circa year survives instead of being
                        // rounded into date-parts it doesn't fit.
                        "issued" => json!({ "raw": value }),
                        "author" => json!([{ "literal": value }]),
                        _ => Value::String(value.into()),
                    },
                );
            }
            item.entry("id")
                .or_insert_with(|| Value::String(format!("row-{ix}")));

            let claimed: HashSet<usize> = fields.iter().filter_map(|(_, at)| *at).collect();
            let note: Vec<String> = headers
                .iter()
                .enumerate()
                .filter(|(i, _)| !claimed.contains(i))
                .filter_map(|(i, header)| cell(Some(i)).map(|v| format!("{header}: {v}")))
                .collect();
            if !note.is_empty() {
                item.insert("note".into(), Value::String(note.join("\n")));
            }
            Value::Object(item)
        })
        .collect();
    Value::Array(items)
}

/// The whole collection as one file: the grid in both formats, plus every image a row resolved to.
/// `images` comes from `table::photos::resolve_row_images`, which is the same resolution the
/// Details panel shows — what you see in the app is what lands in `files/`.
pub fn write_zip(
    path: &Path,
    headers: &[String],
    rows: &[Vec<String>],
    images: &[PathBuf],
) -> Result<(), ExportError> {
    let mut zip = zip::ZipWriter::new(File::create(path)?);
    let text = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    // Photos are already compressed; deflating a JPEG spends CPU to save nothing.
    let binary = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip.start_file("data.csv", text)?;
    zip.write_all(&csv_bytes(headers, rows)?)?;
    zip.start_file("metadata.jsonld", text)?;
    zip.write_all(
        &serde_json::to_vec_pretty(&jsonld_value(headers, rows)).map_err(std::io::Error::from)?,
    )?;

    let mut taken: HashSet<String> = HashSet::new();
    for image in images {
        let Some(name) = image.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Two folders can hold the same filename, and a zip entry that repeats one silently wins.
        let mut name = name.to_string();
        for n in 2.. {
            if taken.insert(name.clone()) {
                break;
            }
            let stem = Path::new(&name).file_stem().unwrap_or_default();
            name = format!("{}_{n}", stem.to_string_lossy());
            if let Some(ext) = image.extension().and_then(|e| e.to_str()) {
                name = format!("{name}.{ext}");
            }
        }
        match std::fs::read(image) {
            Ok(bytes) => {
                zip.start_file(format!("files/{name}"), binary)?;
                zip.write_all(&bytes)?;
            }
            Err(err) => log::warn!("left {} out of the export archive: {err}", image.display()),
        }
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CslMapping, csl_items, derive_csl_mapping, jsonld_value, write_zip};
    use settings::columns::ColumnType;

    fn grid() -> (Vec<String>, Vec<Vec<String>>) {
        let headers = ["Digital ID", "Title", "Taken", "Notes"]
            .map(String::from)
            .to_vec();
        let rows = vec![
            ["1", "First photo", "1943", "on loan"]
                .map(String::from)
                .to_vec(),
            ["2", "Second photo", "", ""].map(String::from).to_vec(),
        ];
        (headers, rows)
    }

    #[test]
    fn jsonld_keeps_column_order_and_drops_empty_cells() {
        let (headers, rows) = grid();
        let doc = jsonld_value(&headers, &rows);
        let graph = doc["@graph"].as_array().unwrap();
        assert_eq!(graph.len(), 2);
        assert_eq!(
            graph[0].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["Digital ID", "Title", "Taken", "Notes"]
        );
        // The second row's blanks are absent, not empty strings — a null value is a claim.
        assert_eq!(graph[1].as_object().unwrap().len(), 2);
    }

    #[test]
    fn csl_maps_declared_types_and_keeps_the_rest_as_a_note() {
        let (headers, rows) = grid();
        let mapping = derive_csl_mapping(&[
            ("Digital ID".into(), ColumnType::Identifier),
            ("Title".into(), ColumnType::Text),
            ("Taken".into(), ColumnType::Date),
            ("Notes".into(), ColumnType::Text),
        ]);
        let items = csl_items(&headers, &rows, &mapping);

        assert_eq!(items[0]["id"], "1");
        assert_eq!(items[0]["title"], "First photo");
        // Raw, so "1943" or "circa 1943" survives whole.
        assert_eq!(items[0]["issued"]["raw"], "1943");
        assert_eq!(items[0]["type"], "document");
        // `Notes` lost the race for `title`, so it has to show up somewhere.
        assert_eq!(items[0]["note"], "Notes: on loan");
        assert!(items[1].get("note").is_none());
    }

    #[test]
    fn an_unmapped_id_falls_back_to_the_row_number() {
        let (headers, rows) = grid();
        let items = csl_items(&headers, &rows, &CslMapping::new());
        assert_eq!(items[0]["id"], "row-0");
        assert_eq!(items[1]["id"], "row-1");
    }

    #[test]
    fn the_archive_carries_both_formats_and_the_images() {
        let dir = std::env::temp_dir().join("qrate-export-zip-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(dir.join("a/1.jpg"), "one").unwrap();
        std::fs::write(dir.join("b/1.jpg"), "also one").unwrap();

        let (headers, rows) = grid();
        let archive = dir.join("out.zip");
        write_zip(
            &archive,
            &headers,
            &rows,
            &[dir.join("a/1.jpg"), dir.join("b/1.jpg")],
        )
        .unwrap();

        let zip = zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
        // Two files sharing a name both survive; the second is renamed rather than overwriting.
        assert_eq!(
            zip.file_names().collect::<std::collections::HashSet<_>>(),
            [
                "data.csv",
                "metadata.jsonld",
                "files/1.jpg",
                "files/1_2.jpg"
            ]
            .into_iter()
            .collect()
        );
    }
}

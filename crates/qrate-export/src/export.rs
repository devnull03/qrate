//! Writing the open project back out: CSV, Excel, JSON-LD, Zotero's CSL-JSON, or a ZIP of all three
//! plus the images.
//!
//! Everything here takes the grid as plain `headers` + `rows` — the same pair
//! `table::save_now` persists — so nothing in this file needs a window or a project handle.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{ColumnType, SheetNote};
use serde_json::{Map, Value, json};
use thiserror::Error;
use zip::write::SimpleFileOptions;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("We couldn't write the file — {0}")]
    Io(#[from] std::io::Error),
    #[error("We couldn't write the spreadsheet — {0}")]
    Csv(#[from] csv::Error),
    #[error("We couldn't write the Excel workbook — {0}")]
    Xlsx(#[from] rust_xlsxwriter::XlsxError),
    #[error("The grid cannot fit in one Excel worksheet")]
    XlsxGridLimit,
    #[error("We couldn't build the archive — {0}")]
    Zip(#[from] zip::result::ZipError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveFile {
    pub path: PathBuf,
    pub source_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportComponent {
    pub row_id: i64,
    pub parent_id: Option<i64>,
    pub level_key: String,
    pub source_path: Option<String>,
}

/// How a CSV is written. The default is plain RFC 4180, what a script or the wizard reads back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsvOptions {
    pub delimiter: u8,
    /// Lead with a UTF-8 byte order mark, without which Excel on Windows reads the file as ANSI.
    pub bom: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            bom: false,
        }
    }
}

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// The grid as CSV: the header row, then every row in table order.
pub fn csv_bytes(
    headers: &[String],
    rows: &[Vec<String>],
    options: CsvOptions,
) -> Result<Vec<u8>, csv::Error> {
    let bom = if options.bom {
        UTF8_BOM.to_vec()
    } else {
        Vec::new()
    };
    let mut writer = csv::WriterBuilder::new()
        .delimiter(options.delimiter)
        .from_writer(bom);
    writer.write_record(headers)?;
    for row in rows {
        writer.write_record(row)?;
    }
    writer.flush()?;
    writer.into_inner().map_err(|e| e.into_error().into())
}

pub fn write_csv(
    path: &Path,
    headers: &[String],
    rows: &[Vec<String>],
    options: CsvOptions,
) -> Result<(), ExportError> {
    Ok(File::create(path)?.write_all(&csv_bytes(headers, rows, options)?)?)
}

/// Write every cell as text so Excel preserves identifiers, dates, and leading zeroes exactly.
pub fn xlsx_bytes(
    headers: &[String],
    rows: &[Vec<String>],
    notes: &[SheetNote],
) -> Result<Vec<u8>, ExportError> {
    if headers.len() > 16_384 || rows.len() > 1_048_575 || rows.iter().any(|row| row.len() > 16_384)
    {
        return Err(ExportError::XlsxGridLimit);
    }
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Catalog")?;
    let bold = rust_xlsxwriter::Format::new().set_bold();
    for (col, header) in headers.iter().enumerate() {
        sheet.write_string_with_format(0, col as u16, header, &bold)?;
    }
    for (row, cells) in rows.iter().enumerate() {
        for (col, value) in cells.iter().enumerate() {
            sheet.write_string((row + 1) as u32, col as u16, value)?;
        }
    }
    for note in notes {
        sheet.insert_note(note.row, note.col, &rust_xlsxwriter::Note::new(&note.text))?;
    }
    sheet.set_freeze_panes(1, 0)?;
    Ok(workbook.save_to_buffer()?)
}

pub fn write_xlsx(
    path: &Path,
    headers: &[String],
    rows: &[Vec<String>],
    notes: &[SheetNote],
) -> Result<(), ExportError> {
    Ok(std::fs::write(path, xlsx_bytes(headers, rows, notes)?)?)
}

pub fn write_json(path: &Path, value: &Value) -> Result<(), ExportError> {
    Ok(serde_json::to_writer_pretty(File::create(path)?, value).map_err(std::io::Error::from)?)
}

/// Fills user-declared archival projection columns without adding columns to the dataset.
pub fn project_structure_columns(
    headers: &[String],
    row_ids: &[i64],
    rows: &mut [Vec<String>],
    structure: &[ExportComponent],
    columns: &[(String, ColumnType)],
    description: &[(String, String)],
) {
    let by_id: std::collections::HashMap<_, _> = structure
        .iter()
        .map(|component| (component.row_id, component))
        .collect();
    let title_col = columns
        .iter()
        .find(|(_, kind)| *kind == ColumnType::Title)
        .and_then(|(name, _)| headers.iter().position(|header| header == name));
    let titles: std::collections::HashMap<_, _> = title_col
        .map(|title| {
            row_ids
                .iter()
                .zip(rows.iter())
                .filter_map(|(id, row)| row.get(title).map(|value| (*id, value.clone())))
                .collect()
        })
        .unwrap_or_default();
    let level_labels: std::collections::HashMap<_, _> = description
        .iter()
        .map(|(key, label)| (key.as_str(), label.as_str()))
        .collect();
    for (source, row) in rows.iter_mut().enumerate() {
        let Some(component) = row_ids.get(source).and_then(|id| by_id.get(id)) else {
            continue;
        };
        for (name, kind) in columns {
            let Some(col) = headers.iter().position(|header| header == name) else {
                continue;
            };
            // A value the archivist typed stays theirs; only a blank cell is filled from structure.
            if row.get(col).is_none_or(|value| !value.trim().is_empty()) {
                continue;
            }
            row[col] = match kind {
                ColumnType::DescriptionLevel => level_labels
                    .get(component.level_key.as_str())
                    .copied()
                    .unwrap_or(component.level_key.as_str())
                    .to_string(),
                ColumnType::ParentComponent => component
                    .parent_id
                    .map(|parent_id| {
                        titles
                            .get(&parent_id)
                            .filter(|title| !title.trim().is_empty())
                            .cloned()
                            .unwrap_or_else(|| parent_id.to_string())
                    })
                    .unwrap_or_default(),
                ColumnType::SourcePath => component.source_path.clone().unwrap_or_default(),
                _ => continue,
            };
        }
    }
}

/// One node per row, keyed by the column headers verbatim. The headers are the vocabulary a
/// collection already uses, so `@vocab` resolves them rather than a mapping table nobody wrote —
/// a reader gets terms that match the spreadsheet they came from. Each node also carries a stable
/// component `@id` and, when it has a parent, an `isPartOf` whole-part link.
pub fn jsonld_hierarchy_value(
    headers: &[String],
    row_ids: &[i64],
    rows: &[Vec<String>],
    structure: &[ExportComponent],
) -> Value {
    let structure: std::collections::HashMap<_, _> =
        structure.iter().map(|item| (item.row_id, item)).collect();
    let graph: Vec<Value> = rows
        .iter()
        .zip(row_ids)
        .map(|(row, row_id)| {
            let mut node: Map<String, Value> = headers
                .iter()
                .zip(row)
                .filter(|(_, cell)| !cell.trim().is_empty())
                .map(|(header, cell)| (header.clone(), Value::String(cell.clone())))
                .collect();
            node.insert("@id".into(), format!("urn:qrate:component:{row_id}").into());
            if let Some(component) = structure.get(row_id) {
                node.insert("additionalType".into(), component.level_key.clone().into());
                if let Some(parent_id) = component.parent_id {
                    node.insert(
                        "isPartOf".into(),
                        json!({ "@id": format!("urn:qrate:component:{parent_id}") }),
                    );
                }
            }
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
pub fn derive_csl_mapping(columns: &[(String, String)]) -> CslMapping {
    let mut mapping = CslMapping::new();
    fn put(mapping: &mut CslMapping, field: &str, header: &str) {
        mapping
            .entry(field.to_string())
            .or_insert(header.to_owned());
    }
    for (header, declared) in columns {
        let kind = ColumnType::from_declared(declared);
        match kind {
            ColumnType::Title => {
                mapping.insert("title".to_string(), header.clone());
            }
            ColumnType::Identifier => put(&mut mapping, "id", header),
            ColumnType::Date => put(&mut mapping, "issued", header),
            ColumnType::Url => put(&mut mapping, "URL", header),
            ColumnType::Text => put(&mut mapping, "title", header),
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
    row_ids: &[i64],
    rows: &[Vec<String>],
    structure: &[ExportComponent],
    images: &[ArchiveFile],
    csv: CsvOptions,
) -> Result<(), ExportError> {
    zip_to(
        File::create(path)?,
        headers,
        row_ids,
        rows,
        structure,
        images,
        csv,
    )
}

pub fn zip_to(
    writer: impl std::io::Write + std::io::Seek,
    headers: &[String],
    row_ids: &[i64],
    rows: &[Vec<String>],
    structure: &[ExportComponent],
    images: &[ArchiveFile],
    csv: CsvOptions,
) -> Result<(), ExportError> {
    let mut zip = zip::ZipWriter::new(writer);
    let text = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    // Photos are already compressed; deflating a JPEG spends CPU to save nothing.
    let binary = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip.start_file("data.csv", text)?;
    zip.write_all(&csv_bytes(headers, rows, csv)?)?;
    zip.start_file("metadata.jsonld", text)?;
    zip.write_all(
        &serde_json::to_vec_pretty(&jsonld_hierarchy_value(headers, row_ids, rows, structure))
            .map_err(std::io::Error::from)?,
    )?;

    let mut taken: HashSet<String> = HashSet::new();
    for image in images {
        let Some(fallback) = image.path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let relative = image
            .source_path
            .as_deref()
            .and_then(safe_archive_path)
            .unwrap_or_else(|| fallback.to_string());
        // Two folders can hold the same filename, and a zip entry that repeats one silently wins.
        let mut name = relative;
        for n in 2.. {
            if taken.insert(name.clone()) {
                break;
            }
            let stem = Path::new(&name).file_stem().unwrap_or_default();
            let parent = Path::new(&name).parent().unwrap_or_else(|| Path::new(""));
            let mut renamed = format!("{}_{n}", stem.to_string_lossy());
            if let Some(ext) = image.path.extension().and_then(|e| e.to_str()) {
                renamed = format!("{renamed}.{ext}");
            }
            name = parent.join(renamed).to_string_lossy().replace('\\', "/");
        }
        match File::open(&image.path) {
            Ok(mut file) => {
                zip.start_file(format!("files/{name}"), binary)?;
                std::io::copy(&mut file, &mut zip)?;
            }
            Err(err) => log::warn!(
                "left {} out of the export archive: {err}",
                image.path.display()
            ),
        }
    }
    zip.finish()?;
    Ok(())
}

fn safe_archive_path(source: &str) -> Option<String> {
    let path = Path::new(source);
    if path.is_absolute() || source.trim().is_empty() {
        return None;
    }
    let safe: PathBuf = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    (!safe.as_os_str().is_empty()).then(|| safe.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveFile, CslMapping, ExportComponent, csl_items, derive_csl_mapping,
        jsonld_hierarchy_value, project_structure_columns, write_xlsx, write_zip,
    };
    use crate::ColumnType;

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
    fn csv_golden_keeps_header_and_row_order() {
        let (headers, rows) = grid();
        assert_eq!(
            super::csv_bytes(&headers, &rows, super::CsvOptions::default()).unwrap(),
            b"Digital ID,Title,Taken,Notes\n1,First photo,1943,on loan\n2,Second photo,,\n"
        );
    }

    /// Excel on Windows needs the BOM to read UTF-8, and a semicolon-locale Excel splits on `;`.
    #[test]
    fn csv_options_add_a_bom_and_change_the_delimiter() {
        let headers = vec!["Title".to_string(), "Place".to_string()];
        let rows = vec![vec!["Café; bar".to_string(), "Montréal".to_string()]];
        let bytes = super::csv_bytes(
            &headers,
            &rows,
            super::CsvOptions {
                delimiter: b';',
                bom: true,
            },
        )
        .unwrap();
        assert_eq!(&bytes[..3], &super::UTF8_BOM);
        assert_eq!(
            std::str::from_utf8(&bytes[3..]).unwrap(),
            "Title;Place\n\"Café; bar\";Montréal\n"
        );

        let tabbed = super::csv_bytes(
            &headers,
            &rows,
            super::CsvOptions {
                delimiter: b'\t',
                bom: false,
            },
        )
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&tabbed).unwrap(),
            "Title\tPlace\nCafé; bar\tMontréal\n"
        );
    }

    #[test]
    fn xlsx_round_trip_keeps_identifiers_and_dates_as_text() {
        use calamine::{DataType as _, Reader as _};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.xlsx");
        write_xlsx(
            &path,
            &["ID".into(), "Date".into()],
            &[vec!["00123".into(), "2026-09-22".into()]],
            &[crate::SheetNote {
                row: 1,
                col: 1,
                text: "circa".into(),
            }],
        )
        .unwrap();
        let mut workbook = calamine::open_workbook_auto(&path).unwrap();
        let range = workbook.worksheet_range_at(0).unwrap().unwrap();
        assert_eq!(range.get((0, 0)).unwrap().get_string(), Some("ID"));
        assert_eq!(range.get((1, 0)).unwrap().get_string(), Some("00123"));
        assert_eq!(range.get((1, 1)).unwrap().get_string(), Some("2026-09-22"));
        use std::io::Read as _;
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut workbook_xml = String::new();
        archive
            .by_name("xl/workbook.xml")
            .unwrap()
            .read_to_string(&mut workbook_xml)
            .unwrap();
        assert!(workbook_xml.contains("name=\"Catalog\""));
        let mut sheet_xml = String::new();
        archive
            .by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut sheet_xml)
            .unwrap();
        assert!(sheet_xml.contains("ySplit=\"1\""));
        let mut comments_xml = String::new();
        archive
            .by_name("xl/comments1.xml")
            .unwrap()
            .read_to_string(&mut comments_xml)
            .unwrap();
        assert!(comments_xml.contains("ref=\"B2\""));
        assert!(comments_xml.contains("circa"));
    }

    #[test]
    fn jsonld_keeps_column_order_and_drops_empty_cells() {
        let (headers, rows) = grid();
        let doc = jsonld_hierarchy_value(&headers, &[1, 2], &rows, &[]);
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/catalog.jsonld")).unwrap();
        assert_eq!(doc, golden);
        let graph = doc["@graph"].as_array().unwrap();
        assert_eq!(graph.len(), 2);
        assert_eq!(
            graph[0].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["Digital ID", "Title", "Taken", "Notes", "@id"]
        );
        // The second row's blanks are absent, not empty strings — a null value is a claim.
        assert_eq!(graph[1].as_object().unwrap().len(), 3);
    }

    #[test]
    fn jsonld_exports_archival_whole_part_relationships() {
        let (headers, rows) = grid();
        let structure = [
            ExportComponent {
                row_id: 10,
                parent_id: None,
                level_key: "series".into(),
                source_path: None,
            },
            ExportComponent {
                row_id: 11,
                parent_id: Some(10),
                level_key: "item".into(),
                source_path: None,
            },
        ];
        let graph = jsonld_hierarchy_value(&headers, &[10, 11], &rows, &structure);
        assert_eq!(graph["@graph"][0]["additionalType"], "series");
        assert_eq!(
            graph["@graph"][1]["isPartOf"]["@id"],
            "urn:qrate:component:10"
        );
    }

    #[test]
    fn declared_structure_columns_are_filled_without_adding_columns() {
        let headers = vec![
            "Title".into(),
            "Level".into(),
            "Parent".into(),
            "Path".into(),
        ];
        let mut rows = vec![
            vec![
                "Photographs".into(),
                "Fonds".into(),
                String::new(),
                String::new(),
            ],
            vec![
                "one.jpg".into(),
                String::new(),
                String::new(),
                String::new(),
            ],
        ];
        let structure = [
            ExportComponent {
                row_id: 10,
                parent_id: None,
                level_key: "series".into(),
                source_path: Some("Photographs".into()),
            },
            ExportComponent {
                row_id: 11,
                parent_id: Some(10),
                level_key: "item".into(),
                source_path: Some("Photographs/one.jpg".into()),
            },
        ];
        project_structure_columns(
            &headers,
            &[10, 11],
            &mut rows,
            &structure,
            &[
                ("Title".into(), ColumnType::Title),
                ("Level".into(), ColumnType::DescriptionLevel),
                ("Parent".into(), ColumnType::ParentComponent),
                ("Path".into(), ColumnType::SourcePath),
            ],
            &[
                ("series".into(), "Series".into()),
                ("item".into(), "Item".into()),
            ],
        );
        assert_eq!(
            rows[1],
            ["one.jpg", "Item", "Photographs", "Photographs/one.jpg"]
        );
        assert_eq!(rows[0][1], "Fonds", "a typed value is never overwritten");
        assert_eq!(headers.len(), 4);
    }

    #[test]
    fn csl_maps_declared_types_and_keeps_the_rest_as_a_note() {
        let (headers, rows) = grid();
        let mapping = derive_csl_mapping(&[
            ("Digital ID".into(), "Identifier".into()),
            ("Notes".into(), "Text".into()),
            ("Title".into(), "Title".into()),
            ("Taken".into(), "Date".into()),
            ("Link".into(), "Url".into()),
        ]);
        assert_eq!(mapping.get("URL").map(String::as_str), Some("Link"));
        let items = csl_items(&headers, &rows, &mapping);
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/catalog.csl.json")).unwrap();
        assert_eq!(items, golden);

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
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/catalog-unmapped.csl.json"))
                .unwrap();
        assert_eq!(items, golden);
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
            &[10, 11],
            &rows,
            &[],
            &[
                ArchiveFile {
                    path: dir.join("a/1.jpg"),
                    source_path: Some("a/1.jpg".into()),
                },
                ArchiveFile {
                    path: dir.join("b/1.jpg"),
                    source_path: Some("b/1.jpg".into()),
                },
            ],
            super::CsvOptions::default(),
        )
        .unwrap();

        let zip = zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
        // Two files sharing a name both survive in their source directories.
        assert_eq!(
            zip.file_names().collect::<std::collections::HashSet<_>>(),
            [
                "data.csv",
                "metadata.jsonld",
                "files/a/1.jpg",
                "files/b/1.jpg"
            ]
            .into_iter()
            .collect()
        );
    }
}

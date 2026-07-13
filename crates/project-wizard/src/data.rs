//! Real CSV parsing and folder-matching logic used by the Files, Link, and
//! Columns steps. Google Sheets are fetched as `.xlsx` by the `cloud-sync`
//! crate and adapted into a [`SpreadsheetPreview`] (see the `From` impl below),
//! then run through the same [`match_folder`] path as local CSV files.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct SpreadsheetPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl SpreadsheetPreview {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn column_count(&self) -> usize {
        self.headers.len()
    }
}

impl From<cloud_sync::SheetData> for SpreadsheetPreview {
    // ponytail: notes are dropped here — the wizard preview has no field for
    // them yet. Carry `sheet.notes` through when the Problems/column-notes
    // panels land (see the ".qrate project file" + "Problems panel" tasks).
    fn from(sheet: cloud_sync::SheetData) -> Self {
        Self {
            headers: sheet.headers,
            rows: sheet.rows,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SpreadsheetError {
    NotCsv,
    Empty,
    NoHeaderRow,
    Io(String),
}

impl SpreadsheetError {
    pub fn message(&self) -> String {
        match self {
            SpreadsheetError::NotCsv => "That doesn't look like a CSV file".into(),
            SpreadsheetError::Empty => "This spreadsheet has no rows yet".into(),
            SpreadsheetError::NoHeaderRow => {
                "We couldn't find a header row — the first row looks like data".into()
            }
            SpreadsheetError::Io(e) => format!("We couldn't open that file — {e}"),
        }
    }
}

pub fn load_csv_preview(path: &str) -> Result<SpreadsheetPreview, SpreadsheetError> {
    let p = Path::new(path);
    let looks_like_csv = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("csv"))
        .unwrap_or(false);
    if !looks_like_csv {
        return Err(SpreadsheetError::NotCsv);
    }

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(p)
        .map_err(|e| SpreadsheetError::Io(e.to_string()))?;

    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| SpreadsheetError::Io(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect();

    if headers.iter().all(|h| h.trim().is_empty()) {
        return Err(SpreadsheetError::Empty);
    }
    // Heuristic: if every header cell parses as a bare number, the "header"
    // row is almost certainly a data row and there's no real header.
    if headers.iter().all(|h| h.trim().parse::<f64>().is_ok()) {
        return Err(SpreadsheetError::NoHeaderRow);
    }

    let mut rows = Vec::new();
    for result in rdr.records() {
        let record = result.map_err(|e| SpreadsheetError::Io(e.to_string()))?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }
    if rows.is_empty() {
        return Err(SpreadsheetError::Empty);
    }

    Ok(SpreadsheetPreview { headers, rows })
}

#[derive(Clone, Debug)]
pub struct SheetCheckResult {
    pub title: String,
    pub row_count: usize,
    pub used_first_tab: bool,
}

#[derive(Clone, Debug)]
pub struct FolderMatch {
    pub matched_rows: usize,
    pub total_rows: usize,
    pub extra_files: usize,
}

#[derive(Clone, Debug)]
pub enum FolderError {
    NotFound,
    Permission,
    Empty,
    NoMatches,
}

impl FolderError {
    pub fn message(&self) -> &'static str {
        match self {
            FolderError::NotFound | FolderError::NoMatches => {
                "This folder doesn't look right — we didn't find any files that match your spreadsheet here"
            }
            FolderError::Permission => {
                "qrate doesn't have permission to open this folder — check its settings and try again"
            }
            FolderError::Empty => "This folder is empty",
        }
    }
}

pub fn list_files(folder: &str) -> Result<Vec<String>, FolderError> {
    let dir = Path::new(folder);
    if !dir.is_dir() {
        return Err(FolderError::NotFound);
    }
    let entries = fs::read_dir(dir).map_err(|_| FolderError::Permission)?;
    let files: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    if files.is_empty() {
        return Err(FolderError::Empty);
    }
    Ok(files)
}

/// Matches spreadsheet rows against files in `folder` by looking for any cell
/// whose value equals a filename (or filename stem) in the folder, case
/// insensitively — the same rule the Link step's "match by exact filename"
/// uses, just summarized here for the Files step's inline validation.
pub fn match_folder(
    preview: &SpreadsheetPreview,
    folder: &str,
) -> Result<FolderMatch, FolderError> {
    let files = list_files(folder)?;
    let file_stems: HashSet<String> = files.iter().map(|f| f.to_lowercase()).collect();
    let file_names_no_ext: HashSet<String> = files
        .iter()
        .map(|f| {
            Path::new(f)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(f)
                .to_lowercase()
        })
        .collect();

    let mut matched_rows = 0usize;
    let mut used_files: HashSet<String> = HashSet::new();
    for row in &preview.rows {
        let hit = row.iter().find_map(|cell| {
            let c = cell.trim().to_lowercase();
            if c.is_empty() {
                return None;
            }
            if file_stems.contains(&c) {
                return Some(c);
            }
            if file_names_no_ext.contains(&c) {
                return Some(c);
            }
            None
        });
        if let Some(hit) = hit {
            matched_rows += 1;
            used_files.insert(hit);
        }
    }

    if matched_rows == 0 {
        return Err(FolderError::NoMatches);
    }

    Ok(FolderMatch {
        matched_rows,
        total_rows: preview.rows.len(),
        extra_files: files.len().saturating_sub(used_files.len()),
    })
}

#[derive(Clone, Debug)]
pub struct ColumnConfigEntry {
    pub name: String,
    pub data_type: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct ColumnConfigPreview {
    pub entries: Vec<ColumnConfigEntry>,
}

#[derive(Clone, Debug)]
pub enum ColumnConfigError {
    MissingDataType,
    DuplicateNames(String),
    NoMatch,
    Io(String),
}

impl ColumnConfigError {
    pub fn message(&self) -> String {
        match self {
            ColumnConfigError::MissingDataType => {
                "This config is missing a Data Type column — we need that to load it".into()
            }
            ColumnConfigError::DuplicateNames(name) => {
                format!("Two columns are both named '{name}' — column names must be unique")
            }
            ColumnConfigError::NoMatch => {
                "This doesn't mention any columns from your spreadsheet — double check you picked the right file".into()
            }
            ColumnConfigError::Io(e) => format!("We couldn't open that file — {e}"),
        }
    }
}

/// Loads a `column_config.csv`-shaped file (Column Name, Data Type,
/// Description) and checks it against the spreadsheet's own headers.
pub fn load_column_config(
    path: &str,
    against_headers: &[String],
) -> Result<ColumnConfigPreview, ColumnConfigError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|e| ColumnConfigError::Io(e.to_string()))?;

    let headers = rdr
        .headers()
        .map_err(|e| ColumnConfigError::Io(e.to_string()))?
        .clone();

    let name_ix = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("Column Name"));
    let type_ix = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("Data Type"));
    let desc_ix = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("Description"));

    let (Some(name_ix), Some(type_ix)) = (name_ix, type_ix) else {
        return Err(ColumnConfigError::MissingDataType);
    };

    let mut entries = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for result in rdr.records() {
        let record = result.map_err(|e| ColumnConfigError::Io(e.to_string()))?;
        let name = record.get(name_ix).unwrap_or_default().trim().to_string();
        if name.is_empty() {
            continue;
        }
        if !seen.insert(name.to_lowercase()) {
            return Err(ColumnConfigError::DuplicateNames(name));
        }
        entries.push(ColumnConfigEntry {
            name: name.clone(),
            data_type: record.get(type_ix).unwrap_or_default().trim().to_string(),
            description: desc_ix
                .and_then(|i| record.get(i))
                .unwrap_or_default()
                .trim()
                .to_string(),
        });
    }

    let matches_any = against_headers.is_empty()
        || entries.iter().any(|e| {
            against_headers
                .iter()
                .any(|h| h.eq_ignore_ascii_case(&e.name))
        });
    if !matches_any {
        return Err(ColumnConfigError::NoMatch);
    }

    Ok(ColumnConfigPreview { entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sample")
            .canonicalize()
            .expect("sample/ directory present in repo")
    }

    #[test]
    fn loads_sample_csv() {
        let csv = sample_dir().join("aderman_collection.csv");
        let preview = load_csv_preview(csv.to_str().unwrap()).unwrap();
        assert_eq!(preview.headers[0], "Digital ID");
        assert_eq!(preview.row_count(), 4);
        assert_eq!(preview.column_count(), 6);
    }

    #[test]
    fn rejects_non_csv() {
        assert!(matches!(
            load_csv_preview("/tmp/whatever.txt"),
            Err(SpreadsheetError::NotCsv)
        ));
    }

    #[test]
    fn matches_sample_photos_by_digital_id() {
        let csv = sample_dir().join("aderman_collection.csv");
        let preview = load_csv_preview(csv.to_str().unwrap()).unwrap();
        let photos = sample_dir().join("photos");
        // Digital IDs 1-4 match photos/1.jpg..4.jpg by filename stem.
        let m = match_folder(&preview, photos.to_str().unwrap()).unwrap();
        assert_eq!(m.matched_rows, 4);
        assert_eq!(m.total_rows, 4);
        assert_eq!(m.extra_files, 0);
    }

    #[test]
    fn folder_errors() {
        let csv = sample_dir().join("aderman_collection.csv");
        let preview = load_csv_preview(csv.to_str().unwrap()).unwrap();
        assert!(matches!(
            match_folder(&preview, "/nonexistent/folder"),
            Err(FolderError::NotFound)
        ));
        let empty = tempdir("qrate-empty-folder");
        assert!(matches!(
            match_folder(&preview, empty.to_str().unwrap()),
            Err(FolderError::Empty)
        ));
    }

    #[test]
    fn column_config_round_trip_and_errors() {
        let dir = tempdir("qrate-config-test");

        let good = dir.join("column_config.csv");
        write!(
            std::fs::File::create(&good).unwrap(),
            "Column Name,Data Type,Description\nTitle,Text,The title\nDate Created,Date (YYYY-MM-DD),When made\n"
        )
        .unwrap();
        let headers = vec!["Title".to_string(), "Creator".to_string()];
        let preview = load_column_config(good.to_str().unwrap(), &headers).unwrap();
        assert_eq!(preview.entries.len(), 2);
        assert_eq!(preview.entries[0].data_type, "Text");

        let missing_type = dir.join("missing_type.csv");
        write!(
            std::fs::File::create(&missing_type).unwrap(),
            "Column Name,Description\nTitle,The title\n"
        )
        .unwrap();
        assert!(matches!(
            load_column_config(missing_type.to_str().unwrap(), &headers),
            Err(ColumnConfigError::MissingDataType)
        ));

        let dupes = dir.join("dupes.csv");
        write!(
            std::fs::File::create(&dupes).unwrap(),
            "Column Name,Data Type\nTitle,Text\ntitle,Text\n"
        )
        .unwrap();
        assert!(matches!(
            load_column_config(dupes.to_str().unwrap(), &headers),
            Err(ColumnConfigError::DuplicateNames(_))
        ));

        let unrelated = dir.join("unrelated.csv");
        write!(
            std::fs::File::create(&unrelated).unwrap(),
            "Column Name,Data Type\nSomething Else,Text\n"
        )
        .unwrap();
        assert!(matches!(
            load_column_config(unrelated.to_str().unwrap(), &headers),
            Err(ColumnConfigError::NoMatch)
        ));
    }

    fn tempdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

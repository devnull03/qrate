//! Real CSV parsing and folder-matching logic used by the Files, Link, and
//! Columns steps. Google Sheets are fetched as `.xlsx` by the `cloud-sync`
//! crate and adapted into a [`SpreadsheetPreview`] (see the `From` impl below),
//! then run through the same [`match_folder`] path as local CSV files.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use settings::columns::ColumnType;

/// A note carried in from the sheet, resolved against this preview's own indices: `row` indexes
/// [`SpreadsheetPreview::rows`] and `column` is the header text, so nothing downstream has to
/// know about spreadsheet coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewNote {
    pub row: usize,
    pub column: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct SpreadsheetPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Cell notes, when the source carried any. CSV has no way to express them.
    pub notes: Vec<PreviewNote>,
}

impl From<cloud_sync::SheetData> for SpreadsheetPreview {
    fn from(sheet: cloud_sync::SheetData) -> Self {
        let (top, left) = sheet.origin;
        let notes = sheet
            .notes
            .iter()
            .filter_map(|n| {
                // Note refs are absolute sheet coordinates; the rows start at the used range's
                // origin, and the first of those is the header.
                let (r, c) = cloud_sync::a1_to_index(&n.cell)?;
                let row = r.checked_sub(top)?.checked_sub(1)? as usize;
                let column = sheet.headers.get(c.checked_sub(left)? as usize)?.clone();
                // An orphan outside the imported grid can't be shown or jumped to.
                (row < sheet.rows.len()).then(|| PreviewNote {
                    row,
                    column,
                    text: n.text.clone(),
                })
            })
            .collect();
        Self {
            headers: sheet.headers,
            rows: sheet.rows,
            notes,
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

    Ok(SpreadsheetPreview {
        headers,
        rows,
        notes: Vec::new(),
    })
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

/// Store the canonical spelling of a type we recognise, so `date` and `DATETIME` both land as
/// `Date` and validators get one string to match. Anything unrecognised is kept exactly as
/// written — flattening someone's `Coordinates` to `Text` would throw away the only record of
/// what they meant, and an unknown type already reads as `Text` where it matters.
fn canonical_type(declared: &str) -> String {
    let declared = declared.trim();
    let known = ColumnType::from_declared(declared);
    if known == ColumnType::Text && !declared.eq_ignore_ascii_case(ColumnType::Text.as_str()) {
        return declared.to_string();
    }
    known.as_str().to_string()
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
            data_type: canonical_type(record.get(type_ix).unwrap_or_default()),
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

    fn sheet(origin: (u32, u32), notes: &[(&str, &str)]) -> cloud_sync::SheetData {
        cloud_sync::SheetData {
            headers: vec!["Digital ID".into(), "Title".into()],
            rows: vec![
                vec!["1".into(), "First".into()],
                vec!["2".into(), "Second".into()],
            ],
            notes: notes
                .iter()
                .map(|(cell, text)| cloud_sync::CellNote {
                    cell: (*cell).into(),
                    text: (*text).into(),
                })
                .collect(),
            origin,
        }
    }

    #[test]
    fn sheet_notes_resolve_against_the_used_range_origin() {
        // Sheet starting at A1: row 1 is the header, so B2 is the first data row's `Title`.
        let p = SpreadsheetPreview::from(sheet((0, 0), &[("B2", "check this")]));
        assert_eq!(
            p.notes,
            vec![PreviewNote {
                row: 0,
                column: "Title".into(),
                text: "check this".into(),
            }]
        );

        // Same grid parked at C5: calamine's rows start there, but note refs stay absolute, so
        // the origin has to come off both axes or every note lands in the wrong cell.
        let p = SpreadsheetPreview::from(sheet((4, 2), &[("D6", "check this")]));
        assert_eq!(
            p.notes,
            vec![PreviewNote {
                row: 0,
                column: "Title".into(),
                text: "check this".into(),
            }]
        );
    }

    #[test]
    fn notes_outside_the_imported_grid_are_dropped() {
        // The header row itself, a row past the last one, a column past the last one, and junk.
        let p = SpreadsheetPreview::from(sheet(
            (0, 0),
            &[
                ("A1", "header"),
                ("A9", "too far down"),
                ("Z2", "too far right"),
                ("nope", "junk"),
            ],
        ));
        assert_eq!(p.notes, Vec::new());
    }

    #[test]
    fn loads_sample_csv() {
        let csv = sample_dir().join("aderman_collection.csv");
        let preview = load_csv_preview(csv.to_str().unwrap()).unwrap();
        assert_eq!(preview.headers[0], "Digital ID");
        assert_eq!(preview.rows.len(), 4);
        assert_eq!(preview.headers.len(), 6);
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

    /// A synonym is stored canonically so validators match one spelling; a type this build does
    /// not know is kept exactly as written rather than flattened to `Text`.
    #[test]
    fn known_types_are_canonicalised_and_unknown_ones_survive() {
        let dir = tempdir("qrate-config-canonical");
        let path = dir.join("column_config.csv");
        write!(
            std::fs::File::create(&path).unwrap(),
            "Column Name,Data Type\nA,  datetime \nB,int\nC,Coordinates\nD,\n"
        )
        .unwrap();
        let entries = load_column_config(path.to_str().unwrap(), &["A".to_string()])
            .unwrap()
            .entries;
        assert_eq!(entries[0].data_type, "Date");
        assert_eq!(entries[1].data_type, "Number");
        assert_eq!(entries[2].data_type, "Coordinates");
        assert_eq!(entries[3].data_type, "", "a blank type stays unconfigured");
    }

    /// The committed example is what someone copies to start from, so it has to load against the
    /// sample collection it was written for — and every type in it has to be one we recognise.
    #[test]
    fn the_sample_column_config_loads_against_the_sample_collection() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample");
        let headers = load_csv_preview(root.join("aderman_collection.csv").to_str().unwrap())
            .unwrap()
            .headers;
        let entries =
            load_column_config(root.join("column_config.csv").to_str().unwrap(), &headers)
                .unwrap()
                .entries;

        assert_eq!(entries.len(), headers.len(), "every column is described");
        for entry in &entries {
            assert!(
                headers.iter().any(|h| h.eq_ignore_ascii_case(&entry.name)),
                "{} is not a column of the sample collection",
                entry.name
            );
            assert_eq!(
                entry.data_type,
                ColumnType::from_declared(&entry.data_type).as_str(),
                "{} declares a type qrate would not recognise",
                entry.name
            );
            assert!(!entry.description.is_empty(), "{} says nothing", entry.name);
        }
    }

    fn tempdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

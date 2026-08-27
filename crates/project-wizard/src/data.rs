//! Real CSV parsing and folder-matching logic used by the Files, Link, and
//! Columns steps. Both a local CSV and a Google Sheet fetched by `data-exchange` become a
//! [`SpreadsheetPreview`], so they run through the same [`match_folder`] path from here on.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use data_exchange::{SpreadsheetError, SpreadsheetPreview};
use settings::columns::ColumnType;

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

/// Why a files folder was turned down. Each variant carries what the archivist needs to act on
/// it — every one of these used to render as the same "this folder doesn't look right", which
/// says nothing about whether the path is wrong, unreadable, or simply named differently from
/// the spreadsheet.
#[derive(Clone, Debug)]
pub enum FolderError {
    NotFound,
    Permission,
    /// Nothing at all under the folder. `recursive` says whether subfolders were included.
    Empty {
        recursive: bool,
    },
    /// Files are there, but no row names one. Carries both sides of the comparison.
    NoMatches {
        files: usize,
        rows: usize,
        sample_files: Vec<String>,
        sample_rows: Vec<String>,
        recursive: bool,
    },
}

/// Appended to whichever message a non-recursive check produced — the files are often one level
/// down, and the fix is a checkbox away.
const SUBFOLDER_HINT: &str = " Tick \"Also search subfolders\" if your files sit inside folders.";

impl FolderError {
    pub fn message(&self) -> String {
        match self {
            FolderError::NotFound => {
                "There's no folder at that path — check it and try again".into()
            }
            FolderError::Permission => {
                "qrate doesn't have permission to open this folder — check its settings and try again".into()
            }
            FolderError::Empty { recursive } => {
                let mut m = String::from("There are no files in this folder.");
                if !recursive {
                    m.push_str(SUBFOLDER_HINT);
                }
                m
            }
            FolderError::NoMatches {
                files,
                rows,
                sample_files,
                sample_rows,
                recursive,
            } => {
                let mut m = format!(
                    "Found {files} file{} here, but none of your {rows} row{} name one. Rows say: {}. Files are named: {}.",
                    if *files == 1 { "" } else { "s" },
                    if *rows == 1 { "" } else { "s" },
                    sample_rows.join(", "),
                    sample_files.join(", "),
                );
                if !recursive {
                    m.push_str(SUBFOLDER_HINT);
                }
                m
            }
        }
    }
}

/// Every filename under `folder`. `recursive` walks subfolders too — collections nest
/// inconsistently, and `table::photos` already resolves rows against the whole tree, so the
/// wizard's check has to be able to look as deep as the app will.
pub fn list_files(folder: &str, recursive: bool) -> Result<Vec<String>, FolderError> {
    let dir = Path::new(folder);
    if !dir.is_dir() {
        log::warn!("Files folder rejected: {folder} is not a folder qrate can see");
        return Err(FolderError::NotFound);
    }
    // Reading the root up front keeps an unreadable folder an error; an unreadable subfolder
    // below is only skipped.
    fs::read_dir(dir).map_err(|e| {
        log::warn!("Files folder rejected: cannot read {folder} — {e}");
        FolderError::Permission
    })?;

    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with("._") || name.eq_ignore_ascii_case(".ds_store") {
                continue;
            }
            if path.is_dir() {
                if recursive {
                    stack.push(path);
                }
            } else {
                files.push(name.to_string());
            }
        }
    }
    if files.is_empty() {
        log::warn!("Files folder rejected: {folder} holds no files (recursive: {recursive})");
        return Err(FolderError::Empty { recursive });
    }
    log::debug!(
        "Files folder {folder}: {} file(s) found (recursive: {recursive})",
        files.len()
    );
    Ok(files)
}

/// Every name a file answers to: its filename, its stem, and each separator-truncated prefix of
/// that stem. The prefixes are what let a row match a *partial* item — one whose media arrives as
/// `2020_04_001_001.jpg`, `2020_04_001_002.jpg` under the id `2020_04_001`, with any number of
/// its parts missing. Without them a whole shoot of multi-part items reads as "no matches".
fn file_keys(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let stem = Path::new(&lower)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&lower)
        .to_string();
    let mut keys = vec![lower.clone(), stem.clone()];
    keys.extend(
        stem.match_indices(['_', '-', '.', ' '])
            .map(|(at, _)| stem[..at].to_string())
            .filter(|p| !p.is_empty()),
    );
    keys
}

/// Matches spreadsheet rows against files in `folder` by looking for any cell whose value names a
/// file there — by filename, filename stem, or the id a multi-part item's files are built from
/// (see [`file_keys`]), case insensitively. Summarized here for the Files step's inline
/// validation; the Link step's "match by exact filename" shows the same numbers.
pub fn match_folder(
    preview: &SpreadsheetPreview,
    folder: &str,
    recursive: bool,
) -> Result<FolderMatch, FolderError> {
    let files = list_files(folder, recursive)?;
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (ix, name) in files.iter().enumerate() {
        for key in file_keys(name) {
            by_key.entry(key).or_default().push(ix);
        }
    }

    let mut matched_rows = 0usize;
    let mut unmatched: Vec<String> = Vec::new();
    let mut used_files: HashSet<usize> = HashSet::new();
    for row in &preview.rows {
        let hit = row.iter().find_map(|cell| {
            let c = cell.trim().to_lowercase();
            if c.is_empty() {
                return None;
            }
            by_key.get(&c)
        });
        match hit {
            Some(hit) => {
                matched_rows += 1;
                used_files.extend(hit);
            }
            // The row's first non-empty cell is what a person recognizes the row by, and the
            // value most likely meant to name a file.
            None => unmatched.push(
                row.iter()
                    .find(|c| !c.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| "<empty row>".into()),
            ),
        }
    }

    if matched_rows == 0 {
        let sample_rows: Vec<String> = unmatched.iter().take(3).cloned().collect();
        let sample_files: Vec<String> = files.iter().take(3).cloned().collect();
        log::warn!(
            "Files folder rejected: none of {} row(s) name any of the {} file(s) in {folder} \
             (recursive: {recursive}). Rows tried, first 5: {:?}. Filenames found, first 5: {:?}",
            preview.rows.len(),
            files.len(),
            &unmatched[..unmatched.len().min(5)],
            &files[..files.len().min(5)],
        );
        return Err(FolderError::NoMatches {
            files: files.len(),
            rows: preview.rows.len(),
            sample_files,
            sample_rows,
            recursive,
        });
    }
    if !unmatched.is_empty() {
        log::info!(
            "Files folder {folder}: {matched_rows} of {} row(s) matched a file. Unmatched, \
             first 5: {:?}",
            preview.rows.len(),
            &unmatched[..unmatched.len().min(5)],
        );
    }

    Ok(FolderMatch {
        matched_rows,
        total_rows: preview.rows.len(),
        extra_files: files.len().saturating_sub(used_files.len()),
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnConfigEntry {
    pub name: String,
    pub data_type: String,
    pub description: String,
    /// Which authority list this column is checked against, if the file says.
    pub authority: Option<String>,
    /// Whether to spell-check it. `None` leaves the default (on) alone.
    pub spellcheck: Option<bool>,
    /// How loud the authority above reports, as a `Severity` key spelling. Per producer, so each
    /// plugin's own severity travels in [`Self::extra`] beside its mapping instead.
    pub authority_severity: Option<String>,
    /// Every other header, kept verbatim. A plugin's columns arrive here under `<id>::<label>` and
    /// `<id>::Severity` — resolving those back to a plugin needs the loaded plugins, which this
    /// module has no business knowing about.
    pub extra: BTreeMap<String, String>,
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

/// The headers this module understands. Everything else in the file is a plugin's, and lands in
/// [`ColumnConfigEntry::extra`] under its own spelling.
const KNOWN: [&str; 6] = [
    "Column Name",
    "Data Type",
    "Description",
    "Authority",
    "Spellcheck",
    "Authority Severity",
];

/// Reads a yes/no cell. Anything else — including empty — is "the file didn't say", which leaves
/// the setting at its default rather than guessing.
fn yes_no(cell: &str) -> Option<bool> {
    match cell.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "on" | "1" => Some(true),
        "no" | "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// Loads a `column_config.csv`-shaped file (Column Name, Data Type, Description, and optionally
/// Authority, Spellcheck, Severity plus a column per plugin mapping) and checks it against the
/// spreadsheet's own headers. Only Column Name and Data Type are required, so a hand-written
/// three-column file still loads.
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

    let at = |wanted: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(wanted));
    let (name_ix, type_ix) = (at("Column Name"), at("Data Type"));
    let (desc_ix, authority_ix) = (at("Description"), at("Authority"));
    let (spellcheck_ix, severity_ix) = (at("Spellcheck"), at("Authority Severity"));
    // Whatever is left belongs to a plugin, carried through by the name it was written under.
    let extra_ixs: Vec<(usize, String)> = headers
        .iter()
        .enumerate()
        .filter(|(_, h)| !KNOWN.iter().any(|known| h.eq_ignore_ascii_case(known)))
        .map(|(ix, h)| (ix, h.to_string()))
        .collect();

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
        let cell = |ix: Option<usize>| {
            ix.and_then(|i| record.get(i))
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        entries.push(ColumnConfigEntry {
            name: name.clone(),
            data_type: canonical_type(record.get(type_ix).unwrap_or_default()),
            description: cell(desc_ix),
            authority: Some(cell(authority_ix)).filter(|s| !s.is_empty()),
            spellcheck: yes_no(&cell(spellcheck_ix)),
            authority_severity: Some(cell(severity_ix).to_ascii_lowercase())
                .filter(|s| !s.is_empty()),
            extra: extra_ixs
                .iter()
                .map(|(ix, header)| (header.clone(), cell(Some(*ix))))
                .filter(|(_, value)| !value.is_empty())
                .collect(),
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
        let m = match_folder(&preview, photos.to_str().unwrap(), false).unwrap();
        assert_eq!(m.matched_rows, 4);
        assert_eq!(m.total_rows, 4);
        assert_eq!(m.extra_files, 0);
    }

    #[test]
    fn subfolders_only_count_when_recursive() {
        let dir = tempdir("qrate-nested-folder");
        fs::create_dir_all(dir.join("batch")).unwrap();
        fs::write(dir.join("batch").join("nested.jpg"), "x").unwrap();
        fs::write(dir.join("top.jpg"), "x").unwrap();

        assert_eq!(
            list_files(dir.to_str().unwrap(), false).unwrap(),
            ["top.jpg"]
        );
        let mut deep = list_files(dir.to_str().unwrap(), true).unwrap();
        deep.sort();
        assert_eq!(deep, ["nested.jpg", "top.jpg"]);
    }

    #[test]
    fn folder_errors() {
        let csv = sample_dir().join("aderman_collection.csv");
        let preview = load_csv_preview(csv.to_str().unwrap()).unwrap();
        assert!(matches!(
            match_folder(&preview, "/nonexistent/folder", true),
            Err(FolderError::NotFound)
        ));
        let empty = tempdir("qrate-empty-folder");
        assert!(matches!(
            match_folder(&preview, empty.to_str().unwrap(), true),
            Err(FolderError::Empty { .. })
        ));
    }

    #[test]
    fn a_partly_present_multi_part_item_still_matches() {
        // access/2020_04 shape: one row per item, its media split into numbered parts. Only the
        // second part is on disk here — the item is partial, not absent, so the folder is fine.
        let dir = tempdir("qrate-partial-item");
        fs::write(dir.join("2020_04_001_002.jpg"), "x").unwrap();

        let preview = SpreadsheetPreview {
            headers: vec!["Digital ID".into()],
            rows: vec![vec!["2020_04_001".into()], vec!["2020_04_002".into()]],
            notes: Vec::new(),
        };
        let m = match_folder(&preview, dir.to_str().unwrap(), true).unwrap();
        assert_eq!(m.matched_rows, 1);
        assert_eq!(m.total_rows, 2);
        assert_eq!(m.extra_files, 0);
    }

    #[test]
    fn rejection_says_what_it_saw() {
        let dir = tempdir("qrate-no-match-message");
        fs::write(dir.join("unrelated.jpg"), "x").unwrap();

        let preview = SpreadsheetPreview {
            headers: vec!["Digital ID".into()],
            rows: vec![vec!["2020_04_001".into()]],
            notes: Vec::new(),
        };
        let message = match_folder(&preview, dir.to_str().unwrap(), false)
            .unwrap_err()
            .message();
        assert!(message.contains("2020_04_001"), "{message}");
        assert!(message.contains("unrelated.jpg"), "{message}");
        assert!(message.contains("subfolders"), "{message}");
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

    /// The columns an export adds, and the ones it cannot know about — a plugin's columns arrive
    /// under its own id, so anything unrecognised has to survive verbatim rather than being dropped
    /// as noise.
    #[test]
    fn the_optional_columns_are_read_and_the_rest_is_kept() {
        let dir = tempdir("qrate-config-optional");
        let path = dir.join("column_config.csv");
        write!(
            std::fs::File::create(&path).unwrap(),
            "Column Name,Data Type,Description,Authority,Spellcheck,Authority Severity,\
             islandora::Vocabularies,islandora::Severity\n\
             Subject,Text,what it is about,LCSH,no,WARNING,subject; genre,error\n\
             Taken,Date,,,,,,\n"
        )
        .unwrap();

        let entries = load_column_config(path.to_str().unwrap(), &["Subject".to_string()])
            .unwrap()
            .entries;
        assert_eq!(entries[0].authority.as_deref(), Some("LCSH"));
        assert_eq!(entries[0].spellcheck, Some(false));
        // Stored lowercase, because that is the spelling `Severity::from_key` reads.
        assert_eq!(entries[0].authority_severity.as_deref(), Some("warning"));
        assert_eq!(
            entries[0]
                .extra
                .get("islandora::Vocabularies")
                .map(String::as_str),
            Some("subject; genre")
        );
        assert_eq!(
            entries[0]
                .extra
                .get("islandora::Severity")
                .map(String::as_str),
            Some("error")
        );

        // An empty cell is "the file didn't say", which leaves each setting at its default.
        assert_eq!(entries[1].authority, None);
        assert_eq!(entries[1].spellcheck, None);
        assert_eq!(entries[1].authority_severity, None);
        assert!(entries[1].extra.is_empty());
    }

    /// A file written before any of the optional columns existed is still the file people have.
    #[test]
    fn a_three_column_config_still_loads() {
        let dir = tempdir("qrate-config-old");
        let path = dir.join("column_config.csv");
        write!(
            std::fs::File::create(&path).unwrap(),
            "Column Name,Data Type,Description\nSubject,Text,what it is about\n"
        )
        .unwrap();

        let entries = load_column_config(path.to_str().unwrap(), &["Subject".to_string()])
            .unwrap()
            .entries;
        assert_eq!(entries[0].description, "what it is about");
        assert_eq!(entries[0].authority, None);
        assert!(entries[0].extra.is_empty());
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

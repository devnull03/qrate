//! Minimal Google Sheets fetching for the project wizard.
//!
//! Scope is deliberately tiny: pull a *public* ("anyone with the link") Google
//! Sheet via the built-in export endpoint. [`fetch_sheet`] grabs the `.xlsx`
//! export once and pulls out both the row values and the cell notes.
//! No OAuth, no Sheets API v4 — a private sheet just yields a clear error.
//! ponytail: export-endpoint only. Add OAuth/API v4 when private sheets matter.

use std::fs;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SheetSyncError {
    #[error("That doesn't look like a Google Sheets link")]
    BadLink,
    #[error(
        "This sheet isn't public — share it as 'Anyone with the link' (Viewer) and try again, or use a public sheet"
    )]
    Private,
    #[error("We couldn't reach Google Sheets — {0}")]
    Http(#[from] reqwest::Error),
    #[error("We couldn't save the downloaded sheet — {0}")]
    Io(#[from] std::io::Error),
    #[error("We couldn't open the downloaded sheet — {0}")]
    Zip(String),
    #[error("We couldn't read the sheet's notes — {0}")]
    Parse(String),
}

/// The `{id}` and optional `{gid}` pulled out of a Sheets URL.
struct SheetRef {
    id: String,
    gid: Option<String>,
}

/// Parse the spreadsheet id (`/spreadsheets/d/{id}`) and optional tab gid
/// (`gid=NNN`, in either the query or the `#fragment`) out of a Sheets link.
fn parse_sheet_ref(link: &str) -> Result<SheetRef, SheetSyncError> {
    let marker = "/spreadsheets/d/";
    let start = link.find(marker).ok_or(SheetSyncError::BadLink)? + marker.len();
    let rest = &link[start..];
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if id.is_empty() {
        return Err(SheetSyncError::BadLink);
    }

    let gid = link.find("gid=").map(|i| {
        link[i + 4..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
    });
    let gid = gid.filter(|g| !g.is_empty());

    Ok(SheetRef { id, gid })
}

/// A single Google Sheets cell note (the "Insert > Note" sticky kind), pinned
/// to a cell like `"B2"`. Threaded comments are *not* here — they don't survive
/// any export; only the Drive API (OAuth) can reach those.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellNote {
    pub cell: String,
    pub text: String,
}

/// A fetched sheet: the first tab's rows plus its cell notes, from one `.xlsx`
/// download. `headers` is the first row; `rows` the rest.
#[derive(Clone, Debug, Default)]
pub struct SheetData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub notes: Vec<CellNote>,
    /// Top-left of the sheet's used range. `headers`/`rows` start here, but [`CellNote::cell`]
    /// is an absolute sheet coordinate — subtract this before indexing into them.
    pub origin: (u32, u32),
}

/// `"B2"` → zero-based `(row, col)`. Columns are bijective base-26: `A`=0 … `Z`=25, `AA`=26.
/// `None` for anything that isn't letters-then-digits.
pub fn a1_to_index(cell: &str) -> Option<(u32, u32)> {
    let split = cell.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = cell.split_at(split);
    if letters.is_empty() || !letters.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let col = letters.bytes().try_fold(0u32, |acc, b| {
        acc.checked_mul(26)?
            .checked_add(u32::from(b.to_ascii_uppercase() - b'A') + 1)
    })?;
    Some((digits.parse::<u32>().ok()?.checked_sub(1)?, col - 1))
}

/// Fetch a public Sheet as `.xlsx` (one download) and pull out both the first
/// tab's rows and its cell notes. CSV export drops the notes, so we use xlsx
/// for everything. The `.xlsx` is cached in the temp dir. Blocking — call it
/// off the UI thread.
pub fn fetch_sheet(link: &str) -> Result<SheetData, SheetSyncError> {
    let SheetRef { id, gid } = parse_sheet_ref(link)?;
    // ponytail: gid forwarded as a hint, falls back to first tab; real multi-tab selection needs API v4.
    let gid_param = gid.map(|g| format!("&gid={g}")).unwrap_or_default();
    let url = format!("https://docs.google.com/spreadsheets/d/{id}/export?format=xlsx{gid_param}");
    // Explicit timeout so a stalled connection returns an error instead of
    // hanging the background worker thread forever.
    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .get(&url)
        .send()?
        .error_for_status()?;

    if resp
        .url()
        .host_str()
        .unwrap_or_default()
        .contains("accounts.google.com")
    {
        return Err(SheetSyncError::Private);
    }
    let body = resp.bytes()?;
    // A private sheet serves an HTML sign-in page instead of the zip; every
    // xlsx starts with the "PK" local-file-header magic.
    if !body.starts_with(b"PK") {
        return Err(SheetSyncError::Private);
    }

    let dir = std::env::temp_dir().join("qrate-sheets");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{id}.xlsx")), &body)?;

    let (headers, rows, origin) = parse_xlsx_rows(&body)?;
    let notes = parse_xlsx_notes(&body)?;
    Ok(SheetData {
        headers,
        rows,
        notes,
        origin,
    })
}

/// First tab's values via calamine — row 0 is the header, the rest are data. Also returns the
/// used range's top-left, which is not always A1 and which cell-note coordinates are relative to.
#[allow(clippy::type_complexity)]
fn parse_xlsx_rows(
    bytes: &[u8],
) -> Result<(Vec<String>, Vec<Vec<String>>, (u32, u32)), SheetSyncError> {
    use calamine::{Reader, Xlsx};

    let mut wb: Xlsx<std::io::Cursor<&[u8]>> =
        calamine::open_workbook_from_rs(std::io::Cursor::new(bytes))
            .map_err(|e: calamine::XlsxError| SheetSyncError::Zip(e.to_string()))?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or_else(|| SheetSyncError::Parse("the sheet has no tabs".into()))?
        .map_err(|e| SheetSyncError::Parse(e.to_string()))?;

    let origin = range.start().unwrap_or((0, 0));
    let mut iter = range.rows();
    let headers = iter
        .next()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .unwrap_or_default();
    let rows = iter
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .collect();
    Ok((headers, rows, origin))
}

/// Cell notes from every `xl/comments*.xml` entry in the xlsx zip.
fn parse_xlsx_notes(bytes: &[u8]) -> Result<Vec<CellNote>, SheetSyncError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| SheetSyncError::Zip(e.to_string()))?;
    let mut notes = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| SheetSyncError::Zip(e.to_string()))?;
        let name = file.name().to_string();
        if name.starts_with("xl/comments") && name.ends_with(".xml") {
            let mut xml = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut xml)?;
            parse_comments_xml(&xml, &mut notes)?;
        }
    }
    Ok(notes)
}

/// Pull `(ref, text)` pairs out of an xlsx `comments*.xml`. Each `<comment>`
/// carries a cell `ref` and one `<text>` made of one or more `<r><t>…</t></r>`
/// runs, which we concatenate.
fn parse_comments_xml(xml: &[u8], out: &mut Vec<CellNote>) -> Result<(), SheetSyncError> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut cur_ref: Option<String> = None;
    let mut cur_text = String::new();
    let mut in_t = false;
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| SheetSyncError::Parse(e.to_string()))?;
        match event {
            Event::Start(e) if e.local_name().as_ref() == b"comment" => {
                cur_ref = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.local_name().as_ref() == b"ref")
                    .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()));
                cur_text.clear();
            }
            Event::Start(e) if e.local_name().as_ref() == b"t" => in_t = true,
            Event::Text(e) if in_t => {
                cur_text.push_str(&e.unescape().unwrap_or_default());
            }
            Event::End(e) if e.local_name().as_ref() == b"t" => in_t = false,
            Event::End(e) if e.local_name().as_ref() == b"comment" => {
                if let Some(cell) = cur_ref.take() {
                    let text = cur_text.trim().to_string();
                    if !text.is_empty() {
                        out.push(CellNote { cell, text });
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_id_and_gid() {
        let r = parse_sheet_ref("https://docs.google.com/spreadsheets/d/1AbC-dEf_123/edit#gid=456")
            .unwrap();
        assert_eq!(r.id, "1AbC-dEf_123");
        assert_eq!(r.gid.as_deref(), Some("456"));

        let r = parse_sheet_ref("https://docs.google.com/spreadsheets/d/xyz789/edit?usp=sharing")
            .unwrap();
        assert_eq!(r.id, "xyz789");
        assert_eq!(r.gid, None);

        let r = parse_sheet_ref("https://docs.google.com/spreadsheets/d/onlyid").unwrap();
        assert_eq!(r.id, "onlyid");
    }

    #[test]
    fn a1_references_convert_to_zero_based_indices() {
        assert_eq!(a1_to_index("A1"), Some((0, 0)));
        assert_eq!(a1_to_index("B2"), Some((1, 1)));
        assert_eq!(a1_to_index("Z9"), Some((8, 25)));
        // Bijective base-26: AA follows Z, so it is 26 rather than 27 or 0.
        assert_eq!(a1_to_index("AA1"), Some((0, 26)));
        assert_eq!(a1_to_index("AB10"), Some((9, 27)));
        assert_eq!(a1_to_index("b2"), Some((1, 1)));

        for bad in ["", "1", "B", "B0", "B-2", "2B", "$B$2"] {
            assert_eq!(a1_to_index(bad), None, "{bad} should not parse");
        }
    }

    #[test]
    fn parses_notes_with_entities_and_multiple_runs() {
        let xml = br#"<?xml version="1.0"?>
<comments><authors><author>A</author></authors><commentList>
<comment ref="B2" authorId="0"><text><r><t>hello &amp; note</t></r></text></comment>
<comment ref="C5" authorId="0"><text><r><rPr/><t>line one</t></r><r><t> two</t></r></text></comment>
<comment ref="D9" authorId="0"><text><r><t>   </t></r></text></comment>
</commentList></comments>"#;
        let mut out = Vec::new();
        parse_comments_xml(xml, &mut out).unwrap();
        // D9 is whitespace-only and dropped; B2 keeps its unescaped `&`; C5's
        // two runs are concatenated.
        assert_eq!(
            out,
            vec![
                CellNote {
                    cell: "B2".into(),
                    text: "hello & note".into()
                },
                CellNote {
                    cell: "C5".into(),
                    text: "line one two".into()
                },
            ]
        );
    }

    #[test]
    fn rejects_non_sheet() {
        assert!(matches!(
            parse_sheet_ref("https://example.com/not-a-sheet"),
            Err(SheetSyncError::BadLink)
        ));
        assert!(matches!(
            parse_sheet_ref("https://docs.google.com/spreadsheets/d/"),
            Err(SheetSyncError::BadLink)
        ));
    }
}

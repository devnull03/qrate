//! IIIF Presentation API 3: the project as one Manifest, with a Canvas for every row that links a
//! file a viewer can present.
//!
//! Pure like the other writers. The caller measures the files, because the desktop app and a
//! browser reach them differently, and qrate serves none of them: every address is built on the
//! web address the archivist says the export will be published at.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::ColumnType;
use crate::export::{ArchiveFile, ExportComponent, archive_names};

const CONTEXT: &str = "http://iiif.io/api/presentation/3/context.json";

/// One row's linked file as it will be published.
#[derive(Clone, Debug, PartialEq)]
pub struct IiifMedia {
    /// Its path below `files/`, as [`crate::archive_names`] lays the ZIP archive out.
    pub path: String,
    /// Pixels: a picture's or a video's own, a document's first page.
    pub size: Option<(u32, u32)>,
    /// Seconds, for a recording or a video.
    pub duration: Option<f64>,
}

/// Each row's linked file under the name it is published by. `extent` answers with a file's pixel
/// size and its running time, whichever it has.
pub fn iiif_media(
    linked: &[Option<ArchiveFile>],
    extent: impl Fn(&Path) -> (Option<(u32, u32)>, Option<f64>),
) -> Vec<Option<IiifMedia>> {
    let files: Vec<ArchiveFile> = linked.iter().flatten().cloned().collect();
    let mut names = archive_names(&files).into_iter();
    linked
        .iter()
        .map(|file| {
            let file = file.as_ref()?;
            let path = names.next().flatten()?;
            let (size, duration) = extent(&file.path);
            Some(IiifMedia {
                path,
                size,
                duration,
            })
        })
        .collect()
}

/// Everything a manifest is built from. `columns` pairs each declared column with its type, and
/// `media` holds one entry per row.
pub struct IiifInput<'a> {
    pub base_url: &'a str,
    pub label: &'a str,
    pub headers: &'a [String],
    pub row_ids: &'a [i64],
    pub rows: &'a [Vec<String>],
    pub structure: &'a [ExportComponent],
    pub columns: &'a [(String, ColumnType)],
    pub media: &'a [Option<IiifMedia>],
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IiifError {
    #[error("The web address must start with http:// or https:// and have no ? or # in it")]
    BaseUrl,
    #[error("No row links a file that a IIIF manifest can present")]
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IiifProblem {
    /// The row links no file and groups no other rows, so it has nothing to show.
    NoFile,
    /// The file is of a type IIIF has no way to present.
    Unsupported,
    /// The file's pixel size or running time could not be read, and a Canvas needs one.
    Unmeasured,
    /// The rights value is not an address IIIF accepts, so it stays in the metadata only.
    Rights,
}

/// Something the export could not carry over, by the row's position in the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IiifIssue {
    pub row: usize,
    pub problem: IiifProblem,
}

/// The address a manifest is built on, without its trailing slash.
pub fn base_url(value: &str) -> Result<&str, IiifError> {
    let base = value.trim().trim_end_matches('/');
    base.strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
        .filter(|host| {
            !host.is_empty() && !host.contains(|c: char| c.is_whitespace() || c == '?' || c == '#')
        })
        .map(|_| base)
        .ok_or(IiifError::BaseUrl)
}

/// The manifest, and what it had to leave out. Addresses are stable: a row keeps its Canvas
/// address for as long as it keeps its row id.
pub fn iiif_manifest(input: &IiifInput) -> Result<(Value, Vec<IiifIssue>), IiifError> {
    let base = base_url(input.base_url)?;

    let column = |kind: ColumnType| {
        input
            .columns
            .iter()
            .find(|(_, declared)| *declared == kind)
            .and_then(|(name, _)| input.headers.iter().position(|header| header == name))
    };
    let (title, identifier, date) = (
        column(ColumnType::Title),
        column(ColumnType::Identifier),
        column(ColumnType::Date),
    );
    let rights = input.headers.iter().position(|header| {
        matches!(
            header.trim().to_ascii_lowercase().as_str(),
            "rights" | "license" | "licence"
        )
    });
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for component in input.structure {
        if let Some(parent) = component.parent_id {
            children.entry(parent).or_default().push(component.row_id);
        }
    }

    let mut issues = Vec::new();
    let mut canvases = Vec::new();
    let mut painted = HashSet::new();
    let mut groups = HashMap::new();
    for (row, (cells, row_id)) in input.rows.iter().zip(input.row_ids).enumerate() {
        let cell = |at: Option<usize>| {
            at.and_then(|i| cells.get(i))
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        };
        let media = input.media.get(row).and_then(Option::as_ref);
        let label = cell(title)
            .or(cell(identifier))
            .or(media.and_then(|media| media.path.rsplit('/').next()))
            .map_or_else(|| format!("Row {}", row + 1), str::to_owned);
        let metadata: Vec<Value> = input
            .headers
            .iter()
            .zip(cells)
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(header, value)| json!({ "label": text(header), "value": text(value) }))
            .collect();
        let grouping = children.contains_key(row_id);
        if grouping {
            groups.insert(*row_id, (label.clone(), metadata.clone()));
        }
        let mut issue = |problem| issues.push(IiifIssue { row, problem });

        let Some(media) = media else {
            if !grouping {
                issue(IiifProblem::NoFile);
            }
            continue;
        };
        let Some((kind, format)) = content_type(&media.path) else {
            issue(IiifProblem::Unsupported);
            continue;
        };
        let size = media
            .size
            .filter(|(width, height)| *width > 0 && *height > 0);
        let duration = media
            .duration
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0);
        let (size, duration) = match kind {
            "Image" | "Text" => (size, None),
            "Sound" => (None, duration),
            _ => (size, duration),
        };
        let measured = match kind {
            "Image" | "Text" => size.is_some(),
            _ => duration.is_some(),
        };
        if !measured {
            issue(IiifProblem::Unmeasured);
            continue;
        }

        let id = format!("{base}/canvas/{row_id}");
        let mut body = Map::new();
        body.insert(
            "id".into(),
            format!("{base}/files/{}", encode_path(&media.path)).into(),
        );
        body.insert("type".into(), kind.into());
        body.insert("format".into(), format.into());
        let mut canvas = Map::new();
        canvas.insert("id".into(), id.clone().into());
        canvas.insert("type".into(), "Canvas".into());
        canvas.insert("label".into(), text(&label));
        for resource in [&mut canvas, &mut body] {
            if let Some((width, height)) = size {
                resource.insert("width".into(), width.into());
                resource.insert("height".into(), height.into());
            }
            if let Some(seconds) = duration {
                resource.insert("duration".into(), seconds.into());
            }
        }
        if let Some(instant) = cell(date).and_then(nav_date) {
            canvas.insert("navDate".into(), instant.into());
        }
        match cell(rights).map(rights_uri) {
            Some(Some(uri)) => {
                canvas.insert("rights".into(), uri.into());
            }
            Some(None) => issue(IiifProblem::Rights),
            None => {}
        }
        if !metadata.is_empty() {
            canvas.insert("metadata".into(), metadata.into());
        }
        canvas.insert(
            "items".into(),
            json!([{
                "id": format!("{id}/page"),
                "type": "AnnotationPage",
                "items": [{
                    "id": format!("{id}/painting"),
                    "type": "Annotation",
                    "motivation": "painting",
                    "body": body,
                    "target": id,
                }],
            }]),
        );
        canvases.push(Value::Object(canvas));
        painted.insert(*row_id);
    }
    if canvases.is_empty() {
        return Err(IiifError::Empty);
    }

    let ranges = Ranges {
        base,
        children: &children,
        painted: &painted,
        groups: &groups,
    };
    let mut seen = HashSet::new();
    let structures: Vec<Value> = input
        .structure
        .iter()
        .filter(|component| component.parent_id.is_none())
        .filter_map(|component| ranges.build(component.row_id, &mut seen))
        .collect();

    let mut manifest = Map::new();
    manifest.insert("@context".into(), CONTEXT.into());
    manifest.insert("id".into(), format!("{base}/manifest.json").into());
    manifest.insert("type".into(), "Manifest".into());
    manifest.insert(
        "label".into(),
        text(match input.label.trim() {
            "" => "Untitled",
            label => label,
        }),
    );
    manifest.insert("items".into(), canvases.into());
    if !structures.is_empty() {
        manifest.insert("structures".into(), structures.into());
    }
    Ok((Value::Object(manifest), issues))
}

/// The archival arrangement as nested Ranges: the viewer's table of contents.
struct Ranges<'a> {
    base: &'a str,
    children: &'a HashMap<i64, Vec<i64>>,
    painted: &'a HashSet<i64>,
    groups: &'a HashMap<i64, (String, Vec<Value>)>,
}

impl Ranges<'_> {
    /// `None` for a row that groups nothing a viewer can show, since a Range may not be empty.
    /// `seen` stops a project whose structure loops back on itself from recursing forever.
    fn build(&self, row_id: i64, seen: &mut HashSet<i64>) -> Option<Value> {
        let below = self.children.get(&row_id)?;
        if !seen.insert(row_id) {
            return None;
        }
        let canvas = |row_id: i64| {
            self.painted.contains(&row_id).then(
                || json!({ "id": format!("{}/canvas/{row_id}", self.base), "type": "Canvas" }),
            )
        };
        let mut items: Vec<Value> = canvas(row_id).into_iter().collect();
        for child in below {
            items.extend(match self.children.contains_key(child) {
                true => self.build(*child, seen),
                false => canvas(*child),
            });
        }
        if items.is_empty() {
            return None;
        }
        let mut range = Map::new();
        range.insert("id".into(), format!("{}/range/{row_id}", self.base).into());
        range.insert("type".into(), "Range".into());
        match self.groups.get(&row_id) {
            Some((label, metadata)) => {
                range.insert("label".into(), text(label));
                if !metadata.is_empty() {
                    range.insert("metadata".into(), metadata.clone().into());
                }
            }
            None => {
                range.insert("label".into(), text(&row_id.to_string()));
            }
        }
        range.insert("items".into(), items.into());
        Some(Value::Object(range))
    }
}

/// What an export could not carry over, in words, or nothing when it carried everything.
pub fn iiif_summary(issues: &[IiifIssue]) -> String {
    let count = |problem| {
        issues
            .iter()
            .filter(|issue| issue.problem == problem)
            .count()
    };
    let rows = |n: usize| format!("{n} row{}", if n == 1 { "" } else { "s" });
    let mut parts = Vec::new();
    for (problem, why) in [
        (IiifProblem::NoFile, "with no linked file"),
        (
            IiifProblem::Unsupported,
            "with a file type IIIF cannot present",
        ),
        (
            IiifProblem::Unmeasured,
            "whose file's size or length could not be read",
        ),
    ] {
        match count(problem) {
            0 => {}
            n => parts.push(format!("left out {} {why}", rows(n))),
        }
    }
    match count(IiifProblem::Rights) {
        0 => {}
        n => parts.push(format!(
            "kept {} rights value{} as metadata only, because IIIF accepts just Creative Commons \
             and RightsStatements.org addresses there",
            n,
            if n == 1 { "" } else { "s" }
        )),
    }
    parts.join("; ")
}

/// A value in no particular language, which is all a spreadsheet cell says about itself.
fn text(value: &str) -> Value {
    json!({ "none": [value] })
}

/// The IIIF content type and the media type of a file, by its extension. A PDF is `Text`, which
/// is how viewers that show documents expect to be handed one.
fn content_type(path: &str) -> Option<(&'static str, &'static str)> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "jpg" | "jpeg" => ("Image", "image/jpeg"),
        "png" => ("Image", "image/png"),
        "gif" => ("Image", "image/gif"),
        "webp" => ("Image", "image/webp"),
        "bmp" => ("Image", "image/bmp"),
        "tif" | "tiff" => ("Image", "image/tiff"),
        "avif" => ("Image", "image/avif"),
        "jp2" => ("Image", "image/jp2"),
        "mp4" | "m4v" => ("Video", "video/mp4"),
        "mov" => ("Video", "video/quicktime"),
        "webm" => ("Video", "video/webm"),
        "mkv" => ("Video", "video/x-matroska"),
        "avi" => ("Video", "video/x-msvideo"),
        "mpg" | "mpeg" => ("Video", "video/mpeg"),
        "mp3" => ("Sound", "audio/mpeg"),
        "wav" => ("Sound", "audio/wav"),
        "flac" => ("Sound", "audio/flac"),
        "ogg" | "oga" => ("Sound", "audio/ogg"),
        "m4a" => ("Sound", "audio/mp4"),
        "aac" => ("Sound", "audio/aac"),
        "aif" | "aiff" => ("Sound", "audio/aiff"),
        "pdf" => ("Text", "application/pdf"),
        _ => return None,
    })
}

/// A path as it appears in a web address, each segment escaped.
fn encode_path(path: &str) -> String {
    path.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// A plain year, month or day as the instant it begins, which is what a viewer's timeline wants.
/// A range or an approximate date names no single instant, so it stays in the metadata only.
fn nav_date(value: &str) -> Option<String> {
    let number = |part: &str, digits: usize| {
        (part.len() == digits && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let mut parts = value.split('-');
    let year = number(parts.next()?, 4).filter(|year| *year > 0)?;
    let month = parts.next().map_or(Some(1), |part| number(part, 2))?;
    let day = parts.next().map_or(Some(1), |part| number(part, 2))?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return None,
    };
    (parts.next().is_none() && (1..=days).contains(&day))
        .then(|| format!("{year:04}-{month:02}-{day:02}T00:00:00Z"))
}

/// A Creative Commons or RightsStatements.org address in the `http://` form IIIF asks for, which
/// is how both publish their identifiers even though their pages are served over `https://`.
fn rights_uri(value: &str) -> Option<String> {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))?;
    [
        "creativecommons.org/licenses/",
        "creativecommons.org/publicdomain/",
        "rightsstatements.org/vocab/",
    ]
    .iter()
    .any(|known| rest.len() > known.len() && rest.starts_with(known))
    .then(|| format!("http://{rest}"))
    .filter(|uri| !uri.contains(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::{
        IiifError, IiifInput, IiifIssue, IiifMedia, IiifProblem, iiif_manifest, iiif_media,
        iiif_summary, nav_date, rights_uri,
    };
    use crate::ColumnType;
    use crate::export::{ArchiveFile, ExportComponent};

    #[test]
    fn each_row_keeps_its_own_file_when_some_rows_link_none() {
        let file = |path: &str| {
            Some(ArchiveFile {
                path: path.into(),
                source_path: Some(path.into()),
            })
        };
        let media = iiif_media(&[None, file("a/1.jpg"), None, file("b/1.jpg")], |path| {
            (Some((path.to_string_lossy().len() as u32, 1)), None)
        });
        assert_eq!(
            media
                .iter()
                .map(|media| media.as_ref().map(|media| media.path.as_str()))
                .collect::<Vec<_>>(),
            [None, Some("a/1.jpg"), None, Some("b/1.jpg")]
        );
        assert_eq!(media[3].as_ref().unwrap().size, Some((7, 1)));
    }

    fn image(path: &str) -> Option<IiifMedia> {
        Some(IiifMedia {
            path: path.into(),
            size: Some((4000, 3000)),
            duration: None,
        })
    }

    struct Fixture {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        structure: Vec<ExportComponent>,
        columns: Vec<(String, ColumnType)>,
        media: Vec<Option<IiifMedia>>,
    }

    impl Fixture {
        fn input<'a>(&'a self, base_url: &'a str) -> IiifInput<'a> {
            IiifInput {
                base_url,
                label: "Harbour photographs",
                headers: &self.headers,
                row_ids: &[10, 11, 12, 13, 14, 15, 16],
                rows: &self.rows,
                structure: &self.structure,
                columns: &self.columns,
                media: &self.media,
            }
        }
    }

    /// A series holding two photographs, a recording and a video, then two loose rows: one that
    /// links nothing and one that links a document.
    fn fixture() -> Fixture {
        let component = |row_id, parent_id: Option<i64>, level: &str| ExportComponent {
            row_id,
            parent_id,
            level_key: level.into(),
            source_path: None,
        };
        let row = |cells: [&str; 5]| cells.map(String::from).to_vec();
        Fixture {
            headers: ["Digital ID", "Title", "Taken", "Rights", "File"]
                .map(String::from)
                .to_vec(),
            rows: vec![
                row(["S1", "Harbour series", "1943/1950", "", ""]),
                row([
                    "1",
                    "First photo",
                    "1943-06",
                    "https://creativecommons.org/licenses/by/4.0/",
                    "a/first photo.jpg",
                ]),
                row(["2", "", "circa 1944", "ask the donor", "a/2.tif"]),
                row(["3", "Interview", "1950", "", "tape.mp3"]),
                row(["4", "Launch day", "", "", "launch.mp4"]),
                row(["5", "Loose note", "", "", ""]),
                row(["6", "Finding aid", "", "", "finding aid.pdf"]),
            ],
            structure: vec![
                component(10, None, "series"),
                component(11, Some(10), "item"),
                component(12, Some(10), "item"),
                component(13, Some(10), "item"),
                component(14, Some(10), "item"),
            ],
            columns: vec![
                ("Digital ID".into(), ColumnType::Identifier),
                ("Title".into(), ColumnType::Title),
                ("Taken".into(), ColumnType::Date),
                ("File".into(), ColumnType::Filename),
            ],
            media: vec![
                None,
                image("a/first photo.jpg"),
                image("a/2.tif"),
                Some(IiifMedia {
                    path: "tape.mp3".into(),
                    size: None,
                    duration: Some(1845.5),
                }),
                Some(IiifMedia {
                    path: "launch.mp4".into(),
                    size: Some((1920, 1080)),
                    duration: Some(62.0),
                }),
                None,
                Some(IiifMedia {
                    path: "finding aid.pdf".into(),
                    size: Some((612, 792)),
                    duration: None,
                }),
            ],
        }
    }

    /// The golden file is the one checked against the IIIF Presentation validator; when this
    /// output changes on purpose, validate the new file before committing it.
    #[test]
    fn a_project_becomes_one_manifest_with_a_canvas_per_linked_file() {
        let fixture = fixture();
        let (manifest, issues) =
            iiif_manifest(&fixture.input("https://example.org/iiif/harbour/")).unwrap();
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/catalog.iiif.json")).unwrap();
        assert_eq!(manifest, golden);

        assert_eq!(
            manifest["id"],
            "https://example.org/iiif/harbour/manifest.json"
        );
        let canvases = manifest["items"].as_array().unwrap();
        assert_eq!(canvases.len(), 5);
        let body = &canvases[0]["items"][0]["items"][0]["body"];
        assert_eq!(
            body["id"],
            "https://example.org/iiif/harbour/files/a/first%20photo.jpg"
        );
        assert_eq!(canvases[0]["navDate"], "1943-06-01T00:00:00Z");
        assert_eq!(
            canvases[0]["rights"],
            "http://creativecommons.org/licenses/by/4.0/"
        );
        // An untitled row is labelled by its identifier, and a date that names no instant and a
        // rights note that is no licence address stay where the archivist wrote them.
        assert_eq!(canvases[1]["label"]["none"][0], "2");
        assert!(canvases[1].get("navDate").is_none());
        assert!(canvases[1].get("rights").is_none());
        assert_eq!(canvases[2]["duration"], 1845.5);
        assert!(canvases[2].get("width").is_none());
        assert_eq!(canvases[3]["width"], 1920);
        assert_eq!(canvases[4]["items"][0]["items"][0]["body"]["type"], "Text");

        let series = &manifest["structures"][0];
        assert_eq!(series["label"]["none"][0], "Harbour series");
        assert_eq!(series["items"].as_array().unwrap().len(), 4);
        assert_eq!(
            issues,
            [
                IiifIssue {
                    row: 2,
                    problem: IiifProblem::Rights
                },
                IiifIssue {
                    row: 5,
                    problem: IiifProblem::NoFile
                },
            ]
        );
    }

    #[test]
    fn a_file_that_cannot_be_presented_is_reported_instead_of_written_malformed() {
        let mut fixture = fixture();
        fixture.media[1] = Some(IiifMedia {
            path: "a/first photo.jpg".into(),
            size: None,
            duration: None,
        });
        fixture.media[2] = image("a/notes.docx");
        fixture.media[4] = Some(IiifMedia {
            path: "launch.mp4".into(),
            size: Some((1920, 1080)),
            duration: None,
        });
        let (manifest, issues) = iiif_manifest(&fixture.input("https://example.org")).unwrap();
        assert_eq!(manifest["items"].as_array().unwrap().len(), 2);
        assert_eq!(
            issues.iter().map(|issue| issue.problem).collect::<Vec<_>>(),
            [
                IiifProblem::Unmeasured,
                IiifProblem::Unsupported,
                IiifProblem::Unmeasured,
                IiifProblem::NoFile,
            ]
        );
        assert_eq!(
            iiif_summary(&issues),
            "left out 1 row with no linked file; left out 1 row with a file type IIIF cannot \
             present; left out 2 rows whose file's size or length could not be read"
        );
        assert_eq!(iiif_summary(&[]), "");
    }

    #[test]
    fn an_address_that_is_not_on_the_web_or_a_project_with_nothing_to_show_is_refused() {
        let mut fixture = fixture();
        for base in [
            "",
            "example.org",
            "ftp://example.org",
            "https://",
            "https://x.org/a#b",
        ] {
            assert_eq!(
                iiif_manifest(&fixture.input(base)).unwrap_err(),
                IiifError::BaseUrl,
                "{base}"
            );
        }
        fixture.media = vec![None; 7];
        assert_eq!(
            iiif_manifest(&fixture.input("https://example.org")).unwrap_err(),
            IiifError::Empty
        );
    }

    #[test]
    fn a_structure_that_loops_back_on_itself_still_ends() {
        let mut fixture = fixture();
        fixture.structure.push(ExportComponent {
            row_id: 10,
            parent_id: Some(11),
            level_key: "series".into(),
            source_path: None,
        });
        let (manifest, _) = iiif_manifest(&fixture.input("https://example.org")).unwrap();
        let series = manifest["structures"][0]["items"].as_array().unwrap();
        assert_eq!(
            series[0]["type"], "Range",
            "the photo that also groups rows"
        );
        assert_eq!(series[0]["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_group_with_nothing_to_show_yields_no_range() {
        let mut fixture = fixture();
        fixture.structure.truncate(1);
        fixture.structure.push(ExportComponent {
            row_id: 15,
            parent_id: Some(10),
            level_key: "item".into(),
            source_path: None,
        });
        let (manifest, _) = iiif_manifest(&fixture.input("https://example.org")).unwrap();
        assert!(manifest.get("structures").is_none());
    }

    #[test]
    fn only_a_whole_real_date_becomes_a_navigation_date() {
        assert_eq!(nav_date("1943").as_deref(), Some("1943-01-01T00:00:00Z"));
        assert_eq!(
            nav_date("2024-02-29").as_deref(),
            Some("2024-02-29T00:00:00Z")
        );
        for not_an_instant in [
            "1943/1950",
            "1943~",
            "194X",
            "2023-02-29",
            "1943-13",
            "43",
            "1943-6",
        ] {
            assert_eq!(nav_date(not_an_instant), None, "{not_an_instant}");
        }
    }

    #[test]
    fn rights_are_kept_only_as_the_addresses_iiif_accepts() {
        assert_eq!(
            rights_uri("https://rightsstatements.org/vocab/InC/1.0/").as_deref(),
            Some("http://rightsstatements.org/vocab/InC/1.0/")
        );
        assert_eq!(
            rights_uri("http://creativecommons.org/publicdomain/zero/1.0/").as_deref(),
            Some("http://creativecommons.org/publicdomain/zero/1.0/")
        );
        for refused in [
            "CC BY 4.0",
            "https://example.org/licence",
            "https://creativecommons.org/licenses/",
        ] {
            assert_eq!(rights_uri(refused), None, "{refused}");
        }
    }
}

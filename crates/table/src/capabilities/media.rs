//! Capabilities that look at what the rows point to — their images, and what looks alike — and at
//! what an export would make of the table.

use ai::agent::{RequestError, Thumbnail};
use base64::Engine as _;
use capability::{Capability, Effect, Exposure, Invocation};
use gpui::{App, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::table;

/// One row's linked image, larger than a thumbnail, for reading labels and handwriting.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ImageRequest {
    pub row: usize,
    /// Page of a multi-page file, from 0.
    #[serde(default)]
    pub page: usize,
    /// Longest side in pixels, 256 to 2048. Defaults to 1568.
    #[serde(default = "default_edge")]
    pub max_edge: u32,
}

fn default_edge() -> u32 {
    1568
}

pub struct ImageCap;

impl Capability for ImageCap {
    const NAME: &'static str = "image";
    const SUMMARY: &'static str = "Read one row's linked file as a PNG of up to 2048 pixels, to read labels, captions or handwriting. Only files the project links; use thumbnails to skim.";
    const GROUP: &'static str = "media";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = ImageRequest;
    type Output = Thumbnail;

    fn validate(input: &ImageRequest) -> Result<(), RequestError> {
        match (256..=2048).contains(&input.max_edge) {
            true => Ok(()),
            false => Err(RequestError::MalformedParams(
                "max_edge must be 256 to 2048".into(),
            )),
        }
    }

    fn label(input: &ImageRequest, _: &App) -> String {
        format!("image of row {} at {} px", input.row + 1, input.max_edge)
    }

    fn outcome(_: &Thumbnail) -> String {
        "one image".into()
    }

    fn run(
        input: ImageRequest,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<Thumbnail, RequestError>> {
        let path = match table(cx).and_then(|state| {
            state
                .read(cx)
                .delegate()
                .row_image(input.row)
                .map(ToOwned::to_owned)
                .ok_or(RequestError::ThumbnailUnavailable)
        }) {
            Ok(path) => path,
            Err(error) => return Task::ready(Err(error)),
        };
        cx.background_executor().spawn(async move {
            let bytes = preview::png(&path, input.page, input.max_edge)
                .ok_or(RequestError::ThumbnailUnavailable)?;
            Ok(Thumbnail {
                row: input.row,
                page: input.page,
                media_type: "image/png".into(),
                data: base64::engine::general_purpose::STANDARD.encode(bytes),
            })
        })
    }
}

/// Rows whose images look like one row's, or like a description.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct SimilarRequest {
    /// Find images like this row's. Give `row` or `text`.
    pub row: Option<usize>,
    /// Find images matching this description, such as "a parade float".
    pub text: Option<String>,
    /// Rows to return, 1 to 50. Defaults to 20.
    #[serde(default = "default_similar")]
    pub limit: usize,
}

fn default_similar() -> usize {
    20
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SimilarRows {
    /// Best first.
    pub items: Vec<SimilarRow>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SimilarRow {
    pub row: usize,
    /// Cosine similarity: 1 is identical.
    pub score: f32,
}

pub struct SimilarCap;

impl Capability for SimilarCap {
    const NAME: &'static str = "similar";
    const SUMMARY: &'static str = "Rank rows by how much their images look like one row's image, or like a short description, using qrate's on-device visual search. Useful for spotting duplicates and series. Needs visual search turned on by the archivist.";
    const GROUP: &'static str = "media";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = SimilarRequest;
    type Output = SimilarRows;

    fn validate(input: &SimilarRequest) -> Result<(), RequestError> {
        if input.row.is_some() == input.text.is_some() {
            return Err(RequestError::MalformedParams(
                "give row or text, not both".into(),
            ));
        }
        if input
            .text
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > 256)
        {
            return Err(RequestError::InvalidSearchText);
        }
        match (1..=50).contains(&input.limit) {
            true => Ok(()),
            false => Err(RequestError::InvalidQueryLimit),
        }
    }

    fn label(input: &SimilarRequest, _: &App) -> String {
        match (&input.row, &input.text) {
            (Some(row), _) => format!("images like row {}", row + 1),
            (None, Some(text)) => format!("images of “{text}”"),
            (None, None) => String::new(),
        }
    }

    fn outcome(output: &SimilarRows) -> String {
        format!("{} row(s)", output.items.len())
    }

    fn run(
        input: SimilarRequest,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<SimilarRows, RequestError>> {
        let prepared = (|| {
            if crate::visual::status(cx) != crate::visual::Status::Ready {
                return Err(RequestError::VisualSearchOff);
            }
            let state = table(cx)?;
            let delegate = state.read(cx).delegate();
            let query = match (input.row, input.text) {
                (Some(row), _) => crate::visual::Query::Like(
                    delegate
                        .row_image(row)
                        .map(ToOwned::to_owned)
                        .ok_or(RequestError::ThumbnailUnavailable)?,
                ),
                (None, text) => crate::visual::Query::Text(text.unwrap_or_default()),
            };
            Ok((crate::visual::scorer(query, cx), delegate.linked_rows()))
        })();
        let (scorer, candidates) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Task::ready(Err(error)),
        };
        let (limit, skip) = (input.limit, input.row);
        cx.background_executor().spawn(async move {
            let rank = scorer.await.ok_or(RequestError::VisualSearchOff)?;
            let items = rank(&candidates)
                .into_iter()
                .filter(|(_, row)| Some(*row) != skip)
                .take(limit)
                .map(|(score, row)| SimilarRow { row, score })
                .collect();
            Ok(SimilarRows { items })
        })
    }
}

/// What an export would make of the table, without writing anything.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ExportCheck {
    pub format: ExportFormat,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Xlsx,
    CslJson,
    Zip,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ExportReport {
    /// Whether the export would be written.
    pub ok: bool,
    /// Size of the file it would write, when it was built in full.
    pub bytes: Option<usize>,
    /// At most 100, the most serious first.
    pub problems: Vec<ExportProblem>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ExportProblem {
    pub row: Option<usize>,
    pub column: Option<String>,
    /// `fails` stops the export; `loses` drops or changes data; `note` is worth knowing.
    pub severity: String,
    pub message: String,
}

/// The longest text Excel keeps in one cell.
const EXCEL_CELL: usize = 32_767;

pub struct ExportCheckCap;

impl Capability for ExportCheckCap {
    const NAME: &'static str = "export_check";
    const SUMMARY: &'static str = "Dry-run an export (CSV, Excel, CSL-JSON for Zotero, or the ZIP with files) in memory and report what would fail or be lost. Writes nothing.";
    const GROUP: &'static str = "review";
    const EXPOSURE: Exposure = Exposure::Agent;
    const EFFECT: Effect = Effect::Read;
    type Input = ExportCheck;
    type Output = ExportReport;

    fn label(input: &ExportCheck, _: &App) -> String {
        format!("{:?} export check", input.format)
    }

    fn outcome(output: &ExportReport) -> String {
        format!("ok: {}, {} problem(s)", output.ok, output.problems.len())
    }

    fn run(
        input: ExportCheck,
        _: &Invocation,
        cx: &mut App,
    ) -> Task<Result<ExportReport, RequestError>> {
        let prepared = (|| {
            let project = cx
                .try_global::<settings::project::CurrentProject>()
                .ok_or(RequestError::ProjectUnavailable)?;
            let state = table(cx)?;
            let delegate = state.read(cx).delegate();
            let (headers, _, rows) = delegate.dataset_snapshot();
            let declared: Vec<(String, String)> = headers
                .iter()
                .map(|header| {
                    let kind = project
                        .data
                        .columns
                        .iter()
                        .find(|column| column.name == *header)
                        .map(|column| column.data_type.clone())
                        .unwrap_or_default();
                    (header.clone(), kind)
                })
                .collect();
            let missing: Vec<(usize, String, String)> = match input.format {
                ExportFormat::Zip => crate::file_links::missing_files(delegate, cx)
                    .into_iter()
                    .map(|(row, col, value)| {
                        (
                            row,
                            delegate.column_name(col).to_string(),
                            value.to_string(),
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Ok((headers, rows, declared, missing))
        })();
        let (headers, rows, declared, missing) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Task::ready(Err(error)),
        };
        let format = input.format;
        cx.background_executor().spawn(async move {
            let mut problems = Vec::new();
            let problem = |row: Option<usize>, column: Option<&str>, severity: &str, message: String| ExportProblem {
                row,
                column: column.map(str::to_owned),
                severity: severity.into(),
                message,
            };
            let built = match format {
                ExportFormat::Csv => qrate_export::export::csv_bytes(&headers, &rows, Default::default())
                    .map(|bytes| bytes.len())
                    .map_err(|error| error.to_string()),
                ExportFormat::Xlsx | ExportFormat::Zip => {
                    for (row, cells) in rows.iter().enumerate() {
                        for (col, cell) in cells.iter().enumerate() {
                            if cell.chars().count() > EXCEL_CELL {
                                problems.push(problem(
                                    Some(row),
                                    headers.get(col).map(String::as_str),
                                    "fails",
                                    format!("{} characters: Excel keeps at most {EXCEL_CELL} in a cell", cell.chars().count()),
                                ));
                            }
                        }
                    }
                    for (row, column, value) in &missing {
                        problems.push(problem(
                            Some(*row),
                            Some(column),
                            "loses",
                            format!("the linked file “{value}” cannot be found, so the archive will not include it"),
                        ));
                    }
                    qrate_export::export::xlsx_bytes(&headers, &rows, &[])
                        .map(|bytes| bytes.len())
                        .map_err(|error| error.to_string())
                }
                ExportFormat::CslJson => {
                    let mapping = qrate_export::export::derive_csl_mapping(&declared);
                    if !mapping.contains_key("title") {
                        problems.push(problem(None, None, "loses", "no column maps to the CSL title, so Zotero items will have none".into()));
                    }
                    if let Some(title) = mapping.get("title").and_then(|name| headers.iter().position(|h| h == name)) {
                        for (row, cells) in rows.iter().enumerate() {
                            if cells.get(title).is_none_or(|cell| cell.trim().is_empty()) {
                                problems.push(problem(Some(row), Some(&headers[title]), "loses", "no title: Zotero lists the item untitled".into()));
                            }
                        }
                    }
                    for field in qrate_export::export::CSL_FIELDS {
                        if !mapping.contains_key(field) {
                            problems.push(problem(None, None, "note", format!("no column maps to CSL `{field}`; unmapped columns go into each item's note")));
                        }
                    }
                    serde_json::to_vec(&qrate_export::export::csl_items(&headers, &rows, &mapping))
                        .map(|bytes| bytes.len())
                        .map_err(|error| error.to_string())
                }
            };
            let order = |severity: &str| match severity {
                "fails" => 0,
                "loses" => 1,
                _ => 2,
            };
            problems.sort_by_key(|problem| order(&problem.severity));
            if let Err(error) = &built {
                problems.insert(0, problem(None, None, "fails", error.clone()));
            }
            let truncated = problems.len() > 100;
            problems.truncate(100);
            Ok(ExportReport {
                ok: built.is_ok() && problems.iter().all(|problem| problem.severity != "fails"),
                bytes: built.ok(),
                problems,
                truncated,
            })
        })
    }
}

pub(super) fn register(cx: &mut App) {
    capability::register::<ImageCap>(cx);
    capability::register::<SimilarCap>(cx);
    capability::register::<ExportCheckCap>(cx);
}

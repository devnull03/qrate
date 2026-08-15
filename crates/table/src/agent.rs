//! Live-table adapter for the external-agent contract: the reads, and the one draft-only write.

use std::collections::HashMap;

use ai::agent::{
    Column, Diagnostic, Field, Finding, ProjectSummary, Request, RequestError, Response, ResultSet,
    Row, Severity, TableRevision,
};
use diagnostics::{DATASET_MAIN, FixProviders, Location, Source};
use gpui::{App, Global, SharedString};

use crate::{QrateTableDelegate, TableStateHandle};

/// The diagnostics source every staged finding is filed under. One name for the whole agent, so a
/// re-run replaces its own drafts wholesale instead of stacking a second copy beside them.
pub const AGENT_SOURCE: &str = "agent";

/// Answer one validated agent request from qrate's live table state.
///
/// This deliberately reads the delegate rather than the `.qrate` database, so responses include
/// unsaved cell edits and the current table generation. Nothing here writes a cell: the one
/// non-read, [`Request::StageFindings`], publishes advisory diagnostics that are recomputed rather
/// than persisted, and offers corrections the archivist still has to click.
pub fn respond_to_agent(request: Request, cx: &mut App) -> Result<Response, RequestError> {
    request.validate()?;

    if let Request::StageFindings { revision, findings } = request {
        return stage_findings(revision, findings, cx);
    }

    let project = cx
        .try_global::<settings::project::CurrentProject>()
        .ok_or(RequestError::ProjectUnavailable)?;
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let table = state.read(cx);
    let delegate = table.delegate();
    let revision = TableRevision(delegate.values_generation());

    let result = match request {
        Request::ProjectSummary => ResultSet::ProjectSummary(ProjectSummary {
            name: project.display_name(),
            row_count: delegate.row_count(),
            column_count: delegate.column_count(),
            has_files_folder: project
                .data
                .values
                .get(settings::project::FILES_FOLDER_KEY)
                .is_some_and(|value| !value.text().trim().is_empty()),
        }),
        Request::Columns => ResultSet::Columns {
            columns: project
                .data
                .headers
                .iter()
                .map(|name| {
                    let configured = project
                        .data
                        .columns
                        .iter()
                        .find(|column| column.name == *name);
                    Column {
                        name: name.clone(),
                        data_type: configured
                            .map_or_else(String::new, |column| column.data_type.clone()),
                        notes: configured.map_or_else(String::new, |column| column.notes.clone()),
                    }
                })
                .collect(),
        },
        Request::Rows { rows } => ResultSet::Rows {
            rows: rows
                .into_iter()
                .filter(|&row| row < delegate.row_count())
                .map(|row| row_from(delegate, row))
                .collect(),
        },
        Request::SearchRows { query, limit } => {
            let query = query.to_lowercase();
            ResultSet::Rows {
                rows: (0..delegate.row_count())
                    .filter(|&row| {
                        delegate.row_fields(row).iter().any(|(column, value)| {
                            column.to_lowercase().contains(&query)
                                || value.to_lowercase().contains(&query)
                        })
                    })
                    .take(limit)
                    .map(|row| row_from(delegate, row))
                    .collect(),
            }
        }
        Request::Diagnostics => ResultSet::Diagnostics {
            diagnostics: diagnostics::Diagnostics::all(cx)
                .iter()
                .map(|diagnostic| Diagnostic {
                    row: diagnostic.location.row,
                    column: diagnostic.location.column.as_ref().map(ToString::to_string),
                    severity: match diagnostic.severity {
                        diagnostics::Severity::Error => Severity::Error,
                        diagnostics::Severity::Warning => Severity::Warning,
                        diagnostics::Severity::Note => Severity::Note,
                    },
                    source: diagnostic.source.label().to_string(),
                    message: diagnostic.message.to_string(),
                })
                .collect(),
        },
        Request::SelectedRows => ResultSet::SelectedRows {
            rows: delegate.selected_source_rows(),
        },
        Request::StageFindings { .. } => unreachable!("staging returned above"),
    };

    Ok(Response { revision, result })
}

/// One agent proposal, held until somebody opens the Fixes menu on its cell.
struct StagedFix {
    /// The cell text the agent judged. The offer is withheld unless the cell still says this.
    expected: SharedString,
    replacement: SharedString,
}

/// Every replacement the agent staged, by `(row, column)`.
///
/// A global rather than a field on anything, because [`FixProviders`] registers a plain `fn`
/// pointer and the offer has no handle to reach back through. Never persisted, and replaced
/// wholesale on the next batch — the same replace-by-source rule the diagnostics follow.
#[derive(Default)]
struct StagedFixes {
    /// The table the whole batch was judged against, retained per the contract. Read by the apply
    /// path, which must refuse a batch whose table has moved on under it.
    #[allow(dead_code)]
    revision: u64,
    by_cell: HashMap<(usize, SharedString), Vec<StagedFix>>,
}
impl Global for StagedFixes {}

/// Publish a batch of drafts: findings into the Problems panel, replacements into the Fixes menu.
/// No cell is touched, and a draft the table has moved past is dropped rather than shown.
fn stage_findings(
    revision: TableRevision,
    findings: Vec<Finding>,
    cx: &mut App,
) -> Result<Response, RequestError> {
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;

    let mut published = Vec::new();
    let mut fixes: HashMap<(usize, SharedString), Vec<StagedFix>> = HashMap::new();
    let mut stale = Vec::new();

    let table = state.read(cx);
    let delegate = table.delegate();
    let current = TableRevision(delegate.values_generation());

    for (ix, finding) in findings.into_iter().enumerate() {
        let cell = (finding.row < delegate.row_count())
            .then(|| delegate.row_fields(finding.row))
            .and_then(|fields| {
                fields
                    .into_iter()
                    .find(|(column, _)| *column == finding.column)
                    .map(|(_, value)| value)
            });
        // A row past the end, a column that no longer exists, or a cell edited since the review:
        // publishing any of those would point the archivist at text the agent never read.
        let Some(cell) = cell.filter(|cell| *cell == finding.expected) else {
            stale.push(ix);
            continue;
        };

        let column = SharedString::from(finding.column);
        published.push(diagnostics::Diagnostic {
            location: Location {
                dataset: DATASET_MAIN.into(),
                row: Some(finding.row),
                column: Some(column.clone()),
            },
            severity: match finding.severity {
                Severity::Error => diagnostics::Severity::Error,
                Severity::Warning => diagnostics::Severity::Warning,
                Severity::Note => diagnostics::Severity::Note,
            },
            source: Source::Validator(AGENT_SOURCE.into()),
            message: finding.message.into(),
            // A computed finding carries no filing stamp — it is recomputed, not observed once.
            filed: None,
        });

        if let Some(replacement) = finding.replacement {
            fixes
                .entry((finding.row, column))
                .or_default()
                .push(StagedFix {
                    expected: cell,
                    replacement: replacement.into(),
                });
        }
    }

    let accepted = published.len();
    diagnostics::Diagnostics::set(
        &Source::Validator(AGENT_SOURCE.into()),
        DATASET_MAIN,
        published,
        cx,
    );
    cx.set_global(StagedFixes {
        revision: revision.0,
        by_cell: fixes,
    });
    FixProviders::register(AGENT_SOURCE, offer_staged, cx);
    log::info!(
        "agent staged {accepted} finding(s) against revision {}, dropping {} judged against text \
         the table has moved past (current revision {})",
        revision.0,
        stale.len(),
        current.0
    );

    Ok(Response {
        revision: current,
        result: ResultSet::Staged { accepted, stale },
    })
}

/// What the Fixes menu offers for a staged finding — but only while the cell still says what the
/// agent judged, because a cell edited since staging is a cell nobody reviewed.
fn offer_staged(location: &Location, text: &str, cx: &App) -> Vec<diagnostics::Fix> {
    let (Some(row), Some(column)) = (location.row, location.column.clone()) else {
        return Vec::new();
    };
    cx.try_global::<StagedFixes>()
        .and_then(|staged| staged.by_cell.get(&(row, column)))
        .map(|staged| {
            staged
                .iter()
                .filter(|fix| fix.expected == text)
                .map(|fix| diagnostics::Fix {
                    label: format!("Use “{}”", fix.replacement).into(),
                    replacement: fix.replacement.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn row_from(delegate: &QrateTableDelegate, row: usize) -> Row {
    Row {
        index: row,
        fields: delegate
            .row_fields(row)
            .into_iter()
            .map(|(column, value)| Field {
                column: column.to_string(),
                value: value.to_string(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note on `note.rs`'s test module.
    use ai::agent::{Finding, ProjectSummary, Request, RequestError, ResultSet, TableRevision};
    use diagnostics::{DATASET_MAIN, Location};
    use gpui::TestAppContext;

    use crate::{AGENT_SOURCE, TablePanel, TableStateHandle, respond_to_agent};

    fn project() -> settings::project::CurrentProject {
        settings::project::CurrentProject {
            file: std::env::temp_dir().join("qrate-agent-smoke.qrate"),
            data: settings::project::ProjectData {
                name: "T".into(),
                columns: Vec::new(),
                headers: vec!["Title".into(), "Medium".into()],
                rows: vec![
                    vec!["Harvest".into(), "Film".into()],
                    vec!["Wharf".into(), "Video".into()],
                    vec!["Cannery".into(), "Film".into()],
                ],
                values: Default::default(),
            },
        }
    }

    fn open(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(project());
        });
        cx.add_window_view(TablePanel::new);
    }

    #[gpui::test]
    fn no_open_project_does_not_leak_table_state(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(
                respond_to_agent(Request::ProjectSummary, cx),
                Err(RequestError::ProjectUnavailable)
            );
        });
    }

    /// The end of the bridge an agent actually grades its review on: real cell values out of the
    /// live delegate, not the shape of the protocol.
    #[gpui::test]
    fn the_live_table_answers_with_its_own_contents(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            assert!(matches!(
                respond_to_agent(Request::ProjectSummary, cx)
                    .unwrap()
                    .result,
                ResultSet::ProjectSummary(ProjectSummary {
                    row_count: 3,
                    column_count: 2,
                    ..
                })
            ));

            let ResultSet::Columns { columns } =
                respond_to_agent(Request::Columns, cx).unwrap().result
            else {
                panic!("columns answered with another result set");
            };
            let names: Vec<_> = columns.iter().map(|column| column.name.as_str()).collect();
            assert_eq!(names, ["Title", "Medium"]);

            let ResultSet::Rows { rows } = respond_to_agent(
                Request::SearchRows {
                    query: "video".into(),
                    limit: 10,
                },
                cx,
            )
            .unwrap()
            .result
            else {
                panic!("search answered with another result set");
            };
            assert_eq!(rows.len(), 1, "case-insensitive search matched cell values");
            assert_eq!(rows[0].index, 1);
            assert_eq!(rows[0].fields[0].value, "Wharf");
        });
    }

    /// A filtered view is what the archivist sees, but an index that means different rows
    /// depending on the filter would make every quoted finding unfindable.
    #[gpui::test]
    fn a_filter_does_not_move_the_row_indices_an_agent_is_given(cx: &mut TestAppContext) {
        open(cx);

        let state = cx.update(|cx| {
            cx.try_global::<TableStateHandle>()
                .and_then(|handle| handle.0.upgrade())
                .expect("the panel publishes its state handle")
        });
        // Keep only "Film" — source rows 0 and 2, so view row 1 is source row 2.
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.delegate_mut().set_column_kept(1, &["Film".into()]);
                cx.notify();
            });
            state.update(cx, |state, cx| state.set_selected_row(1, cx));
        });
        cx.run_until_parked();

        cx.update(|cx| {
            assert!(
                matches!(
                    respond_to_agent(Request::ProjectSummary, cx)
                        .unwrap()
                        .result,
                    ResultSet::ProjectSummary(ProjectSummary { row_count: 3, .. })
                ),
                "the filter hid a row from the count"
            );
            assert_eq!(
                respond_to_agent(Request::SelectedRows, cx).unwrap().result,
                ResultSet::SelectedRows { rows: vec![2] }
            );

            let ResultSet::Rows { rows } = respond_to_agent(Request::Rows { rows: vec![2] }, cx)
                .unwrap()
                .result
            else {
                panic!("rows answered with another result set");
            };
            assert_eq!(rows[0].fields[0].value, "Cannery");
        });
    }

    fn finding(row: usize, column: &str, expected: &str, replacement: &str) -> Finding {
        Finding {
            row,
            column: column.into(),
            severity: ai::agent::Severity::Warning,
            message: "the medium contradicts the title".into(),
            expected: expected.into(),
            replacement: Some(replacement.into()),
        }
    }

    /// Staging is the whole of the agent's power over the data: a finding reaches the Problems
    /// panel and its correction reaches the Fixes menu, but the cell keeps the text it had. A
    /// draft judged against text the cell no longer carries is dropped rather than published,
    /// because that is a cell nobody reviewed.
    #[gpui::test]
    fn staged_findings_are_drafts_and_stale_ones_are_dropped(cx: &mut TestAppContext) {
        open(cx);

        cx.update(|cx| {
            let staged = respond_to_agent(
                Request::StageFindings {
                    revision: TableRevision(0),
                    findings: vec![
                        finding(0, "Title", "Harvest", "Harvest, 1962"),
                        finding(1, "Medium", "Kinescope", "Video"),
                        finding(99, "Title", "", "Nowhere"),
                    ],
                },
                cx,
            )
            .unwrap()
            .result;
            assert_eq!(
                staged,
                ResultSet::Staged {
                    accepted: 1,
                    stale: vec![1, 2]
                },
                "row 1's Medium says Video, not Kinescope, and row 99 does not exist"
            );

            let published: Vec<_> =
                diagnostics::Diagnostics::at(DATASET_MAIN, Some(0), Some("Title"), cx).collect();
            assert_eq!(published.len(), 1);
            assert_eq!(published[0].source.label(), AGENT_SOURCE);
            assert_eq!(published[0].severity, diagnostics::Severity::Warning);

            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                column: Some("Title".into()),
            };
            let offered = diagnostics::fixes::at(&location, "Harvest", cx);
            assert_eq!(
                offered.len(),
                1,
                "the fix is offered against the judged text"
            );
            assert_eq!(offered[0].replacement, "Harvest, 1962");
            let ResultSet::Rows { rows } = respond_to_agent(Request::Rows { rows: vec![0] }, cx)
                .unwrap()
                .result
            else {
                panic!("rows answered with another result set");
            };
            assert_eq!(
                rows[0].fields[0].value, "Harvest",
                "staging a replacement must not write the cell"
            );
            assert!(
                diagnostics::fixes::at(&location, "Harvest, retitled by hand", cx).is_empty(),
                "a cell edited since staging is offered nothing"
            );
        });
    }

    /// A second batch replaces the first rather than stacking beside it, so an agent that re-reads
    /// and re-stages retracts what it no longer stands by. An empty batch is how it retracts all.
    #[gpui::test]
    fn re_staging_replaces_the_previous_batch(cx: &mut TestAppContext) {
        open(cx);

        let stage = |findings: Vec<Finding>, cx: &mut gpui::App| {
            respond_to_agent(
                Request::StageFindings {
                    revision: TableRevision(0),
                    findings,
                },
                cx,
            )
            .unwrap()
        };
        let agent_findings = |cx: &gpui::App| {
            diagnostics::Diagnostics::all(cx)
                .iter()
                .filter(|d| d.source.label() == AGENT_SOURCE)
                .count()
        };

        cx.update(|cx| {
            stage(
                vec![
                    finding(0, "Title", "Harvest", "Harvest, 1962"),
                    finding(1, "Title", "Wharf", "Wharf, 1958"),
                ],
                cx,
            );
            assert_eq!(agent_findings(cx), 2);

            stage(vec![finding(0, "Title", "Harvest", "Harvest, 1962")], cx);
            assert_eq!(agent_findings(cx), 1, "the second batch replaced the first");

            stage(Vec::new(), cx);
            assert_eq!(agent_findings(cx), 0, "an empty batch retracts everything");
        });
    }
}

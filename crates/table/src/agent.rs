//! Live-table adapter for the external-agent read contract.

use ai::agent::{
    Column, Diagnostic, Field, ProjectSummary, Request, RequestError, Response, ResultSet, Row,
    Severity, TableRevision,
};
use gpui::App;

use crate::{QrateTableDelegate, TableStateHandle};

/// Answer one validated agent request from qrate's live table state.
///
/// This deliberately reads the delegate rather than the `.qrate` database, so responses include
/// unsaved cell edits and the current table generation. There is no write operation in this API.
pub fn respond_to_agent(request: Request, cx: &App) -> Result<Response, RequestError> {
    request.validate()?;

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
    };

    Ok(Response { revision, result })
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
    use ai::agent::{ProjectSummary, Request, RequestError, ResultSet};
    use gpui::TestAppContext;

    use crate::{TablePanel, TableStateHandle, respond_to_agent};

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
}

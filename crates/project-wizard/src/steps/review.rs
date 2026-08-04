//! Stage 5 · Review & Create — every path converges here.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, StyledExt, h_flex, v_flex};

use crate::launcher;
use crate::project;
use crate::recent;
use crate::wizard::{ColumnSource, EntryKind, LinkMethod, ProjectWizard, WizardStep};

impl ProjectWizard {
    pub(crate) fn create_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.project_name(cx);
        let source = match self.entry_kind {
            EntryKind::Blank => "Blank".to_string(),
            EntryKind::Csv => "CSV + folder".to_string(),
            EntryKind::Sheet => "Google Sheet".to_string(),
        };
        // When files are skipped the folder/link state is stale — don't let the
        // project claim a link method was chosen.
        let link_method = (!self.skips_link()).then_some(match self.link_method {
            LinkMethod::ExactFilename => "exact filename",
            LinkMethod::CustomPattern => "custom pattern",
        });
        // Prefer config from the Columns step; else fall back to spreadsheet headers, all Text.
        let columns: Vec<project::ProjectColumn> = self
            .config_preview
            .as_ref()
            .map(|p| {
                p.entries
                    .iter()
                    .map(|e| project::ProjectColumn {
                        name: e.name.clone(),
                        data_type: e.data_type.clone(),
                        notes: e.description.clone(),
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                self.spreadsheet_headers()
                    .into_iter()
                    .map(|name| project::ProjectColumn {
                        name,
                        data_type: "Text".into(),
                        notes: String::new(),
                    })
                    .collect()
            });
        // The imported rows themselves — the whole point of the `.qrate` file.
        let (headers, rows) = self
            .csv_preview
            .as_ref()
            .map(|p| (p.headers.clone(), p.rows.clone()))
            .unwrap_or_default();
        // Skipped files → the folder field is stale, same reasoning as `link_method` above.
        let files_folder = (!self.skip_files && !self.folder_path.trim().is_empty())
            .then_some(self.folder_path.as_str());

        match project::write_project_file(
            &self.save_path,
            &name,
            &source,
            link_method,
            files_folder,
            &columns,
            &headers,
            &rows,
        ) {
            Ok(file) => {
                // Imported notes become Problems-panel entries. Non-fatal: a lost note must not
                // fail project creation. `open_project` below wakes the diagnostics loader,
                // which reads them straight back, so this write is the single source of truth.
                if let Some(preview) = &self.csv_preview {
                    let notes: Vec<_> = preview
                        .notes
                        .iter()
                        .map(|n| project::StoredNote {
                            dataset: diagnostics::DATASET_MAIN.into(),
                            row: Some(n.row),
                            column: Some(n.column.clone()),
                            severity: "note".into(),
                            message: n.text.clone(),
                        })
                        .collect();
                    if let Err(e) = project::write_notes(
                        std::path::Path::new(&file),
                        diagnostics::SOURCE_NOTE,
                        &notes,
                    ) {
                        eprintln!("Couldn't save the sheet's notes — {e}");
                    }
                }
                // Load the file straight back so the main window opens on the
                // real, round-tripped data (same path the launcher uses).
                if let Err(e) = project::open_project(std::path::Path::new(&file), cx) {
                    self.name_error = Some(format!("Couldn't open the new project — {e}").into());
                    self.step = WizardStep::Name;
                    return;
                }
                recent::record_opened(name, file, cx);
                // No success screen — hand off to the main app right away.
                if let Some(hooks) = cx.try_global::<launcher::LauncherHooks>().copied() {
                    (hooks.open_main_window)(cx);
                }
                window.remove_window();
            }
            Err(e) => {
                self.name_error = Some(format!("Couldn't create the project — {e}").into());
                self.step = WizardStep::Name;
            }
        }
    }

    pub(crate) fn render_review_step(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = self.project_name(cx);
        let source = match self.entry_kind {
            EntryKind::Blank => "Blank project",
            EntryKind::Csv => "CSV + folder",
            EntryKind::Sheet => "Google Sheet",
        };
        let spreadsheet_line = self
            .csv_preview
            .as_ref()
            .map(|p| format!("{} rows · {} columns", p.rows.len(), p.headers.len()));
        // Skipped files → no folder was matched, so don't show a stale Files line.
        let files_line = (!self.skip_files)
            .then(|| {
                self.folder_match.as_ref().map(|m| {
                    let method = match self.link_method {
                        LinkMethod::ExactFilename => "exact filename",
                        LinkMethod::CustomPattern => "custom pattern",
                    };
                    let extra = if m.extra_files > 0 {
                        format!(" · {} not linked", m.extra_files)
                    } else {
                        String::new()
                    };
                    format!(
                        "{} of {} matched ({method}){extra}",
                        m.matched_rows, m.total_rows
                    )
                })
            })
            .flatten();
        let column_count = self
            .config_preview
            .as_ref()
            .map(|c| c.entries.len())
            .unwrap_or_else(|| self.spreadsheet_headers().len());
        let columns_line = {
            let source_desc = match self.column_source {
                ColumnSource::AutoFromSpreadsheet => "Auto-matched from spreadsheet",
                ColumnSource::LoadFromFileOrSheet => "Loaded from file/Sheet",
                ColumnSource::SkipForNow => "Set up later",
            };
            format!("{source_desc} · {column_count} columns")
        };

        v_flex()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child("Ready to create your project"),
            )
            .child(
                v_flex()
                    .gap_1()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_sm()
                    .child(summary_row("Name", &name))
                    .child(summary_row("Location", &self.save_path))
                    .child(summary_row("Source", source))
                    .when_some(spreadsheet_line.clone(), |el, line| {
                        el.child(summary_row("Spreadsheet", &line))
                    })
                    .when_some(files_line.clone(), |el, line| {
                        el.child(summary_row("Files", &line))
                    })
                    // Blank projects go through Columns too, so always show it.
                    .child(summary_row("Columns", &columns_line)),
            )
        // The shared wizard footer supplies the "Create Project" (Next) and
        // "← Back" controls — see `ProjectWizard::render_footer`.
    }
}

fn summary_row(label: &str, value: &str) -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(Label::new(format!("{label}:")).font_semibold())
        .child(value.to_string())
}

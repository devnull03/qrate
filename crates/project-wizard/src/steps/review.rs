//! Stage 5 · Review & Create — every path converges here.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, StyledExt, h_flex, v_flex};

use crate::launcher;
use crate::project::{self, ProjectManifest};
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
        // manifest claim files were matched or a link method was chosen.
        let files_matched = (!self.skip_files)
            .then(|| {
                self.folder_match
                    .as_ref()
                    .map(|m| (m.matched_rows, m.total_rows))
            })
            .flatten();
        let link_method = (!self.skips_link()).then(|| match self.link_method {
            LinkMethod::ExactFilename => "exact filename".to_string(),
            LinkMethod::CustomPattern => "custom pattern".to_string(),
        });
        // Blank projects go through the Columns step too, so honor any config
        // loaded there; otherwise fall back to spreadsheet headers (empty for
        // Blank), every column defaulting to Text.
        let columns = self
            .config_preview
            .as_ref()
            .map(|p| {
                p.entries
                    .iter()
                    .map(|e| project::ManifestColumn {
                        name: e.name.clone(),
                        data_type: e.data_type.clone(),
                        description: e.description.clone(),
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                self.spreadsheet_headers()
                    .into_iter()
                    .map(|name| project::ManifestColumn {
                        name,
                        data_type: "Text".into(),
                        description: String::new(),
                    })
                    .collect()
            });

        let manifest = ProjectManifest {
            name: name.clone(),
            source,
            files_matched,
            link_method,
            columns,
        };

        match project::write_project_file(&self.save_path, &name, &manifest) {
            Ok(dir) => {
                recent::record_opened(name, dir, cx);
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
            .map(|p| format!("{} rows · {} columns", p.row_count(), p.column_count()));
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
                ColumnSource::RecentlyUsed => "From a saved config",
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

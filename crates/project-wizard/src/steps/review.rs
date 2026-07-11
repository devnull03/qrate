//! Stage 5 · Review & Create — every path converges here.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, StyledExt, h_flex, v_flex};

use crate::project::{self, ProjectManifest};
use crate::recent;
use crate::wizard::{ColumnSource, EntryKind, LinkMethod, ProjectWizard, WizardStep};

impl ProjectWizard {
    pub(crate) fn create_project(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.project_name(cx);
        let source = match self.entry_kind {
            EntryKind::Blank => "Blank".to_string(),
            EntryKind::Csv => "CSV + folder".to_string(),
            EntryKind::Sheet => "Google Sheet".to_string(),
        };
        let files_matched = self
            .folder_match
            .as_ref()
            .map(|m| (m.matched_rows, m.total_rows));
        let link_method = (self.entry_kind != EntryKind::Blank).then(|| {
            match self.link_method {
                LinkMethod::ExactFilename => "exact filename".to_string(),
                LinkMethod::CustomPattern => "custom pattern".to_string(),
            }
        });
        let columns = match self.entry_kind {
            EntryKind::Blank => Vec::new(),
            _ => self
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
                    // No explicit config: auto-created from the spreadsheet,
                    // every column defaults to Text (same rule the mocked
                    // Sheet-config load uses).
                    self.spreadsheet_headers()
                        .into_iter()
                        .map(|name| project::ManifestColumn {
                            name,
                            data_type: "Text".into(),
                            description: String::new(),
                        })
                        .collect()
                }),
        };

        let manifest = ProjectManifest {
            name: name.clone(),
            source,
            files_matched,
            link_method,
            columns,
        };

        match project::write_project_file(&self.save_path, &name, &manifest) {
            Ok(dir) => {
                recent::record_opened(name, dir.clone(), cx);
                self.created_dir = Some(dir);
                self.step = WizardStep::Success;
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
        let files_line = self.folder_match.as_ref().map(|m| {
            let method = match self.link_method {
                LinkMethod::ExactFilename => "exact filename",
                LinkMethod::CustomPattern => "custom pattern",
            };
            format!("{} of {} ({method})", m.matched_rows, m.total_rows)
        });
        let columns_line = match self.column_source {
            ColumnSource::AutoFromSpreadsheet => "Auto-matched from spreadsheet".to_string(),
            ColumnSource::RecentlyUsed => "From a saved config".to_string(),
            ColumnSource::LoadFromFileOrSheet => "Loaded from file/Sheet".to_string(),
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
                    .when_some(files_line.clone(), |el, line| {
                        el.child(summary_row("Files matched", &line))
                    })
                    .when(self.entry_kind != EntryKind::Blank, |el| {
                        el.child(summary_row("Columns", &columns_line))
                    }),
            )
            .child(
                Button::new("create-project")
                    .label("Create Project")
                    .primary()
                    .w_full()
                    .on_click(cx.listener(|this, _, window, cx| this.create_project(window, cx))),
            )
            .child(
                div()
                    .id("review-back")
                    .cursor_pointer()
                    .text_sm()
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child("← Back")
                    .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx))),
            )
    }
}

fn summary_row(label: &str, value: &str) -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(Label::new(format!("{label}:")).font_semibold())
        .child(value.to_string())
}

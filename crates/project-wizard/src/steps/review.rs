//! Stage 5 · Review & Create — every path converges here.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::description_list::DescriptionList;
use gpui_component::{Sizable, StyledExt, v_flex};

use plugin_api::ColumnMapContributions;
use settings::columns::{ColumnSettings, ColumnSettingsMap};

use crate::data::ColumnConfigPreview;
use crate::launcher;
use crate::project;
use crate::recent;
use crate::wizard::{ColumnSource, EntryKind, LinkMethod, ProjectWizard, WizardStep};

/// The half of a column config that `__columns` cannot hold: what each column is checked against,
/// whether it is spell-checked, how loud its findings are, and whichever of the file's remaining
/// columns an active plugin recognises as its own mapping.
///
/// Keyed by header name, the identity the table mints and the settings page reads. A config row
/// naming a column the sheet doesn't have is dropped here; the wizard already warned about it on
/// the Columns step.
fn column_settings(
    headers: &[String],
    preview: Option<&ColumnConfigPreview>,
    cx: &gpui::App,
) -> ColumnSettingsMap {
    let mut map = ColumnSettingsMap::new();
    let Some(preview) = preview else {
        return map;
    };
    let maps = ColumnMapContributions::all(cx);

    for header in headers {
        let Some(entry) = preview
            .entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(header))
        else {
            continue;
        };

        let mut settings = ColumnSettings {
            authority: entry.authority.clone(),
            ..Default::default()
        };
        if let Some(on) = entry.spellcheck {
            settings.spellcheck = on;
        }
        // Severity is per producer: the authority names its own, and each plugin's rides in the
        // `<id>::Severity` column beside its mapping.
        if let (Some(authority), Some(severity)) =
            (settings.authority.clone(), entry.authority_severity.clone())
        {
            settings.severity.insert(authority, severity);
        }
        for (plugin, spec) in &maps {
            if let Some(cell) = entry.extra.get(&format!("{plugin}::{}", spec.label)) {
                let chosen: Vec<gpui::SharedString> = cell
                    .split(';')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(|v| v.to_string().into())
                    .collect();
                ColumnMapContributions::put(plugin, spec, &mut settings, &chosen);
            }
            if let Some(severity) = entry.extra.get(&format!("{plugin}::Severity")) {
                settings
                    .severity
                    .insert(plugin.to_string(), severity.to_ascii_lowercase());
            }
        }

        if settings != ColumnSettings::default() {
            map.insert(header.clone(), settings);
        }
    }
    map
}

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
                        log::error!("couldn't save the sheet's notes — {e}");
                    }
                }
                // Everything in the column config that `__columns` has no room for. Written before
                // the project is opened, so the first validation run already sees it. Non-fatal for
                // the same reason the notes above are.
                let settings = column_settings(&headers, self.config_preview.as_ref(), cx);
                if !settings.is_empty() {
                    match serde_json::to_string(&settings) {
                        Ok(json) => {
                            if let Err(e) = settings::project::write_setting(
                                std::path::Path::new(&file),
                                settings::columns::COLUMN_SETTINGS_KEY,
                                &json,
                            ) {
                                log::error!("couldn't save the imported column settings — {e}");
                            }
                        }
                        Err(e) => log::error!("couldn't save the imported column settings — {e}"),
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
                // One column, so every row reads label-then-value down the card; `bordered` (the
                // default) is what draws the card.
                DescriptionList::new()
                    .columns(1)
                    .small()
                    .item("Name", name.to_string(), 1)
                    .item("Location", self.save_path.clone(), 1)
                    .item("Source", source, 1)
                    .when_some(spreadsheet_line.clone(), |list, line| {
                        list.item("Spreadsheet", line, 1)
                    })
                    .when_some(files_line.clone(), |list, line| list.item("Files", line, 1))
                    // Blank projects go through Columns too, so always show it.
                    .item("Columns", columns_line, 1),
            )
        // The shared wizard footer supplies the "Create Project" (Next) and
        // "← Back" controls — see `ProjectWizard::render_footer`.
    }
}

//! Stage 5 · Review & Create — every path converges here.

use std::collections::{BTreeSet, HashMap};

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
        if let Some(on) = entry.variant_review {
            settings.variant_review = on;
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

fn project_columns(
    headers: &[String],
    preview: Option<&ColumnConfigPreview>,
    title_column: &str,
    file_column: &str,
) -> Vec<project::ProjectColumn> {
    let mut columns: Vec<project::ProjectColumn> = preview
        .map(|preview| {
            preview
                .entries
                .iter()
                .map(|entry| project::ProjectColumn {
                    name: entry.name.clone(),
                    data_type: entry.data_type.clone(),
                    notes: entry.description.clone(),
                })
                .collect()
        })
        .unwrap_or_else(|| {
            headers
                .iter()
                .map(|name| project::ProjectColumn {
                    name: name.clone(),
                    data_type: settings::columns::ColumnType::Text.as_str().into(),
                    notes: String::new(),
                })
                .collect()
        });

    for required in [title_column, file_column] {
        if !required.is_empty()
            && !columns
                .iter()
                .any(|column| column.name.eq_ignore_ascii_case(required))
        {
            columns.push(project::ProjectColumn {
                name: required.to_string(),
                data_type: settings::columns::ColumnType::Text.as_str().into(),
                notes: String::new(),
            });
        }
    }
    for column in &mut columns {
        let kind = settings::columns::ColumnType::from_declared(&column.data_type);
        if matches!(
            kind,
            settings::columns::ColumnType::Title | settings::columns::ColumnType::Filename
        ) {
            column.data_type = settings::columns::ColumnType::Text.as_str().into();
        }
        if column.name.eq_ignore_ascii_case(title_column) {
            column.name = title_column.to_string();
            column.data_type = settings::columns::ColumnType::Title.as_str().into();
        } else if column.name.eq_ignore_ascii_case(file_column) {
            column.name = file_column.to_string();
            column.data_type = settings::columns::ColumnType::Filename.as_str().into();
        }
    }
    columns
}

fn append_folder_components(
    headers: &[String],
    rows: &mut Vec<Vec<String>>,
    title_column: Option<&str>,
    file_column: Option<&str>,
    folder: &str,
    recursive: bool,
    description: &settings::description::DescriptionConfig,
) -> Vec<project::RowStructure> {
    let Some(file_col) =
        file_column.and_then(|name| headers.iter().position(|header| header == name))
    else {
        return Vec::new();
    };
    let title_col = title_column.and_then(|name| headers.iter().position(|header| header == name));
    let plan = match file_ingest::plan(
        std::path::Path::new(folder),
        &file_ingest::PlanOptions {
            recursive,
            include_root: true,
            folder_level_key: &description.folder_level_key,
            file_level_key: &description.file_level_key,
        },
    ) {
        Ok(plan) => plan,
        Err(error) => {
            log::warn!(
                "The new project was created without rows for the files in {folder}: the folder could not be read ({error:?})"
            );
            return Vec::new();
        }
    };
    for warning in &plan.warnings {
        log::warn!("Skipped a path while importing {folder}: {warning:?}");
    }
    let mut files_by_key: HashMap<String, BTreeSet<usize>> = HashMap::new();
    for (index, component) in plan.components.iter().enumerate() {
        if component.kind == file_ingest::EntryKind::File {
            let path = file_ingest::normalized_path(&component.source_path);
            for key in settings::filenames::lookup_keys(&path) {
                files_by_key.entry(key).or_default().insert(index);
            }
        }
    }
    let claimed: HashMap<usize, usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(source, row)| {
            let matches: Vec<usize> = settings::filenames::lookup_keys(row.get(file_col)?)
                .iter()
                .filter_map(|key| files_by_key.get(key))
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            (matches.len() == 1).then(|| (matches[0], source))
        })
        .collect();
    let mut next_order: HashMap<Option<project::RowId>, i64> = HashMap::new();
    let mut planned_rows = Vec::<usize>::with_capacity(plan.components.len());
    let mut structure: Vec<project::RowStructure> = Vec::with_capacity(plan.components.len());
    for (component_index, component) in plan.components.into_iter().enumerate() {
        let source_path = file_ingest::normalized_path(&component.source_path);
        let existing = claimed.get(&component_index).copied();
        let source = existing.unwrap_or_else(|| {
            let mut row = vec![String::new(); headers.len()];
            if let Some(title_col) = title_col {
                row[title_col] = component.title.clone();
            }
            if component.kind == file_ingest::EntryKind::File {
                row[file_col] = source_path.clone();
            }
            rows.push(row);
            rows.len() - 1
        });
        planned_rows.push(source);
        let parent_id = component
            .parent
            .and_then(|parent| planned_rows.get(parent))
            .map(|source| *source as project::RowId + 1);
        let sibling_order = next_order.entry(parent_id).or_default();
        *sibling_order += 1;
        structure.push(project::RowStructure {
            row_id: source as project::RowId + 1,
            parent_id,
            level_key: component.level_key,
            sibling_order: *sibling_order - 1,
            source_path: Some(source_path),
            source_kind: Some(match component.kind {
                file_ingest::EntryKind::File => project::SourceKind::File,
                file_ingest::EntryKind::Directory => project::SourceKind::Directory,
            }),
        });
    }
    structure
}

impl ProjectWizard {
    pub(crate) fn create_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.project_name(cx);
        let source = match self.entry_kind {
            EntryKind::Blank => "Blank".to_string(),
            EntryKind::LocalFile => "Spreadsheet + folder".to_string(),
            EntryKind::Sheet => "Google Sheet".to_string(),
        };
        // When files are skipped the folder/link state is stale — don't let the
        // project claim a link method was chosen.
        let link_method = (!self.skips_link()).then_some(match self.link_method {
            LinkMethod::ExactFilename => "exact filename",
            LinkMethod::CustomPattern => "custom pattern",
        });
        let spreadsheet_headers = self.spreadsheet_headers();
        let columns = project_columns(
            &spreadsheet_headers,
            self.config_preview.as_ref(),
            self.title_column.as_deref().unwrap_or_default(),
            self.file_column.as_deref().unwrap_or_default(),
        );
        // The imported rows themselves — the whole point of the `.qrate` file. Blank projects
        // still create the empty dataset with their required Title and File headers.
        let (headers, mut rows) = self
            .spreadsheet_preview
            .as_ref()
            .map(|preview| (preview.headers.clone(), preview.rows.clone()))
            .unwrap_or_else(|| (spreadsheet_headers, Vec::new()));
        // A file in the folder that no row names is still part of the collection. It arrives as
        // its own row, empty but for the filename, so it can be catalogued in qrate instead of
        // being noticed only when someone counts the folder.
        let description = self.description_config(cx);
        let structure = if !self.skip_files {
            append_folder_components(
                &headers,
                &mut rows,
                self.title_column.as_deref(),
                self.file_column.as_deref(),
                &self.folder_path,
                self.recurse_subfolders,
                &description,
            )
        } else {
            Vec::new()
        };
        // A project with no rows is a grid with nothing to type into — the first row has to be
        // created before anything else can be, so create it here rather than making the archivist
        // find Insert Row on an empty screen.
        if rows.is_empty() {
            rows.push(vec![String::new(); headers.len()]);
        }
        // Skipped files → the folder field is stale, same reasoning as `link_method` above.
        let files_folder = (!self.skip_files && !self.folder_path.trim().is_empty())
            .then_some(self.folder_path.as_str());

        match project::write_project_file(
            &self.save_path,
            &project::ProjectSpec {
                name: &name,
                source: &source,
                link_method,
                files_folder,
                columns: &columns,
                headers: &headers,
                rows: &rows,
            },
        ) {
            Ok(file) => {
                log::info!(
                    "created project {file} from {source}: {} rows, {} arranged components, {} profile",
                    rows.len(),
                    structure.len(),
                    description.profile.key()
                );
                if !structure.is_empty()
                    && let Err(error) =
                        project::write_row_structure(std::path::Path::new(&file), &structure)
                {
                    log::error!("couldn't save the imported folder hierarchy — {error}");
                }
                for (key, value) in [
                    (
                        settings::description::DESCRIPTION_PROFILE_KEY,
                        description.profile.key().to_string(),
                    ),
                    (
                        settings::description::DESCRIPTION_LEVELS_KEY,
                        serde_json::to_string(&description.levels).unwrap_or_default(),
                    ),
                    (
                        settings::description::FOLDER_LEVEL_KEY,
                        description.folder_level_key.clone(),
                    ),
                    (
                        settings::description::FILE_LEVEL_KEY,
                        description.file_level_key.clone(),
                    ),
                ] {
                    if let Err(error) =
                        settings::project::write_setting(std::path::Path::new(&file), key, &value)
                    {
                        log::error!("couldn't save description profile setting {key} — {error}");
                    }
                }
                // Imported notes become Problems-panel entries. Non-fatal: a lost note must not
                // fail project creation. `open_project` below wakes the diagnostics loader,
                // which reads them straight back, so this write is the single source of truth.
                if let Some(preview) = &self.spreadsheet_preview {
                    let notes: Vec<_> = preview
                        .notes
                        .iter()
                        .map(|n| project::StoredNote {
                            dataset: diagnostics::DATASET_MAIN.into(),
                            row: Some(n.row),
                            // Creation inserts rows in source order, starting at SQLite id 1.
                            row_id: Some(n.row as project::RowId + 1),
                            column: Some(n.column.clone()),
                            severity: "note".into(),
                            message: n.text.clone(),
                            // A spreadsheet comment carries neither, and stamping the import date
                            // would claim the note was written the day the project was made.
                            created_at: None,
                            author: None,
                        })
                        .collect();
                    if let Err(e) = project::write_notes(
                        std::path::Path::new(&file),
                        diagnostics::SOURCE_NOTE,
                        &notes,
                        &[],
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
            EntryKind::LocalFile => "Spreadsheet + folder",
            EntryKind::Sheet => "Google Sheet",
        };
        let spreadsheet_line = self
            .spreadsheet_preview
            .as_ref()
            .map(|p| format!("{} rows · {} columns", p.rows.len(), p.headers.len()));
        // Skipped files → no folder was matched, so don't show a stale Files line.
        let files_line = (!self.skip_files)
            .then(|| {
                self.folder_match.as_ref().map(|m| {
                    if self.entry_kind == EntryKind::Blank {
                        return format!(
                            "{} file{} · each becomes a table row",
                            m.extra_files.len(),
                            if m.extra_files.len() == 1 { "" } else { "s" },
                        );
                    }
                    let method = match self.link_method {
                        LinkMethod::ExactFilename => "exact filename",
                        LinkMethod::CustomPattern => "custom pattern",
                    };
                    let extra = if !m.extra_files.is_empty() {
                        format!(" · {} added as empty rows", m.extra_files.len())
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
            let source_desc = if self.entry_kind == EntryKind::Blank {
                "Required Title and File columns"
            } else {
                match self.column_source {
                    ColumnSource::AutoFromSpreadsheet => "Auto-matched from spreadsheet",
                    ColumnSource::LoadFromFileOrSheet => "Loaded from file/Sheet",
                    ColumnSource::SkipForNow => "Set up later",
                }
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

#[cfg(test)]
mod tests {
    use settings::columns::ColumnType;

    use crate::data::{ColumnConfigEntry, ColumnConfigPreview};

    use super::{append_folder_components, project_columns};

    fn roles(columns: &[crate::project::ProjectColumn]) -> Vec<(&str, ColumnType)> {
        columns
            .iter()
            .map(|column| {
                (
                    column.name.as_str(),
                    ColumnType::from_declared(&column.data_type),
                )
            })
            .collect()
    }

    #[test]
    fn configured_columns_materialize_the_required_roles() {
        let headers = vec!["Object Name".into(), "Digital Path".into()];
        let preview = ColumnConfigPreview {
            entries: vec![
                ColumnConfigEntry {
                    name: "Object Name".into(),
                    data_type: "Title".into(),
                    ..Default::default()
                },
                ColumnConfigEntry {
                    name: "Digital Path".into(),
                    data_type: "Filename".into(),
                    ..Default::default()
                },
            ],
        };

        assert_eq!(
            roles(&project_columns(
                &headers,
                Some(&preview),
                "Object Name",
                "Digital Path"
            )),
            vec![
                ("Object Name", ColumnType::Title),
                ("Digital Path", ColumnType::Filename)
            ]
        );
    }

    #[test]
    fn bare_headers_materialize_the_required_roles() {
        let headers = vec!["Title".into(), "File".into(), "Notes".into()];

        assert_eq!(
            roles(&project_columns(&headers, None, "Title", "File")),
            vec![
                ("Title", ColumnType::Title),
                ("File", ColumnType::Filename),
                ("Notes", ColumnType::Text)
            ]
        );
    }

    #[test]
    fn blank_folder_tree_becomes_archival_component_rows() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("series")).unwrap();
        std::fs::write(folder.path().join("series").join("one.jpg"), "photo").unwrap();
        let headers = vec!["Title".into(), "File".into()];
        let mut rows = Vec::new();
        let description = settings::description::DescriptionProfile::Rad.defaults();

        let structure = append_folder_components(
            &headers,
            &mut rows,
            Some("Title"),
            Some("File"),
            folder.path().to_str().unwrap(),
            true,
            &description,
        );

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], ["series", ""]);
        assert_eq!(rows[2], ["one.jpg", "series/one.jpg"]);
        assert_eq!(structure[1].parent_id, Some(1));
        assert_eq!(structure[2].parent_id, Some(2));
        assert_eq!(structure[1].level_key, "series");
        assert_eq!(structure[2].level_key, "item");
    }
}

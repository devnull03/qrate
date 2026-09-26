//! Stage 5 · Review & Create — every path converges here.

use std::collections::HashMap;

use file_ingest::duplicates::{DuplicatePolicy, ExistingComponent as Existing, Parent, Resolution};

use gpui::{prelude::FluentBuilder, *};
use gpui_component::description_list::DescriptionList;
use gpui_component::label::Label;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme, Sizable, StyledExt, h_flex, v_flex};

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

        let mut settings = ColumnSettings::default();
        crate::column_config::apply_entry(entry, &maps, &mut settings);

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

/// What the Files and Columns steps settled on, as the folder import reads it.
struct FolderImport<'a> {
    headers: &'a [String],
    title_column: Option<&'a str>,
    file_column: Option<&'a str>,
    folder: &'a str,
    plan: &'a file_ingest::ImportPlan,
    description: &'a settings::description::DescriptionConfig,
    policy: DuplicatePolicy,
}

fn append_folder_components(
    import: &FolderImport<'_>,
    rows: &mut Vec<Vec<String>>,
) -> Vec<project::RowStructure> {
    let FolderImport {
        headers,
        title_column,
        file_column,
        folder,
        plan,
        description,
        policy,
    } = *import;
    let Some(file_col) =
        file_column.and_then(|name| headers.iter().position(|header| header == name))
    else {
        return Vec::new();
    };
    let title_col = title_column.and_then(|name| headers.iter().position(|header| header == name));
    let mut plan = plan.clone();
    for component in &mut plan.components {
        component.level_key = match component.kind {
            file_ingest::EntryKind::Directory => &description.folder_level_key,
            file_ingest::EntryKind::File => &description.file_level_key,
        }
        .clone();
    }
    for warning in &plan.warnings {
        log::warn!("Skipped a path while importing {folder}: {warning:?}");
    }
    // The spreadsheet rows are what this import can already be holding, so they are the existing
    // components: a file matching one of them links that row instead of adding another.
    let existing: Vec<Existing> = rows
        .iter()
        .enumerate()
        .map(|(source, row)| Existing {
            key: source as u64,
            absolute_source: None,
            filename_keys: row
                .get(file_col)
                .map(|value| settings::filenames::lookup_keys(value))
                .unwrap_or_default(),
        })
        .collect();
    let resolved =
        file_ingest::duplicates::resolve(plan, &existing, settings::filenames::keys, policy);
    if resolved.ambiguous() > 0 {
        log::info!(
            "{} file(s) in {folder} are named by more than one row and were left for the archivist",
            resolved.ambiguous()
        );
    }
    let mut next_order: HashMap<Option<project::RowId>, i64> = HashMap::new();
    let mut planned_rows = Vec::<usize>::with_capacity(resolved.plan.components.len());
    let mut structure: Vec<project::RowStructure> =
        Vec::with_capacity(resolved.plan.components.len());
    let parents = resolved.parents.clone();
    let resolutions = resolved.resolutions.clone();
    for (component_index, component) in resolved.plan.components.into_iter().enumerate() {
        let source_path = file_ingest::normalized_path(&component.source_path);
        let claimed = match &resolutions[component_index] {
            Resolution::Skip { existing } | Resolution::Update { existing } => {
                Some(*existing as usize)
            }
            // Several rows name this file. Only "update" picks one, by sheet order.
            Resolution::Ambiguous { candidates } => (policy == DuplicatePolicy::Update)
                .then(|| candidates.first().map(|row| *row as usize))
                .flatten(),
            Resolution::Create => None,
        };
        let source = claimed.unwrap_or_else(|| {
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
        let parent_id = match parents[component_index] {
            Some(Parent::Planned(parent)) => planned_rows.get(parent).copied(),
            Some(Parent::Existing(row)) => Some(row as usize),
            None => None,
        }
        .map(|source| source as project::RowId + 1);
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
        let spreadsheet_headers = self.effective_headers();
        let columns = project_columns(
            &spreadsheet_headers,
            self.selected_config(),
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
            let Some(plan) = self.folder_plan.as_ref() else {
                self.name_error = Some(
                    "The files preview is no longer available — choose the folder again".into(),
                );
                self.step = WizardStep::Files;
                return;
            };
            append_folder_components(
                &FolderImport {
                    headers: &headers,
                    title_column: self.title_column.as_deref(),
                    file_column: self.file_column.as_deref(),
                    folder: &self.folder_path,
                    plan,
                    description: &description,
                    policy: self.duplicate_policy,
                },
                &mut rows,
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
                settings::AppSettings::set_text(
                    crate::NEW_PROJECT_FOLDER_KEY,
                    self.save_path.clone().into(),
                    cx,
                );
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
                for (key, value) in description.values().into_iter().chain([(
                    settings::project::IMPORT_DUPLICATE_POLICY_KEY,
                    self.duplicate_policy.key().to_string(),
                )]) {
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
                let settings = column_settings(&headers, self.selected_config(), cx);
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
                settings::dirty::clear(settings::dirty::PROJECT_DATA, cx);
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
        let column_count = self.effective_headers().len();
        let columns_line = {
            let source_desc = match self.column_source {
                ColumnSource::AutoFromSpreadsheet => "Auto-matched from spreadsheet",
                ColumnSource::LoadFromFileOrSheet => "Loaded from file/Sheet",
                ColumnSource::DefaultBlank => "Default Title and File columns",
            };
            format!("{source_desc} · {column_count} columns")
        };
        let folder_plan = (!self.skip_files)
            .then_some(self.folder_plan.as_ref())
            .flatten();

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
            .when_some(folder_plan, |review, plan| {
                const MAX_VISIBLE: usize = 100;
                let files = plan
                    .components
                    .iter()
                    .filter(|component| component.kind == file_ingest::EntryKind::File)
                    .count();
                let folders = plan.components.len() - files;
                let mut depths = Vec::with_capacity(plan.components.len());
                for component in &plan.components {
                    depths.push(component.parent.map_or(0, |parent| depths[parent] + 1));
                }
                review.child(
                    v_flex()
                        .gap_2()
                        .child(div().font_semibold().child("Hierarchy preview"))
                        .child(
                            Label::new(format!(
                                "{} component{} · {files} file{} · {folders} folder{}",
                                plan.components.len(),
                                if plan.components.len() == 1 { "" } else { "s" },
                                if files == 1 { "" } else { "s" },
                                if folders == 1 { "" } else { "s" },
                            ))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            v_flex()
                                .max_h(px(280.))
                                .overflow_y_scrollbar()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .children(
                                    plan.components
                                        .iter()
                                        .zip(depths)
                                        .take(MAX_VISIBLE)
                                        .enumerate()
                                        .map(|(index, (component, depth))| {
                                            h_flex()
                                                .gap_2()
                                                .px_2()
                                                .py_1()
                                                .pl(px(8. + depth as f32 * 18.))
                                                .when(index > 0, |row| {
                                                    row.border_t_1().border_color(cx.theme().border)
                                                })
                                                .child(
                                                    Label::new(match component.kind {
                                                        file_ingest::EntryKind::Directory => {
                                                            "Folder"
                                                        }
                                                        file_ingest::EntryKind::File => "File",
                                                    })
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground),
                                                )
                                                .child(
                                                    Label::new(component.title.clone()).text_sm(),
                                                )
                                        }),
                                )
                                .when(plan.components.len() > MAX_VISIBLE, |tree| {
                                    tree.child(
                                        Label::new(format!(
                                            "… and {} more components",
                                            plan.components.len() - MAX_VISIBLE
                                        ))
                                        .px_2()
                                        .py_1()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground),
                                    )
                                }),
                        )
                        .when(!plan.warnings.is_empty(), |preview| {
                            preview.child(
                                Label::new(format!(
                                    "{} path warning{} will be skipped.",
                                    plan.warnings.len(),
                                    if plan.warnings.len() == 1 { "" } else { "s" }
                                ))
                                .text_sm()
                                .text_color(cx.theme().warning),
                            )
                        }),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use settings::columns::ColumnType;

    use crate::data::{ColumnConfigEntry, ColumnConfigPreview};

    use file_ingest::duplicates::DuplicatePolicy;

    use super::{FolderImport, append_folder_components, project_columns};

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

    /// ASNT-77: two rows naming one file is the archivist's ambiguity to settle, not qrate's.
    #[test]
    fn two_rows_naming_one_file_follow_the_chosen_policy() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("one.jpg"), "photo").unwrap();
        let headers: Vec<String> = vec!["Title".into(), "File".into()];
        let description = settings::description::DescriptionProfile::Rad.defaults();
        let plan = file_ingest::plan(folder.path(), &file_ingest::PlanOptions::default()).unwrap();
        let import = |policy| FolderImport {
            headers: &headers,
            title_column: Some("Title"),
            file_column: Some("File"),
            folder: folder.path().to_str().unwrap(),
            plan: &plan,
            description: &description,
            policy,
        };
        let sheet = || {
            vec![
                vec!["A".to_string(), "one.jpg".to_string()],
                vec!["B".to_string(), "one.jpg".to_string()],
            ]
        };

        // Neither row is picked: the file arrives as its own row, so nothing is claimed wrongly.
        let mut rows = sheet();
        let structure = append_folder_components(&import(DuplicatePolicy::Skip), &mut rows);
        assert_eq!(rows.len(), 4);
        assert_eq!(structure.last().unwrap().row_id, 4);

        // "Link the first row" takes the first in spreadsheet order and adds no row for the file.
        let mut rows = sheet();
        let structure = append_folder_components(&import(DuplicatePolicy::Update), &mut rows);
        assert_eq!(rows.len(), 3);
        assert_eq!(structure.last().unwrap().row_id, 1);
        assert_eq!(rows[0], ["A", "one.jpg"]);
    }

    #[test]
    fn blank_folder_tree_becomes_archival_component_rows() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("series")).unwrap();
        std::fs::write(folder.path().join("series").join("one.jpg"), "photo").unwrap();
        let headers = vec!["Title".into(), "File".into()];
        let mut rows = Vec::new();
        let description = settings::description::DescriptionProfile::Rad.defaults();
        let plan = file_ingest::plan(folder.path(), &file_ingest::PlanOptions::default()).unwrap();

        let structure = append_folder_components(
            &FolderImport {
                headers: &headers,
                title_column: Some("Title"),
                file_column: Some("File"),
                folder: folder.path().to_str().unwrap(),
                plan: &plan,
                description: &description,
                policy: DuplicatePolicy::Skip,
            },
            &mut rows,
        );

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], ["series", ""]);
        assert_eq!(rows[2], ["one.jpg", "series/one.jpg"]);
        assert_eq!(structure[1].parent_id, Some(1));
        assert_eq!(structure[2].parent_id, Some(2));
        assert_eq!(structure[1].level_key, "series");
        assert_eq!(structure[2].level_key, "item");
    }

    #[test]
    fn excluding_the_selected_root_starts_with_its_children() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("series")).unwrap();
        std::fs::write(folder.path().join("series").join("one.jpg"), "photo").unwrap();
        let headers = vec!["Title".into(), "File".into()];
        let description = settings::description::DescriptionProfile::Rad.defaults();
        let plan = file_ingest::plan(
            folder.path(),
            &file_ingest::PlanOptions {
                include_root: false,
                ..Default::default()
            },
        )
        .unwrap();
        let mut rows = Vec::new();

        let structure = append_folder_components(
            &FolderImport {
                headers: &headers,
                title_column: Some("Title"),
                file_column: Some("File"),
                folder: folder.path().to_str().unwrap(),
                plan: &plan,
                description: &description,
                policy: DuplicatePolicy::Skip,
            },
            &mut rows,
        );

        assert_eq!(rows, [["series", ""], ["one.jpg", "series/one.jpg"]]);
        assert_eq!(structure[0].parent_id, None);
        assert_eq!(structure[1].parent_id, Some(1));
    }
}

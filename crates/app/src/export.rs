//! File ▸ Export: ask where it goes, then hand the grid to `qrate_export`.
//!
//! Everything the writers need is read out of `cx` before any dialog opens, so the spawned task
//! only carries plain values. The formats themselves live in `qrate-export`; what's here is the
//! action, the save dialog, and the CSL field-mapping picker.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use gpui::{
    Action, AnyWindowHandle, App, AppContext as _, AsyncApp, ClickEvent, IntoElement,
    ParentElement, SharedString, Styled, Window,
};
use gpui_component::button::Button;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::notification::Notification;
use gpui_component::{Sizable as _, WindowExt as _, h_flex};
use qrate_export::export::{self, ArchiveFile, CSL_FIELDS, CslMapping, ExportComponent};
use qrate_export::{ProjectNote, SheetNote};
use schemars::JsonSchema;
use serde::Deserialize;
use settings::columns::ColumnType;
use settings::project::CurrentProject;

/// Where the CSL picker's answer is remembered, per project.
const CSL_MAPPING_KEY: &str = "csl_mapping";

struct ExportGrid {
    headers: Vec<String>,
    row_ids: Vec<settings::project::RowId>,
    rows: Vec<Vec<String>>,
    structure: Vec<ExportComponent>,
    /// Each column's note, for the formats that carry notes; the row notes are read from the
    /// project file off the UI thread.
    column_notes: Vec<(String, String)>,
    /// Where a ZIP finds each row's linked file. `None` for every other format.
    files: Option<ZipFiles>,
}

/// The files folder and the columns declared to name files, which a ZIP resolves against disk.
struct ZipFiles {
    folder: String,
    declared: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum ExportFormat {
    Csv,
    Xlsx,
    JsonLd,
    Csl,
    Zip,
    GoogleSheet,
    GoogleSheetSync,
}

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = this_app)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub format: ExportFormat,
}

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = this_app)]
#[serde(deny_unknown_fields)]
pub struct PluginExport {
    pub plugin: String,
    pub export: String,
}

const MAX_PLUGIN_EXPORT_BYTES: usize = 64 * 1024 * 1024;

/// Menu order. Each entry is the label and the extension of the file the save dialog offers, which
/// is named after the project; the Sheets target never touches disk, so it has no name to suggest.
pub const EXPORT_FORMATS: [(ExportFormat, &str, Option<&str>); 7] = [
    (ExportFormat::Csv, "CSV…", Some("csv")),
    (ExportFormat::Xlsx, "Excel (.xlsx)…", Some("xlsx")),
    (ExportFormat::JsonLd, "JSON-LD…", Some("jsonld")),
    (ExportFormat::Csl, "Zotero (CSL-JSON)…", Some("json")),
    (ExportFormat::Zip, "ZIP Archive…", Some("zip")),
    (ExportFormat::GoogleSheet, "New Google Sheet…", None),
    (ExportFormat::GoogleSheetSync, "Sync to Google Sheet…", None),
];

/// Whether a menu entry belongs to Google sync, which is hidden entirely until the user opts in.
pub fn is_google(format: ExportFormat) -> bool {
    matches!(
        format,
        ExportFormat::GoogleSheet | ExportFormat::GoogleSheetSync
    )
}

/// The open project's grid as the writers want it, with in-session edits — the same snapshot
/// `table::save_now` persists. `None` with no project or no live table.
fn grid(cx: &App) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let state = cx
        .try_global::<table::TableStateHandle>()
        .and_then(|h| h.0.upgrade())?;
    let (headers, _, rows) = state.read(cx).delegate().dataset_snapshot();
    Some((headers, rows))
}

/// Ask a plugin for JSON from a fixed live-table snapshot, then let the host save it.
///
/// Lua receives no path or file handle. Canceling the save dialog discards the snapshot before the
/// plugin runs, and dropping qrate cancels the host task.
pub fn run_plugin(action: &PluginExport, cx: &mut App) {
    let (Some(project), Some((headers, rows)), Some(plugin)) = (
        cx.try_global::<CurrentProject>(),
        grid(cx),
        plugin_host::exporter(&action.plugin, cx),
    ) else {
        log::warn!("plugin export was asked for with no project or plugin available");
        return;
    };
    let Some(spec) = plugin
        .exports()
        .iter()
        .find(|spec| spec.id.as_ref() == action.export)
        .cloned()
    else {
        log::warn!(
            "{} has no declared export {:?}",
            action.plugin,
            action.export
        );
        return;
    };
    let columns = headers
        .iter()
        .map(|name| {
            let data_type = project
                .data
                .columns
                .iter()
                .find(|column| &column.name == name)
                .map_or_else(|| "Text".to_string(), |column| column.data_type.clone());
            let column = settings::columns::get(name, cx);
            plugin_api::ExportColumn {
                name: name.clone().into(),
                data_type: data_type.into(),
                settings: column
                    .plugins
                    .get(&action.plugin)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            }
        })
        .collect();
    let snapshot = plugin_api::ExportSnapshot {
        title: project.display_name().into(),
        columns,
        rows: rows
            .into_iter()
            .map(|row| row.into_iter().map(SharedString::from).collect())
            .collect(),
    };
    let directory = project
        .file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let receiver = cx.prompt_for_new_path(&directory, Some(spec.suggested_name.as_ref()));
    let export_id = action.export.clone();
    let window = cx.active_window();

    cx.spawn(async move |cx| {
        let Ok(Ok(Some(path))) = receiver.await else {
            return;
        };
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let result = cx
            .background_spawn(async move {
                let value = plugin
                    .export(&export_id, &snapshot)
                    .map_err(anyhow::Error::msg)?;
                let mut bytes = serde_json::to_vec_pretty(&value)?;
                bytes.push(b'\n');
                anyhow::ensure!(
                    bytes.len() <= MAX_PLUGIN_EXPORT_BYTES,
                    "plugin export exceeds the {} MiB output limit",
                    MAX_PLUGIN_EXPORT_BYTES / 1024 / 1024
                );
                let parent = path
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("export path has no parent"))?;
                let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
                temporary.write_all(&bytes)?;
                temporary.as_file_mut().sync_all()?;
                temporary
                    .persist(&path)
                    .map_err(|error| error.error)
                    .map(|_| ())?;
                Ok::<(), anyhow::Error>(())
            })
            .await;
        let note = match result {
            Ok(()) => Notification::success(format!("Exported to {name}")),
            Err(error) => {
                log::error!("plugin export failed: {error:#}");
                Notification::error(format!("Could not export to {name}: {error:#}"))
            }
        };
        tell(window, note, cx);
    })
    .detach();
}

pub fn run(format: ExportFormat, window: &mut Window, cx: &mut App) {
    if is_google(format) && !settings::google_enabled(cx) {
        log::info!("Google Sheets export requested while the integration is disabled");
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Enable Google Sheets")
                .content(|content, _, _| {
                    content.child(
                        "Google Sheets is disabled. Enable it now to sign in and continue this export.",
                    )
                })
                .button_props(DialogButtonProps::default().ok_text("Enable and continue"))
                .on_ok(move |_: &ClickEvent, window, cx| {
                    log::info!("Google Sheets enabled from the Export menu");
                    settings::AppSettings::set_bool(settings::GOOGLE_SYNC_KEY, true, cx);
                    crate::app_menus::install(cx);
                    run(format, window, cx);
                    true
                })
        });
        return;
    }
    let (Some(project), Some(state)) = (
        cx.try_global::<CurrentProject>(),
        cx.try_global::<table::TableStateHandle>()
            .and_then(|handle| handle.0.upgrade()),
    ) else {
        log::warn!("export was asked for with no project open");
        return;
    };
    let (file, title) = (project.file.clone(), project.display_name());
    let (headers, row_ids, mut rows, structure) = {
        let delegate = state.read(cx).delegate();
        let (headers, row_ids, rows) = delegate.dataset_snapshot();
        let structure: Vec<ExportComponent> = delegate
            .row_structure()
            .iter()
            .cloned()
            .map(|item| ExportComponent {
                row_id: item.row_id,
                parent_id: item.parent_id,
                level_key: item.level_key,
                source_path: item.source_path,
            })
            .collect();
        (headers, row_ids, rows, structure)
    };
    let declared: Vec<_> = project
        .data
        .columns
        .iter()
        .map(|column| {
            (
                column.name.clone(),
                ColumnType::from_declared(&column.data_type),
            )
        })
        .collect();
    let description = settings::description::DescriptionConfig::from_values(&project.data.values);
    let levels: Vec<_> = description
        .levels
        .iter()
        .map(|level| (level.key.clone(), level.label.clone()))
        .collect();
    export::project_structure_columns(
        &headers, &row_ids, &mut rows, &structure, &declared, &levels,
    );
    let column_notes = project
        .data
        .columns
        .iter()
        .map(|column| (column.name.clone(), column.notes.clone()))
        .collect::<Vec<_>>();
    let files = (format == ExportFormat::Zip).then(|| ZipFiles {
        folder: project
            .data
            .values
            .get(settings::project::FILES_FOLDER_KEY)
            .map(|v| v.text().to_string())
            .unwrap_or_default(),
        declared: table::photos::declared_file_columns(&project.data),
    });
    let grid = ExportGrid {
        headers,
        row_ids,
        rows,
        structure,
        column_notes,
        files,
    };

    if is_google(format) {
        // A project that already knows its spreadsheet refills that one; otherwise "Sync" asks
        // Google's chooser, which is also what grants qrate access to the file.
        let linked = project
            .data
            .values
            .get(settings::project::GOOGLE_SHEET_ID_KEY)
            .map(|v| v.text().to_string())
            .filter(|id| !id.is_empty());
        let target = match (format, linked) {
            (ExportFormat::GoogleSheet, _) => SheetTarget::New,
            (_, Some(id)) => SheetTarget::Existing(id),
            (_, None) => SheetTarget::Choose,
        };
        return to_google_sheet(title, file, grid, target, window, cx);
    }
    if format == ExportFormat::Csl {
        return ask_csl_mapping(file, title, grid.headers, grid.rows, window, cx);
    }
    save_as(format, file, title, grid, CslMapping::new(), cx);
}

/// The project's notes as sheet cell notes. Reads the project file, so it runs off the UI thread.
fn sheet_notes(file: &Path, grid: &ExportGrid) -> anyhow::Result<Vec<SheetNote>> {
    let project_notes = settings::project::read_notes(file)?
        .into_iter()
        .map(|note| ProjectNote {
            dataset: note.dataset,
            row_id: note.row_id,
            column: note.column,
            severity: note.severity,
            message: note.message,
            created_at: note.created_at,
            author: note.author,
        })
        .collect::<Vec<_>>();
    Ok(qrate_export::sheet_notes(
        &grid.headers,
        &grid.row_ids,
        &grid.column_notes,
        &project_notes,
    ))
}

/// Each row's linked file, and where it sat in the source tree. Scans the files folder, so it runs
/// off the UI thread.
fn archive_files(grid: &ExportGrid, files: &ZipFiles) -> Vec<ArchiveFile> {
    let source_paths: HashMap<settings::project::RowId, &String> = grid
        .structure
        .iter()
        .filter_map(|component| Some((component.row_id, component.source_path.as_ref()?)))
        .collect();
    table::photos::resolve_row_images(&grid.headers, &grid.rows, &files.folder, &files.declared)
        .into_iter()
        .enumerate()
        .filter_map(|(index, path)| {
            let source_path = grid
                .row_ids
                .get(index)
                .and_then(|row_id| source_paths.get(row_id))
                .map(|source| (*source).clone());
            Some(ArchiveFile {
                path: path?,
                source_path,
            })
        })
        .collect()
}

/// How far a ZIP export has got, and the archivist's way to stop it.
#[derive(Default)]
struct Progress {
    written: AtomicU64,
    /// The bytes of linked files the archive will copy; zero until they have been found.
    total: AtomicU64,
    cancel: AtomicBool,
    done: AtomicBool,
}

/// The archive's file, counting what goes through it and refusing to go on once cancelled — the
/// writer is the one place the ZIP writer comes back to between (and within) the files it copies.
struct Metered<W> {
    inner: W,
    progress: Arc<Progress>,
}

impl<W: std::io::Write> std::io::Write for Metered<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.progress.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other(CANCELLED));
        }
        let written = self.inner.write(buf)?;
        self.progress
            .written
            .fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<W: std::io::Seek> std::io::Seek for Metered<W> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

const CANCELLED: &str = "export cancelled";

/// How often the ZIP progress notice is refreshed while the archive is written.
const PROGRESS_TICK: Duration = Duration::from_millis(250);

/// One export notice per window: the progress of one, then its outcome, replace each other.
struct ExportNotice;

/// Show `note` in the window the export started from. A window closed meanwhile has nobody left
/// to tell, and failures are logged besides.
fn tell(window: Option<AnyWindowHandle>, note: Notification, cx: &mut AsyncApp) {
    if let Some(window) = window {
        cx.update_window(window, |_, window, cx| {
            window.push_notification(note.id::<ExportNotice>(), cx)
        })
        .ok();
    }
}

/// `<project name>.<extension>`, with the characters no file system accepts replaced.
fn suggested_name(title: &str, extension: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let stem = stem.trim().trim_end_matches('.');
    match stem.is_empty() {
        true => format!("export.{extension}"),
        false => format!("{stem}.{extension}"),
    }
}

/// Ask where it goes, then write it off the UI thread, and say how it went. The ZIP copies every
/// linked file, so it alone reports progress and can be cancelled.
fn save_as(
    format: ExportFormat,
    project_file: PathBuf,
    title: String,
    grid: ExportGrid,
    mapping: CslMapping,
    cx: &mut App,
) {
    let window = cx.active_window();
    let directory = project_file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let suggested = EXPORT_FORMATS
        .iter()
        .find(|(f, _, _)| *f == format)
        .and_then(|(_, _, extension)| *extension)
        .map(|extension| suggested_name(&title, extension));
    let receiver = cx.prompt_for_new_path(&directory, suggested.as_deref());

    cx.spawn(async move |cx| {
        let Ok(Ok(Some(path))) = receiver.await else {
            return;
        };
        let rows = grid.rows.len();
        let progress = Arc::new(Progress::default());
        let task = cx.background_spawn({
            let (path, progress) = (path.clone(), progress.clone());
            async move {
                let result = (|| -> anyhow::Result<()> {
                    let parent = path.parent().unwrap_or_else(|| Path::new("."));
                    let temporary = tempfile::Builder::new()
                        .prefix(".qrate-export-")
                        .tempfile_in(parent)?
                        .into_temp_path();
                    write_export(
                        format,
                        &project_file,
                        &temporary,
                        &grid,
                        &mapping,
                        &progress,
                    )?;
                    if progress.cancel.load(Ordering::Relaxed) {
                        anyhow::bail!(CANCELLED);
                    }
                    temporary.persist(&path)?;
                    Ok(())
                })();
                progress.done.store(true, Ordering::Release);
                result
            }
        });

        if format == ExportFormat::Zip {
            let cancel_note = |label: String| {
                let progress = progress.clone();
                Notification::new().message(label).action(move |_, _, _| {
                    let progress = progress.clone();
                    Button::new("cancel-export")
                        .label("Cancel")
                        .on_click(move |_, _, _| progress.cancel.store(true, Ordering::Relaxed))
                })
            };
            let mut shown = None;
            while !progress.done.load(Ordering::Acquire) {
                let total = progress.total.load(Ordering::Relaxed);
                let percent = (total > 0)
                    .then(|| (progress.written.load(Ordering::Relaxed) * 100 / total).min(99));
                if shown != Some(percent) {
                    shown = Some(percent);
                    let label = match percent {
                        Some(percent) => format!("Exporting the archive… {percent}%"),
                        None => "Exporting the archive…".to_string(),
                    };
                    tell(window, cancel_note(label), cx);
                }
                cx.background_executor().timer(PROGRESS_TICK).await;
            }
        }

        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let note = match task.await {
            Ok(()) => Notification::success(format!(
                "Exported {rows} row{} to {name}",
                if rows == 1 { "" } else { "s" }
            )),
            Err(_) if progress.cancel.load(Ordering::Relaxed) => {
                Notification::info("Export cancelled")
            }
            Err(err) => {
                log::error!("could not export to {}: {err:#}", path.display());
                Notification::error(format!("Could not export to {name}: {err:#}"))
            }
        };
        tell(window, note, cx);
    })
    .detach();
}

/// Write `grid` to `path` in `format`. Runs on the background executor.
fn write_export(
    format: ExportFormat,
    project_file: &Path,
    path: &Path,
    grid: &ExportGrid,
    mapping: &CslMapping,
    progress: &Arc<Progress>,
) -> anyhow::Result<()> {
    let ExportGrid {
        headers,
        row_ids,
        rows,
        structure,
        ..
    } = grid;
    match format {
        ExportFormat::Csv => export::write_csv(path, headers, rows)?,
        ExportFormat::Xlsx => {
            export::write_xlsx(path, headers, rows, &sheet_notes(project_file, grid)?)?
        }
        ExportFormat::JsonLd => export::write_json(
            path,
            &export::jsonld_hierarchy_value(headers, row_ids, rows, structure),
        )?,
        ExportFormat::Csl => export::write_json(path, &export::csl_items(headers, rows, mapping))?,
        ExportFormat::Zip => {
            let images = grid
                .files
                .as_ref()
                .map(|files| archive_files(grid, files))
                .unwrap_or_default();
            let total: u64 = images
                .iter()
                .filter_map(|image| std::fs::metadata(&image.path).ok())
                .map(|meta| meta.len())
                .sum();
            progress.total.store(total.max(1), Ordering::Relaxed);
            let file = Metered {
                inner: BufWriter::new(File::create(path)?),
                progress: progress.clone(),
            };
            export::zip_to(file, headers, row_ids, rows, structure, &images)?
        }
        // Handled in `run` — they have no path to write to.
        ExportFormat::GoogleSheet | ExportFormat::GoogleSheetSync => {}
    }
    Ok(())
}

/// Which column feeds which CSL field. Opens on the saved answer, or on what the declared column
/// types imply — and always opens, so a guess is something the user sees rather than inherits.
fn ask_csl_mapping(
    project_file: PathBuf,
    title: String,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    window: &mut Window,
    cx: &mut App,
) {
    let declared: Vec<(String, String)> = cx
        .try_global::<CurrentProject>()
        .map(|project| {
            headers
                .iter()
                .map(|name| {
                    let kind = project
                        .data
                        .columns
                        .iter()
                        .find(|c| &c.name == name)
                        .map(|c| c.data_type.clone())
                        .unwrap_or_default();
                    (name.clone(), kind)
                })
                .collect()
        })
        .unwrap_or_default();
    let saved = settings::project::read_setting(&project_file, CSL_MAPPING_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<CslMapping>(&raw).ok());
    // Shared with the dialog's buttons: the content builder re-runs on every frame, so the picked
    // mapping cannot live inside it.
    let picked = Rc::new(RefCell::new(
        saved.unwrap_or_else(|| export::derive_csl_mapping(&declared)),
    ));

    let (for_content, for_ok) = (picked.clone(), picked.clone());
    let columns: Vec<SharedString> = headers.iter().map(SharedString::from).collect();
    window.open_dialog(cx, move |dialog, _, _| {
        let (mapping, columns) = (for_content.clone(), columns.clone());
        let (project_file, title, headers, rows) = (
            project_file.clone(),
            title.clone(),
            headers.clone(),
            rows.clone(),
        );
        let for_ok = for_ok.clone();
        dialog
            .title("Export for Zotero")
            .w(gpui::px(420.0))
            .content(move |content, _, _| {
                let (mapping, columns) = (mapping.clone(), columns.clone());
                content.p_4().gap_2().children(CSL_FIELDS.map(|field| {
                    let (mapping, columns) = (mapping.clone(), columns.clone());
                    let chosen: Option<SharedString> =
                        mapping.borrow().get(field).map(SharedString::from);
                    h_flex()
                        .justify_between()
                        .child(field)
                        .child(
                            Button::new(SharedString::from(format!("csl-{field}")))
                                .outline()
                                .small()
                                .label(chosen.unwrap_or_else(|| "— skip —".into()))
                                .dropdown_menu(move |menu, _, _| {
                                    let (mapping, columns) = (mapping.clone(), columns.clone());
                                    let pick = |menu: PopupMenu,
                                                label: SharedString,
                                                column: Option<SharedString>,
                                                mapping: &Rc<RefCell<CslMapping>>| {
                                        let mapping = mapping.clone();
                                        menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, window, _| {
                                                let mut m = mapping.borrow_mut();
                                                match &column {
                                                    Some(c) => m.insert(field.into(), c.to_string()),
                                                    None => m.remove(field),
                                                };
                                                window.refresh();
                                            },
                                        ))
                                    };
                                    let menu =
                                        pick(menu, "— skip —".into(), None, &mapping).separator();
                                    columns.iter().fold(menu, |menu, column| {
                                        pick(menu, column.clone(), Some(column.clone()), &mapping)
                                    })
                                }),
                        )
                        .into_any_element()
                }))
            })
            .button_props(DialogButtonProps::default().ok_text("Export…"))
            .on_ok(move |_: &ClickEvent, _, cx| {
                let mapping = for_ok.borrow().clone();
                if let Ok(raw) = serde_json::to_string(&mapping)
                    && let Err(err) =
                        settings::project::write_setting(&project_file, CSL_MAPPING_KEY, &raw)
                {
                    log::warn!("could not remember the Zotero field mapping: {err}");
                }
                save_as(
                    ExportFormat::Csl,
                    project_file.clone(),
                    title.clone(),
                    ExportGrid {
                        headers: headers.clone(),
                        row_ids: Vec::new(),
                        rows: rows.clone(),
                        structure: Vec::new(),
                        column_notes: Vec::new(),
                        files: None,
                    },
                    mapping,
                    cx,
                );
                true
            })
    });
}

/// Which spreadsheet a sync writes to.
enum SheetTarget {
    New,
    /// The one this project is already linked to.
    Existing(String),
    /// Ask Google's chooser. Picking a file there is what grants `drive.file` access to it — an
    /// id the user typed would name a file the token cannot reach.
    Choose,
}

/// Sign in if we have to, settle on a spreadsheet, fill it. Every step blocks, so the whole thing
/// runs on the background executor; only opening a URL and writing settings back come to the main
/// thread.
fn to_google_sheet(
    title: String,
    project_file: PathBuf,
    grid: ExportGrid,
    target: SheetTarget,
    window: &mut Window,
    cx: &mut App,
) {
    let stored = crate::google::stored(cx);
    let refresh_token = crate::google::refresh_token();
    let window = Some(window.window_handle());

    cx.spawn(async move |cx| {
        let Some(token) = resolve_token(stored, refresh_token, cx).await else {
            return;
        };

        let chosen = match target {
            SheetTarget::New => None,
            SheetTarget::Existing(id) => Some(id),
            SheetTarget::Choose => match choose_sheet(&token.access, cx).await {
                Some(id) => Some(id),
                None => return,
            },
        };

        if let Some(id) = &chosen {
            let (access, id) = (token.access.clone(), id.clone());
            let name = cx
                .background_spawn(async move {
                    data_exchange::google::sheet_title(&access, &id)
                })
                .await
                .unwrap_or_else(|err| {
                    log::warn!("could not read the Google spreadsheet's title: {err}");
                    "this spreadsheet".to_string()
                });
            let answer = window.and_then(|window| {
                cx.update_window(window, |_, window, cx| {
                    window.prompt(
                        gpui::PromptLevel::Warning,
                        &format!("Replace the contents of {name}?"),
                        Some(&format!(
                            "This replaces everything in {name}'s first tab with this project's rows."
                        )),
                        &["Replace", "Cancel"],
                        cx,
                    )
                })
                .ok()
            });
            let Some(answer) = answer else {
                return;
            };
            if answer.await != Ok(0) {
                return;
            }
        }

        let rows = grid.rows.len();
        let sheet = cx
            .background_spawn(async move {
                let notes = sheet_notes(&project_file, &grid)?;
                let fill = |id: &str, existing: bool| {
                    fill_google_sheet(&token.access, id, &grid, &notes, existing)
                };
                anyhow::Ok(match chosen {
                    Some(id) => fill(&id, true).map(|()| (id, None))?,
                    None => data_exchange::google::create_sheet(
                        &token.access,
                        &format!("{title} — qrate"),
                    )
                    .and_then(|(id, url)| fill(&id, false).map(|()| (id, Some(url))))?,
                })
            })
            .await;
        let note = match sheet {
            // A new sheet is worth opening; one the user already had is not — they went looking
            // for their data to be current, not for another browser tab.
            Ok((id, url)) => {
                cx.update(|cx| {
                    settings::project::CurrentProject::set_text(
                        settings::project::GOOGLE_SHEET_ID_KEY,
                        id.into(),
                        cx,
                    );
                    if let Some(url) = url {
                        cx.open_url(&url);
                    }
                });
                Notification::success(format!(
                    "Exported {rows} row{} to Google Sheets",
                    if rows == 1 { "" } else { "s" }
                ))
            }
            Err(err) => {
                log::error!("Google sync could not write the spreadsheet: {err:#}");
                Notification::error(format!("Could not write the Google spreadsheet: {err:#}"))
            }
        };
        tell(window, note, cx);
    })
    .detach();
}

/// Replace the first tab's values, then its notes. An `existing` sheet may carry notes from an
/// earlier sync on cells that no longer hold what they described, so those are cleared first.
fn fill_google_sheet(
    token: &str,
    id: &str,
    grid: &ExportGrid,
    notes: &[SheetNote],
    existing: bool,
) -> Result<(), data_exchange::google::GoogleError> {
    data_exchange::google::write_values(token, id, &grid.headers, &grid.rows)?;
    if notes.is_empty() && !existing {
        return Ok(());
    }
    let sheet_id = data_exchange::google::first_tab_id(token, id)?;
    if existing {
        let clear = serde_json::json!({ "requests": [{ "updateCells": {
            "range": { "sheetId": sheet_id },
            "fields": "note",
        }}]});
        data_exchange::google::batch_update(token, id, &clear)?;
    }
    if notes.is_empty() {
        return Ok(());
    }
    let body = qrate_export::sheet_note_request_body(sheet_id, notes);
    data_exchange::google::batch_update(token, id, &body)
}

/// Start Google authentication from Settings without requiring an export as the trigger.
pub fn authenticate(cx: &mut App) {
    log::info!("Google sign-in requested from Settings");
    let stored = crate::google::stored(cx);
    let refresh_token = crate::google::refresh_token();
    cx.spawn(async move |cx| {
        if resolve_token(stored, refresh_token, cx).await.is_some() {
            log::info!("Google sign-in completed successfully");
        }
    })
    .detach();
}

/// Authenticate, open Google's chooser, and remember the chosen destination in this project.
pub fn choose_sync_destination(cx: &mut App) {
    let Some(project_file) = cx
        .try_global::<CurrentProject>()
        .map(|project| project.file.clone())
    else {
        return log::warn!("Google sync destination requested with no project open");
    };
    log::info!("Google sync destination chooser requested");
    let stored = crate::google::stored(cx);
    let refresh_token = crate::google::refresh_token();
    cx.spawn(async move |cx| {
        let Some(token) = resolve_token(stored, refresh_token, cx).await else {
            return;
        };
        let Some(id) = choose_sheet(&token.access, cx).await else {
            return;
        };
        cx.update(|cx| {
            let still_open = cx
                .try_global::<CurrentProject>()
                .is_some_and(|project| project.file == project_file);
            if !still_open {
                return log::warn!(
                    "the project changed while choosing a Google sync destination; ignoring the selection"
                );
            }
            CurrentProject::set_text(
                settings::project::GOOGLE_SHEET_ID_KEY,
                id.clone().into(),
                cx,
            );
            log::info!("Google sync destination updated to spreadsheet {id}");
        });
    })
    .detach();
}

pub fn clear_sync_destination(cx: &mut App) {
    if cx.has_global::<CurrentProject>() {
        CurrentProject::set_text(
            settings::project::GOOGLE_SHEET_ID_KEY,
            SharedString::default(),
            cx,
        );
        log::info!("Google sync destination cleared");
    }
}

async fn resolve_token(
    stored: crate::google::Stored,
    refresh_token: Option<String>,
    cx: &mut gpui::AsyncApp,
) -> Option<data_exchange::google::Token> {
    log::info!("resolving Google OAuth client credentials");
    let (creds, persist) = cx.background_spawn(async move { stored.refreshed() }).await;
    let Some(creds) = creds else {
        log::error!(
            "Google sync has no client credentials — this build has none compiled in and the \
             credential endpoint could not be reached"
        );
        return None;
    };
    if let Some((creds, etag)) = persist {
        cx.update(|cx| crate::google::remember(&creds, etag, cx));
        log::info!("updated cached Google OAuth client credentials");
    }
    let token = sign_in(&creds, refresh_token, cx).await;
    if token.is_some() {
        cx.update(|cx| crate::google::set_authenticated(true, cx));
    }
    token
}

/// Hand the user Google's own file chooser and wait for what they pick. Same two-step shape as
/// consent, and for the same reason: the loopback port is bound before the page opens so the
/// answer cannot arrive before we are listening.
async fn choose_sheet(access_token: &str, cx: &mut gpui::AsyncApp) -> Option<String> {
    log::info!("opening the Google spreadsheet chooser");
    let picker = data_exchange::google::begin_picker(&crate::site::url("/picker"), access_token);
    let picker = match picker {
        Ok(picker) => picker,
        Err(err) => {
            log::error!("Google sync could not open the spreadsheet chooser: {err}");
            return None;
        }
    };
    cx.update(|cx| cx.open_url(&picker.url));
    match cx
        .background_spawn(async move { picker.wait_for_file_id() })
        .await
    {
        Ok(id) => {
            log::info!("Google spreadsheet chooser returned a selection");
            Some(id)
        }
        Err(err) => {
            log::info!("no spreadsheet was chosen: {err}");
            None
        }
    }
}

/// A stored grant, else a fresh consent. Opening the browser has to happen on the main thread and
/// the listener has to be bound before it does, so those two steps are interleaved here rather
/// than run as one background job.
async fn sign_in(
    creds: &data_exchange::google::ClientCreds,
    refresh_token: Option<String>,
    cx: &mut gpui::AsyncApp,
) -> Option<data_exchange::google::Token> {
    if let Some(stored) = refresh_token {
        log::info!("refreshing the stored Google sign-in");
        let creds = creds.clone();
        let token = cx
            .background_spawn(async move { data_exchange::google::refresh(&creds, &stored) })
            .await;
        match token {
            Ok(token) => {
                log::info!("stored Google sign-in refreshed successfully");
                return Some(token);
            }
            // An expired or revoked grant is not an error the user should see — it means consent
            // again, which is what falls through below.
            Err(err) => {
                log::info!("the stored Google sign-in no longer works, asking again: {err}")
            }
        }
    }

    // Bound before the browser opens, so Google's redirect can't arrive before we're listening.
    log::info!("starting Google browser consent");
    let consent = cx.update(|cx| match data_exchange::google::begin_consent(creds) {
        Ok(consent) => {
            cx.open_url(&consent.url);
            Some(consent)
        }
        Err(err) => {
            log::error!("Google sync could not start sign-in: {err}");
            None
        }
    })?;

    match cx
        .background_spawn(async move { consent.wait_for_token() })
        .await
    {
        Ok(token) => {
            if let Some(refresh) = &token.refresh {
                crate::google::set_refresh_token(refresh);
            }
            log::info!("Google browser consent completed successfully");
            Some(token)
        }
        Err(err) => {
            log::error!("Google sync could not sign in: {err}");
            None
        }
    }
}

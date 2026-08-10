//! File ▸ Export: ask where it goes, then hand the grid to `data_exchange::export`.
//!
//! Everything the writers need is read out of `cx` before any dialog opens, so the spawned task
//! only carries plain values. The formats themselves live in `data-exchange`; what's here is the
//! action, the save dialog, and the CSL field-mapping picker.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use data_exchange::export::{self, CSL_FIELDS, CslMapping};
use gpui::{
    Action, App, AppContext as _, ClickEvent, IntoElement, ParentElement, SharedString, Styled,
    Window,
};
use gpui_component::button::Button;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::{Sizable as _, WindowExt as _, h_flex};
use schemars::JsonSchema;
use serde::Deserialize;
use settings::columns::ColumnType;
use settings::project::CurrentProject;

/// Where the CSL picker's answer is remembered, per project.
const CSL_MAPPING_KEY: &str = "csl_mapping";

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum ExportFormat {
    Csv,
    JsonLd,
    Csl,
    Zip,
    GoogleSheet,
}

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = this_app)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub format: ExportFormat,
}

/// Menu order. Each entry is the label and the filename the save dialog offers; the Sheets target
/// never touches disk, so it has no name to suggest.
pub const EXPORT_FORMATS: [(ExportFormat, &str, Option<&str>); 5] = [
    (ExportFormat::Csv, "CSV…", Some("export.csv")),
    (ExportFormat::JsonLd, "JSON-LD…", Some("export.jsonld")),
    (ExportFormat::Csl, "Zotero (CSL-JSON)…", Some("export.json")),
    (ExportFormat::Zip, "ZIP Archive…", Some("export.zip")),
    (ExportFormat::GoogleSheet, "New Google Sheet…", None),
];

/// The open project's grid as the writers want it, with in-session edits — the same snapshot
/// `table::save_now` persists. `None` with no project or no live table.
fn grid(cx: &App) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let state = cx
        .try_global::<table::TableStateHandle>()
        .and_then(|h| h.0.upgrade())?;
    Some(state.read(cx).delegate().dataset_snapshot())
}

pub fn run(format: ExportFormat, window: &mut Window, cx: &mut App) {
    let (Some(project), Some((headers, rows))) = (cx.try_global::<CurrentProject>(), grid(cx))
    else {
        log::warn!("export was asked for with no project open");
        return;
    };
    let (file, title) = (project.file.clone(), project.display_name());

    if format == ExportFormat::GoogleSheet {
        return to_google_sheet(title, headers, rows, window, cx);
    }
    if format == ExportFormat::Csl {
        return ask_csl_mapping(file, headers, rows, window, cx);
    }

    let images = if format == ExportFormat::Zip {
        let folder = cx
            .try_global::<CurrentProject>()
            .and_then(|p| p.data.values.get(settings::project::FILES_FOLDER_KEY))
            .map(|v| v.text().to_string())
            .unwrap_or_default();
        table::photos::resolve_row_images(&headers, &rows, &folder)
            .into_iter()
            .flatten()
            .collect()
    } else {
        Vec::new()
    };

    save_as(format, file, headers, rows, images, CslMapping::new(), cx);
}

/// Ask where it goes, then write it off the UI thread. The ZIP copies image bytes and the others
/// are a single small file, but they all wait on the same dialog, so they all go to the executor.
fn save_as(
    format: ExportFormat,
    project_file: PathBuf,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    images: Vec<PathBuf>,
    mapping: CslMapping,
    cx: &mut App,
) {
    let directory = project_file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let suggested = EXPORT_FORMATS
        .iter()
        .find(|(f, _, _)| *f == format)
        .and_then(|(_, _, name)| *name);
    let receiver = cx.prompt_for_new_path(&directory, suggested);

    cx.background_spawn(async move {
        let Ok(Ok(Some(path))) = receiver.await else {
            return;
        };
        let result = match format {
            ExportFormat::Csv => export::write_csv(&path, &headers, &rows),
            ExportFormat::JsonLd => {
                export::write_json(&path, &export::jsonld_value(&headers, &rows))
            }
            ExportFormat::Csl => {
                export::write_json(&path, &export::csl_items(&headers, &rows, &mapping))
            }
            ExportFormat::Zip => export::write_zip(&path, &headers, &rows, &images),
            // Handled in `run` — it has no path to write to.
            ExportFormat::GoogleSheet => return,
        };
        if let Err(err) = result {
            log::error!("could not export to {}: {err}", path.display());
        }
    })
    .detach();
}

/// Which column feeds which CSL field. Opens on the saved answer, or on what the declared column
/// types imply — and always opens, so a guess is something the user sees rather than inherits.
fn ask_csl_mapping(
    project_file: PathBuf,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    window: &mut Window,
    cx: &mut App,
) {
    let declared: Vec<(String, ColumnType)> = cx
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
                        .map(|c| ColumnType::from_declared(&c.data_type))
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
        let (project_file, headers, rows) =
            (project_file.clone(), headers.clone(), rows.clone());
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
                    headers.clone(),
                    rows.clone(),
                    Vec::new(),
                    mapping,
                    cx,
                );
                true
            })
    });
}

/// Where the Google refresh token lives once the user has consented — user-wide, so signing in
/// once covers every project.
const GOOGLE_REFRESH_KEY: &str = "google_refresh_token";

/// Sign in if we have to, create the sheet, open it. Every step blocks, so the whole thing runs on
/// the background executor; only opening the consent URL and the finished sheet come back to the
/// main thread.
fn to_google_sheet(
    title: String,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    _window: &mut Window,
    cx: &mut App,
) {
    let stored = settings::AppSettings::get(cx)
        .values
        .get(GOOGLE_REFRESH_KEY)
        .map(|v| v.text().to_string());

    // Bound before the browser opens, so Google's redirect can't arrive before we're listening.
    let consent = match stored.is_none().then(data_exchange::google::begin_consent) {
        Some(Ok(consent)) => {
            cx.open_url(&consent.url);
            Some(consent)
        }
        Some(Err(err)) => return log::error!("Google export could not start sign-in: {err}"),
        None => None,
    };

    cx.spawn(async move |cx| {
        let token = cx
            .background_spawn(async move {
                match consent {
                    Some(consent) => consent.wait_for_token(),
                    None => data_exchange::google::refresh(&stored.unwrap_or_default()),
                }
            })
            .await;
        let token = match token {
            Ok(token) => token,
            Err(err) => return log::error!("Google export could not sign in: {err}"),
        };
        // Only the first consent issues one; a refresh reuses what is already stored.
        if let Some(refresh) = token.refresh.clone() {
            cx.update(|cx| settings::AppSettings::set_text(GOOGLE_REFRESH_KEY, refresh.into(), cx));
        }

        let created = cx
            .background_spawn(async move {
                data_exchange::google::create_sheet(
                    &token.access,
                    &format!("{title} — qrate"),
                    &headers,
                    &rows,
                )
            })
            .await;
        match created {
            Ok(url) => {
                cx.update(|cx| cx.open_url(&url));
            }
            Err(err) => log::error!("Google export could not create the sheet: {err}"),
        }
    })
    .detach();
}

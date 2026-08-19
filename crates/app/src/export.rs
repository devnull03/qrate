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
    GoogleSheetSync,
}

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = this_app)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub format: ExportFormat,
}

/// Menu order. Each entry is the label and the filename the save dialog offers; the Sheets target
/// never touches disk, so it has no name to suggest.
pub const EXPORT_FORMATS: [(ExportFormat, &str, Option<&str>); 6] = [
    (ExportFormat::Csv, "CSV…", Some("export.csv")),
    (ExportFormat::JsonLd, "JSON-LD…", Some("export.jsonld")),
    (ExportFormat::Csl, "Zotero (CSL-JSON)…", Some("export.json")),
    (ExportFormat::Zip, "ZIP Archive…", Some("export.zip")),
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
    let (Some(project), Some((headers, rows))) = (cx.try_global::<CurrentProject>(), grid(cx))
    else {
        log::warn!("export was asked for with no project open");
        return;
    };
    let (file, title) = (project.file.clone(), project.display_name());

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
        return to_google_sheet(title, headers, rows, target, window, cx);
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
            // Handled in `run` — they have no path to write to.
            ExportFormat::GoogleSheet | ExportFormat::GoogleSheetSync => return,
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
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    target: SheetTarget,
    _window: &mut Window,
    cx: &mut App,
) {
    let stored = crate::google::stored(cx);
    let refresh_token = crate::google::refresh_token();

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

        let sheet = cx
            .background_spawn(async move {
                match chosen {
                    Some(id) => {
                        data_exchange::google::write_values(&token.access, &id, &headers, &rows)
                            .map(|()| (id, None))
                    }
                    None => data_exchange::google::create_sheet(
                        &token.access,
                        &format!("{title} — qrate"),
                    )
                    .and_then(|(id, url)| {
                        data_exchange::google::write_values(&token.access, &id, &headers, &rows)
                            .map(|()| (id, Some(url)))
                    }),
                }
            })
            .await;
        match sheet {
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
            }
            Err(err) => log::error!("Google sync could not write the spreadsheet: {err}"),
        }
    })
    .detach();
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
    let picker = data_exchange::google::begin_picker(
        data_exchange::google::DEFAULT_PICKER_PAGE,
        access_token,
    );
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

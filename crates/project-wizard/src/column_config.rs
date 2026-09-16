//! Loading a column config — a file or a Google Sheet listing each column's type, description and
//! checks. One view for both places it happens: the wizard's Columns step, before a project exists,
//! and an open project, where it re-types the columns already there.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::Button;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputState};
use gpui_component::label::Label;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme, WindowExt, h_flex, v_flex};

use plugin_api::{ColumnMapContributions, ColumnMapSpec};
use settings::columns::ColumnSettings;
use settings::project::CurrentProject;

use crate::data::{self, ColumnConfigEntry, ColumnConfigPreview};
use crate::steps::files::{MsgKind, inline_message};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LoadConfigTab {
    File,
    Sheet,
}

/// The source picker, what it loaded, and how that lines up with `headers`. It renders itself, so
/// a dialog can hold it as a child and never needs rebuilding when a click changes what it shows.
pub struct ColumnConfigLoader {
    pub(crate) headers: Vec<String>,
    tab: LoadConfigTab,
    file_path: String,
    sheet_link: Entity<InputState>,
    pub(crate) preview: Option<ColumnConfigPreview>,
    error: Option<SharedString>,
}

impl ColumnConfigLoader {
    pub fn new(headers: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            headers,
            tab: LoadConfigTab::File,
            file_path: String::new(),
            sheet_link: cx.new(|cx| {
                InputState::new(window, cx).placeholder("docs.google.com/spreadsheets/d/…")
            }),
            preview: None,
            error: None,
        }
    }

    fn loaded(&mut self, result: Result<ColumnConfigPreview, String>) {
        (self.preview, self.error) = match result {
            Ok(preview) => (Some(preview), None),
            Err(message) => (None, Some(message.into())),
        };
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a column config file".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let path = path.to_string_lossy().to_string();
                this.update(cx, |this, cx| {
                    let result = data::load_column_config(&path, &this.headers);
                    this.file_path = path;
                    this.loaded(result.map_err(|error| error.message()));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// A Sheet's headers become the config, one Text column each.
    // move to the background executor if it ever drags.
    fn check_sheet(&mut self, cx: &mut Context<Self>) {
        let link = self.sheet_link.read(cx).value().to_string();
        let result = data_exchange::fetch_sheet(&link)
            .map(data_exchange::SpreadsheetPreview::from)
            .map(|preview| ColumnConfigPreview {
                entries: preview
                    .headers
                    .into_iter()
                    .map(|name| ColumnConfigEntry {
                        name,
                        data_type: "Text".into(),
                        ..Default::default()
                    })
                    .collect(),
            })
            .map_err(|error| error.to_string());
        self.loaded(result);
        cx.notify();
    }
}

impl Render for ColumnConfigLoader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = TabBar::new("load-config-tabs")
            .segmented()
            .selected_index(match self.tab {
                LoadConfigTab::File => 0,
                LoadConfigTab::Sheet => 1,
            })
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.tab = match ix {
                    0 => LoadConfigTab::File,
                    _ => LoadConfigTab::Sheet,
                };
                cx.notify();
            }))
            .child(Tab::new().label("From file"))
            .child(Tab::new().label("From Google Sheet"));

        let body = match self.tab {
            LoadConfigTab::File => v_flex()
                .gap_1()
                .child(Label::new("Config file").text_sm())
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .px_2()
                                .py_1p5()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(match self.file_path.is_empty() {
                                    true => "Choose a file…".to_string(),
                                    false => self.file_path.clone(),
                                }),
                        )
                        .child(
                            Button::new("browse-config")
                                .label("Browse…")
                                .outline()
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.browse(window, cx)),
                                ),
                        ),
                ),
            LoadConfigTab::Sheet => v_flex()
                .gap_1()
                .child(Label::new("Sheet link").text_sm())
                .child(
                    h_flex()
                        .gap_2()
                        .child(Input::new(&self.sheet_link).flex_1())
                        .child(
                            Button::new("check-config-sheet")
                                .label("Check")
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| this.check_sheet(cx))),
                        ),
                ),
        };

        v_flex()
            .gap_3()
            .child(tabs)
            .child(body)
            .map(|el| match (&self.preview, &self.error) {
                (Some(preview), _) => el.child(inline_message(
                    "config-status",
                    format!("Loaded {} columns", preview.entries.len()),
                    MsgKind::Success,
                )),
                (None, Some(error)) => el.child(inline_message(
                    "config-status",
                    error.clone(),
                    MsgKind::Error,
                )),
                (None, None) => el,
            })
            .when(self.preview.is_some(), |el| {
                el.child(mapping(&self.headers, self.preview.as_ref(), cx))
            })
    }
}

/// Each column beside the config entry it takes its type from, with the ones the config says
/// nothing about marked.
pub(crate) fn mapping(
    headers: &[String],
    config: Option<&ColumnConfigPreview>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(
            Label::new("Column → qrate field")
                .text_sm()
                .text_color(cx.theme().muted_foreground),
        )
        .when(headers.is_empty(), |table| {
            table.child(
                Label::new("No columns yet.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
        })
        .children(headers.iter().map(|header| {
            let target =
                config
                    .and_then(|config| entry_for(config, header))
                    .map(|entry| match entry.data_type.is_empty() {
                        true => entry.name.clone(),
                        false => format!("{} ({})", entry.name, entry.data_type),
                    });
            h_flex()
                .justify_between()
                .text_sm()
                .py_1()
                .border_b_1()
                .border_color(cx.theme().border)
                // The warning colour already says "unmapped" — a trailing glyph saying it
                // again depends on font coverage for nothing.
                .when(target.is_none(), |el| el.text_color(cx.theme().warning))
                .child(header.clone())
                .child(format!("→ {}", target.as_deref().unwrap_or("Unmapped")))
        }))
        .into_any_element()
}

pub(crate) fn entry_for<'a>(
    config: &'a ColumnConfigPreview,
    header: &str,
) -> Option<&'a ColumnConfigEntry> {
    config
        .entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(header))
}

/// Lay what `entry` says over `settings`. Only what the file states is written, so a config with no
/// Spellcheck column leaves each column's spell-checking where the archivist put it.
pub(crate) fn apply_entry(
    entry: &ColumnConfigEntry,
    maps: &[(SharedString, ColumnMapSpec)],
    settings: &mut ColumnSettings,
) {
    if entry.authority.is_some() {
        settings.authority = entry.authority.clone();
    }
    if let Some(on) = entry.spellcheck {
        settings.spellcheck = on;
    }
    if let Some(on) = entry.variant_review {
        settings.variant_review = on;
    }
    // Severity is per producer: the authority names its own, and each plugin's rides in the
    // `<id>::Severity` column beside its mapping.
    if let (Some(authority), Some(severity)) =
        (entry.authority.clone(), entry.authority_severity.clone())
    {
        settings.severity.insert(authority, severity);
    }
    for (plugin, spec) in maps {
        if let Some(cell) = entry.extra.get(&format!("{plugin}::{}", spec.label)) {
            let chosen: Vec<SharedString> = cell
                .split(';')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string().into())
                .collect();
            ColumnMapContributions::put(plugin, spec, settings, &chosen);
        }
        if let Some(severity) = entry.extra.get(&format!("{plugin}::Severity")) {
            settings
                .severity
                .insert(plugin.to_string(), severity.to_ascii_lowercase());
        }
    }
}

/// Load a column config onto the open project: every column the config names takes its type,
/// description and checks. Columns it doesn't name are left exactly as they are.
pub fn open_column_config_dialog(window: &mut Window, cx: &mut App) {
    let Some(headers) = cx
        .try_global::<CurrentProject>()
        .map(|project| project.data.headers.clone())
    else {
        return;
    };
    let loader = cx.new(|cx| ColumnConfigLoader::new(headers, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        let apply_from = loader.clone();
        dialog
            .title("Load column config")
            .child(loader.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Apply")
                    .show_cancel(true),
            )
            .on_ok(move |_, _, cx| {
                let Some(config) = apply_from.read(cx).preview.clone() else {
                    return false;
                };
                apply(&config, cx);
                true
            })
    });
}

fn apply(config: &ColumnConfigPreview, cx: &mut App) {
    let headers = cx.global::<CurrentProject>().data.headers.clone();
    let maps = ColumnMapContributions::all(cx);
    let mut applied = 0;
    for header in &headers {
        let Some(entry) = entry_for(config, header) else {
            continue;
        };
        CurrentProject::set_column_type(header, &entry.data_type, cx);
        if !entry.description.is_empty() {
            CurrentProject::set_column_notes(header, &entry.description, cx);
        }
        settings::columns::update(header, |settings| apply_entry(entry, &maps, settings), cx);
        applied += 1;
    }
    log::info!(
        "applied a column config to {applied} of {} columns",
        headers.len()
    );
}

#[cfg(test)]
mod tests {
    use settings::columns::ColumnSettings;

    use crate::column_config::apply_entry;
    use crate::data::ColumnConfigEntry;

    /// Re-applying a config to an open project must not undo choices the file is silent on.
    #[test]
    fn only_what_the_config_states_is_overwritten() {
        let mut settings = ColumnSettings {
            spellcheck: false,
            authority: Some("LCSH".into()),
            ..Default::default()
        };
        apply_entry(
            &ColumnConfigEntry {
                name: "Subject".into(),
                variant_review: Some(true),
                ..Default::default()
            },
            &[],
            &mut settings,
        );
        assert!(
            !settings.spellcheck,
            "no Spellcheck column, so it stays off"
        );
        assert_eq!(settings.authority.as_deref(), Some("LCSH"));
        assert!(settings.variant_review);
    }
}

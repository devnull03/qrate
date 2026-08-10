//! Stage 4 · Set up your columns, plus the Stage 4c sub-dialog opened
//! from the "load from file/Sheet" card.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::Button;
use gpui_component::collapsible::Collapsible;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme, StyledExt, WindowExt, h_flex, v_flex};

use crate::data;
use crate::steps::files::{MsgKind, inline_message};
use crate::wizard::{ColumnSource, EntryKind, LoadConfigTab, ProjectWizard, option_card};

impl ProjectWizard {
    fn browse_config_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                let s = path.to_string_lossy().to_string();
                this.update_in(cx, |this, window, cx| {
                    this.load_config_from_file(s, cx);
                    let entity = cx.entity();
                    this.refresh_load_config_dialog(entity, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn load_config_from_file(&mut self, path: String, _cx: &mut Context<Self>) {
        self.config_file_path = path;
        let headers = self.spreadsheet_headers();
        match data::load_column_config(&self.config_file_path, &headers) {
            Ok(preview) => {
                self.config_preview = Some(preview);
                self.config_error = None;
            }
            Err(e) => {
                self.config_error = Some(e.message().into());
                self.config_preview = None;
            }
        }
    }

    /// Fetches the given public Sheet as `.xlsx` and turns its headers into a
    /// column config (one Text column each).
    // ponytail: blocking fetch on the UI thread — fine for a "Check" click;
    // move to the background executor if it ever drags.
    fn load_config_from_sheet(&mut self, cx: &mut Context<Self>) {
        let link = self.sheet_link_input.read(cx).value().to_string();
        let fetched = data_exchange::fetch_sheet(&link)
            .map_err(|e| e.to_string())
            .map(data_exchange::SpreadsheetPreview::from);
        match fetched {
            Ok(preview) => {
                self.config_preview = Some(data::ColumnConfigPreview {
                    entries: preview
                        .headers
                        .into_iter()
                        .map(|name| data::ColumnConfigEntry {
                            name,
                            data_type: "Text".into(),
                            ..Default::default()
                        })
                        .collect(),
                });
                self.config_error = None;
            }
            Err(msg) => {
                self.config_error = Some(msg.into());
                self.config_preview = None;
            }
        }
    }

    /// Registers the dialog with a builder that closes over a *snapshot* of
    /// the fields it needs (`tab`, `file_path`, ...), taken once right now.
    ///
    /// It must NOT capture `entity` and call `entity.read(cx)` inside the
    /// builder instead: `window.open_dialog`'s builder is re-invoked on every
    /// render of *this* view (`ProjectWizard::render` calls
    /// `Root::render_dialog_layer`, which calls this builder), and by the
    /// time it runs, `self` is already leased for that render — reading it
    /// again panics with "cannot read ... while it is already being
    /// updated". This isn't timing-dependent (deferring a click doesn't
    /// help); it happens on the very next frame the dialog is open,
    /// regardless of which button was pressed.
    ///
    /// Because the builder is a pure function of the snapshot, any click
    /// inside it that changes what the dialog should show (tab switch,
    /// Browse, Check) must call `refresh_load_config_dialog` afterwards to
    /// re-register the dialog with an up-to-date snapshot.
    fn open_load_config_dialog(&self, entity: Entity<Self>, window: &mut Window, cx: &mut App) {
        let tab = self.load_config_tab;
        let file_path = self.config_file_path.clone();
        let preview = self.config_preview.clone();
        let error = self.config_error.clone();
        let sheet_link_input = self.sheet_link_input.clone();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let tabs = TabBar::new("load-config-tabs")
                .segmented()
                .selected_index(match tab {
                    LoadConfigTab::File => 0,
                    LoadConfigTab::Sheet => 1,
                })
                .on_click({
                    let entity = entity.clone();
                    move |ix: &usize, window, cx| {
                        let tab = if *ix == 0 {
                            LoadConfigTab::File
                        } else {
                            LoadConfigTab::Sheet
                        };
                        entity.update(cx, |this, cx| {
                            this.load_config_tab = tab;
                            let entity = cx.entity();
                            this.refresh_load_config_dialog(entity, window, cx);
                        });
                    }
                })
                .child(Tab::new().label("From file"))
                .child(Tab::new().label("From Google Sheet"));

            let body = match tab {
                LoadConfigTab::File => {
                    let entity_browse = entity.clone();
                    v_flex()
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
                                        .child(if file_path.is_empty() {
                                            "Choose a file…".to_string()
                                        } else {
                                            file_path.clone()
                                        }),
                                )
                                .child(
                                    Button::new("browse-config")
                                        .label("Browse…")
                                        .outline()
                                        .on_click(move |_, window, cx| {
                                            entity_browse.update(cx, |this, cx| {
                                                this.browse_config_file(window, cx)
                                            });
                                        }),
                                ),
                        )
                        .into_any_element()
                }
                LoadConfigTab::Sheet => {
                    let entity_check = entity.clone();
                    v_flex()
                        .gap_1()
                        .child(Label::new("Sheet link").text_sm())
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Input::new(&sheet_link_input).flex_1())
                                .child(
                                    Button::new("check-config-sheet")
                                        .label("Check")
                                        .outline()
                                        .on_click(move |_, window, cx| {
                                            entity_check.update(cx, |this, cx| {
                                                this.load_config_from_sheet(cx);
                                                let entity = cx.entity();
                                                this.refresh_load_config_dialog(entity, window, cx);
                                            });
                                        }),
                                ),
                        )
                        .into_any_element()
                }
            };

            let status = match (&preview, &error) {
                (Some(p), _) => Some(
                    inline_message(
                        "config-status",
                        format!("Loaded {} columns", p.entries.len()),
                        MsgKind::Success,
                    )
                    .into_any_element(),
                ),
                (None, Some(e)) => Some(
                    inline_message("config-status", e.clone(), MsgKind::Error).into_any_element(),
                ),
                (None, None) => None,
            };

            let entity_ok = entity.clone();
            dialog
                .title("Load column config")
                .child(v_flex().gap_3().child(tabs).child(body).children(status))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Next →")
                        .cancel_text("← Back")
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let has_preview = entity_ok.read(cx).config_preview.is_some();
                    if has_preview {
                        entity_ok.update(cx, |this, cx| {
                            this.column_source = ColumnSource::LoadFromFileOrSheet;
                            cx.notify();
                        });
                        true
                    } else {
                        false
                    }
                })
        });
    }

    /// Re-registers the load-config dialog with a fresh snapshot. Call this
    /// after any mutation (inside the dialog) to state the dialog displays —
    /// see the note on `open_load_config_dialog` for why the dialog can't
    /// just re-read `self` on its own.
    fn refresh_load_config_dialog(&self, entity: Entity<Self>, window: &mut Window, cx: &mut App) {
        window.close_dialog(cx);
        self.open_load_config_dialog(entity, window, cx);
    }

    fn render_advanced_mapping(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let headers = self.spreadsheet_headers();
        let config = self.config_preview.clone();
        let open = self.show_advanced_mapping;

        Collapsible::new()
            .open(open)
            .child(
                v_flex()
                    .id("advanced-mapping-toggle")
                    .cursor_pointer()
                    .gap_1()
                    .p_2p5()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        Label::new(if open {
                            "▾ Advanced: column mapping (optional)"
                        } else {
                            "▸ Advanced: column mapping (optional)"
                        })
                        .text_sm(),
                    )
                    .when(!open, |el| {
                        el.child(
                            Label::new(
                                "We matched your columns automatically. Only open this if something looks off.",
                            )
                            .text_sm()
                            .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_advanced_mapping = !this.show_advanced_mapping;
                        cx.notify();
                    })),
            )
            .content({
                let mut table = v_flex().gap_1().mt_2().child(
                    Label::new("Spreadsheet column → qrate field")
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                );
                if headers.is_empty() {
                    table = table.child(
                        Label::new("No spreadsheet columns yet.")
                            .text_sm()
                            .text_color(cx.theme().muted_foreground),
                    );
                }
                for header in &headers {
                    let mapped = config.as_ref().and_then(|c| {
                        c.entries
                            .iter()
                            .find(|e| e.name.eq_ignore_ascii_case(header))
                            .map(|e| {
                                if e.data_type.is_empty() {
                                    e.name.clone()
                                } else {
                                    format!("{} ({})", e.name, e.data_type)
                                }
                            })
                    });
                    let (target, unmapped) = match mapped {
                        Some(name) => (name, false),
                        None => ("Unmapped".to_string(), true),
                    };
                    table = table.child(
                        h_flex()
                            .justify_between()
                            .text_sm()
                            .py_1()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            // The warning colour already says "unmapped" — a trailing glyph saying it
                            // again depends on font coverage for nothing.
                            .when(unmapped, |el| el.text_color(cx.theme().warning))
                            .child(header.clone())
                            .child(format!("→ {target}")),
                    );
                }
                table
            })
    }

    pub(crate) fn render_columns_step(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_blank = self.entry_kind == EntryKind::Blank;
        let auto_selected = self.column_source == ColumnSource::AutoFromSpreadsheet;
        let load_selected = self.column_source == ColumnSource::LoadFromFileOrSheet;
        let skip_selected = self.column_source == ColumnSource::SkipForNow;

        v_flex()
            .gap_3()
            .child(div().text_lg().font_semibold().child("Set up your columns"))
            .child(
                Label::new("Choose where the field list comes from.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .when(!is_blank, |el| {
                el.child(
                    option_card(
                        "col-auto",
                        "Auto-created from this spreadsheet",
                        "Default — columns matched from what you just imported.",
                        auto_selected,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.column_source = ColumnSource::AutoFromSpreadsheet;
                        cx.notify();
                    })),
                )
            })
            .child(
                option_card(
                    "col-load",
                    "Load from a file or Sheet link…",
                    "Same picker as your main import — CSV file or Google Sheet.",
                    load_selected,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let entity = cx.entity();
                    this.open_load_config_dialog(entity, window, cx);
                })),
            )
            .when(is_blank, |el| {
                el.child(
                    option_card(
                        "col-skip",
                        "Skip — set up columns later",
                        "Start empty; add columns in project settings whenever you're ready.",
                        skip_selected,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.column_source = ColumnSource::SkipForNow;
                        cx.notify();
                    })),
                )
            })
            .child(self.render_advanced_mapping(cx))
            .child(
                Label::new("You can always adjust this later in project settings.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
    }
}

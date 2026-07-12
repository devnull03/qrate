//! Stage 4 · Set up your columns, plus the Stage 4b/4c sub-dialogs opened
//! from the "recently used" and "load from file/Sheet" cards.

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
use crate::wizard::{
    ColumnSource, EntryKind, LoadConfigTab, ProjectWizard, RECENT_CONFIGS, option_card,
};

impl ProjectWizard {
    fn browse_config_file(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a column config file".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let s = path.to_string_lossy().to_string();
                this.update(cx, |this, cx| {
                    this.load_config_from_file(s, cx);
                    cx.notify();
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

    /// Fetches the given public Sheet as CSV and turns its headers into a
    /// column config (one Text column each).
    // ponytail: blocking fetch on the UI thread — fine for a "Check" click;
    // move to the background executor if it ever drags.
    fn load_config_from_sheet(&mut self, cx: &mut Context<Self>) {
        let link = self.sheet_link_input.read(cx).value().to_string();
        let fetched = cloud_sync::fetch_sheet_csv(&link)
            .map_err(|e| e.message())
            .and_then(|path| {
                data::load_csv_preview(&path.to_string_lossy()).map_err(|e| e.message())
            });
        match fetched {
            Ok(preview) => {
                self.config_preview = Some(data::ColumnConfigPreview {
                    entries: preview
                        .headers
                        .into_iter()
                        .map(|name| data::ColumnConfigEntry {
                            name,
                            data_type: "Text".into(),
                            description: String::new(),
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

    /// Associated fn (not `&mut self`): it must run *outside* the click
    /// listener's entity update, because `open_dialog`'s builder reads the
    /// same entity synchronously. Callers defer it via `window.defer`.
    fn open_recent_config_dialog(entity: Entity<Self>, window: &mut Window, cx: &mut App) {
        window.open_dialog(cx, move |dialog, _window, cx| {
            let selected_ix = entity.read(cx).recent_config_selected;

            let mut list = v_flex().gap_2();
            for (ix, config) in RECENT_CONFIGS.iter().enumerate() {
                let entity_row = entity.clone();
                list = list.child(
                    option_card(
                        ("recent-config", ix),
                        config.name,
                        format!(
                            "{} columns · last used {}",
                            config.column_count, config.last_used
                        ),
                        ix == selected_ix,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        entity_row.update(cx, |this, cx| {
                            this.recent_config_selected = ix;
                            cx.notify();
                        });
                    }),
                );
            }

            let warning = if RECENT_CONFIGS[selected_ix].name == "Photo Donations 2025" {
                Some("'Photo Donations 2025' only matches 2 of your 8 columns — use it anyway, or start fresh?")
            } else {
                None
            };

            let entity_ok = entity.clone();
            dialog
                .title("Choose a saved column config")
                .child(
                    v_flex()
                        .gap_3()
                        .child(list)
                        .when_some(warning, |el, msg| {
                            el.child(inline_message(msg, MsgKind::Warning, cx))
                        }),
                )
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Use this config")
                        .cancel_text("Cancel"),
                )
                .on_ok(move |_, _, cx| {
                    entity_ok.update(cx, |this, cx| {
                        this.column_source = ColumnSource::RecentlyUsed;
                        cx.notify();
                    });
                    true
                })
        });
    }

    /// See [`Self::open_recent_config_dialog`] — same deferred-open contract.
    fn open_load_config_dialog(entity: Entity<Self>, window: &mut Window, cx: &mut App) {
        window.open_dialog(cx, move |dialog, _window, cx| {
            let state = entity.read(cx);
            let tab = state.load_config_tab;
            let file_path = state.config_file_path.clone();
            let preview = state.config_preview.clone();
            let error = state.config_error.clone();
            let sheet_link_input = state.sheet_link_input.clone();

            let tabs = TabBar::new("load-config-tabs")
                .segmented()
                .selected_index(match tab {
                    LoadConfigTab::File => 0,
                    LoadConfigTab::Sheet => 1,
                })
                .on_click({
                    let entity = entity.clone();
                    move |ix: &usize, _, cx| {
                        let ix = *ix;
                        entity.update(cx, |this, cx| {
                            this.load_config_tab = if ix == 0 {
                                LoadConfigTab::File
                            } else {
                                LoadConfigTab::Sheet
                            };
                            cx.notify();
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
                                        .on_click(move |_, _, cx| {
                                            entity_browse
                                                .update(cx, |this, cx| this.browse_config_file(cx));
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
                                        .on_click(move |_, _, cx| {
                                            entity_check.update(cx, |this, cx| {
                                                this.load_config_from_sheet(cx)
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
                        format!("✓ Loaded {} columns", p.entries.len()),
                        MsgKind::Success,
                        cx,
                    )
                    .into_any_element(),
                ),
                (None, Some(e)) => {
                    Some(inline_message(e.clone(), MsgKind::Error, cx).into_any_element())
                }
                (None, None) => None,
            };

            let entity_ok = entity.clone();
            dialog
                .title("Load column config")
                .child(v_flex().gap_3().child(tabs).child(body).children(status))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Next →")
                        .cancel_text("← Back"),
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
                            .when(unmapped, |el| el.text_color(cx.theme().warning))
                            .child(header.clone())
                            .child(format!(
                                "→ {target}{}",
                                if unmapped { " ⚠" } else { "" }
                            )),
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
        let recent_selected = self.column_source == ColumnSource::RecentlyUsed;
        let load_selected = self.column_source == ColumnSource::LoadFromFileOrSheet;
        let skip_selected = self.column_source == ColumnSource::SkipForNow;
        let recent_summary = RECENT_CONFIGS
            .iter()
            .map(|c| format!("\"{}\"", c.name))
            .collect::<Vec<_>>()
            .join(" · ");

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
                    "col-recent",
                    "Choose a recently used config",
                    recent_summary,
                    recent_selected,
                    cx,
                )
                .on_click(cx.listener(|_this, _, window, cx| {
                    let entity = cx.entity();
                    window.defer(cx, move |window, cx| {
                        Self::open_recent_config_dialog(entity, window, cx);
                    });
                })),
            )
            .child(
                option_card(
                    "col-load",
                    "Load from a file or Sheet link…",
                    "Same picker as your main import — CSV file or Google Sheet.",
                    load_selected,
                    cx,
                )
                .on_click(cx.listener(|_this, _, window, cx| {
                    let entity = cx.entity();
                    window.defer(cx, move |window, cx| {
                        Self::open_load_config_dialog(entity, window, cx);
                    });
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

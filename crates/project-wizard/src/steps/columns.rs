//! Stage 4 · Set up your columns, plus the Stage 4c sub-dialog opened
//! from the "load from file/Sheet" card.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::collapsible::Collapsible;
use gpui_component::combobox::{Combobox, ComboboxState};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::label::Label;
use gpui_component::searchable_list::SearchableVec;
use gpui_component::{ActiveTheme, IndexPath, Sizable, StyledExt, WindowExt, v_flex};

use crate::column_config::{ColumnConfigLoader, mapping};
use crate::data;
use crate::steps::files::{MsgKind, inline_message};
use crate::wizard::{ColumnChoice, ColumnSource, EntryKind, ProjectWizard, option_card};

fn sync_required_picker(
    state: &Entity<ComboboxState<SearchableVec<ColumnChoice>>>,
    choices: &[ColumnChoice],
    selected: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let selected = selected.unwrap_or_default();
    let current = state.read(cx).selected_values();
    if current.len() == 1 && current[0].as_ref() == selected {
        return;
    }
    let index = choices
        .iter()
        .position(|choice| choice.value.as_ref() == selected)
        .unwrap_or_default();
    state.update(cx, |state, cx| {
        state.set_selected_indices(vec![IndexPath::new(index)], window, cx);
    });
}

fn inferred_required_column(
    headers: &[String],
    config: Option<&data::ColumnConfigPreview>,
    kind: settings::columns::ColumnType,
) -> Option<String> {
    config
        .and_then(|config| {
            config.entries.iter().find_map(|entry| {
                (settings::columns::ColumnType::from_declared(&entry.data_type) == kind)
                    .then(|| {
                        headers
                            .iter()
                            .find(|header| header.eq_ignore_ascii_case(&entry.name))
                            .cloned()
                    })
                    .flatten()
            })
        })
        .or_else(|| {
            headers
                .iter()
                .find(|header| settings::columns::ColumnType::from_declared(header) == kind)
                .cloned()
        })
}
impl ProjectWizard {
    fn inferred_required_column(&self, kind: settings::columns::ColumnType) -> Option<String> {
        inferred_required_column(&self.effective_headers(), self.selected_config(), kind)
    }

    fn prefill_required_columns(&mut self) {
        if self.title_column.is_none() {
            self.title_column = self.inferred_required_column(settings::columns::ColumnType::Title);
        }
        if self.file_column.is_none() {
            self.file_column =
                self.inferred_required_column(settings::columns::ColumnType::Filename);
        }
    }

    fn reset_required_column_defaults(&mut self) {
        self.title_column = None;
        self.file_column = None;
        self.prefill_required_columns();
    }

    fn render_required_columns(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.prefill_required_columns();
        let choices: Vec<ColumnChoice> = std::iter::once(ColumnChoice {
            value: "".into(),
            label: "Choose a column…".into(),
        })
        .chain(self.effective_headers().into_iter().map(|header| {
            let label: SharedString = header.clone().into();
            ColumnChoice {
                value: header.into(),
                label,
            }
        }))
        .collect();
        if self.required_column_choices != choices {
            self.title_picker.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(choices.clone()), window, cx);
            });
            self.file_picker.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(choices.clone()), window, cx);
            });
            self.required_column_choices = choices.clone();
        }
        sync_required_picker(
            &self.title_picker,
            &choices,
            self.title_column.as_deref(),
            window,
            cx,
        );
        sync_required_picker(
            &self.file_picker,
            &choices,
            self.file_column.as_deref(),
            window,
            cx,
        );
        let missing = self.title_column.as_deref().unwrap_or_default().is_empty()
            || self.file_column.as_deref().unwrap_or_default().is_empty();

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(if missing {
                cx.theme().warning
            } else {
                cx.theme().border
            })
            .child(div().font_semibold().child("Required columns"))
            .child(
                Label::new("Choose the title shown for each row and the file linked to it.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Title column").text_sm())
                    .child(
                        Combobox::new(&self.title_picker)
                            .small()
                            .w_full()
                            .placeholder("Choose a column…"),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("File column").text_sm())
                    .child(
                        Combobox::new(&self.file_picker)
                            .small()
                            .w_full()
                            .placeholder("Choose a column…"),
                    ),
            )
            .when(missing, |block| {
                block.child(inline_message(
                    "required-columns-error",
                    "Choose both required columns to continue.",
                    MsgKind::Error,
                ))
            })
    }
    fn open_load_config_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let headers = if self.entry_kind == EntryKind::Blank {
            Vec::new()
        } else {
            self.spreadsheet_headers()
        };
        let loader = match &self.config_loader {
            // The spreadsheet may have changed since, and a config is checked against its headers.
            Some(loader) => {
                loader.update(cx, |loader, _| loader.headers = headers);
                loader.clone()
            }
            None => {
                let loader = cx.new(|cx| ColumnConfigLoader::new(headers, window, cx));
                self.config_loader.insert(loader).clone()
            }
        };
        let entity = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (loader_ok, entity) = (loader.clone(), entity.clone());
            dialog
                .title("Load column config")
                .child(loader.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Next →")
                        .cancel_text("← Back")
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let Some(preview) = loader_ok.read(cx).preview.clone() else {
                        return false;
                    };
                    entity.update(cx, |this, cx| {
                        this.config_preview = Some(preview);
                        this.column_source = ColumnSource::LoadFromFileOrSheet;
                        this.reset_required_column_defaults();
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn render_advanced_mapping(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // Title and File are picked above, in the required-columns section. Listing them here too
        // showed each one twice with two different answers — the picker's and the config's.
        let headers: Vec<String> = self
            .effective_headers()
            .into_iter()
            .filter(|h| {
                ![self.title_column.as_deref(), self.file_column.as_deref()]
                    .iter()
                    .flatten()
                    .any(|chosen| chosen == h)
            })
            .collect();
        let config = self.selected_config().cloned();
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
            .content(div().mt_2().child(mapping(&headers, config.as_ref(), cx)))
    }

    pub(crate) fn render_columns_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_blank = self.entry_kind == EntryKind::Blank;
        let auto_selected = self.column_source == ColumnSource::AutoFromSpreadsheet;
        let load_selected = self.column_source == ColumnSource::LoadFromFileOrSheet;
        let skip_selected = self.column_source == ColumnSource::DefaultBlank;

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
                        this.reset_required_column_defaults();
                        cx.notify();
                    })),
                )
            })
            .child(
                option_card(
                    "col-load",
                    "Load from a file or Sheet link…",
                    "Same picker as your main import — a local spreadsheet or a Google Sheet.",
                    load_selected,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_load_config_dialog(window, cx);
                })),
            )
            .when(is_blank, |el| {
                el.child(
                    option_card(
                        "col-skip",
                        "Use default Title and File columns",
                        "Start with qrate's two required columns. You can add more later.",
                        skip_selected,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.column_source = ColumnSource::DefaultBlank;
                        this.reset_required_column_defaults();
                        cx.notify();
                    })),
                )
            })
            .child(self.render_required_columns(window, cx))
            .when(load_selected, |el| {
                el.child(self.render_advanced_mapping(cx))
            })
            .child(
                Label::new("You can always adjust this later in project settings.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
    }
}

#[cfg(test)]
mod tests {
    use settings::columns::ColumnType;

    use crate::data::{ColumnConfigEntry, ColumnConfigPreview};
    use crate::steps::columns::inferred_required_column;

    #[test]
    fn declared_config_roles_take_priority_over_header_names() {
        let headers = vec!["Title".into(), "Object Name".into(), "File".into()];
        let config = ColumnConfigPreview {
            entries: vec![ColumnConfigEntry {
                name: "Object Name".into(),
                data_type: "Title".into(),
                ..Default::default()
            }],
        };

        assert_eq!(
            inferred_required_column(&headers, Some(&config), ColumnType::Title).as_deref(),
            Some("Object Name")
        );
        assert_eq!(
            inferred_required_column(&headers, Some(&config), ColumnType::Filename).as_deref(),
            Some("File")
        );
    }

    #[test]
    fn unrelated_headers_do_not_infer_required_roles() {
        let headers = vec!["Identifier".into(), "Description".into()];

        assert_eq!(
            inferred_required_column(&headers, None, ColumnType::Title),
            None
        );
        assert_eq!(
            inferred_required_column(&headers, None, ColumnType::Filename),
            None
        );
    }
}

//! Stage 3a/3b · Files — the CSV and Google Sheet forks. Both need a files
//! folder and share the same folder-matching validation; only the top field
//! (spreadsheet file vs. sheet link) differs.

use gpui::*;
use gpui_component::button::Button;
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt, h_flex, v_flex};

use crate::data;
use crate::wizard::{EntryKind, ProjectWizard};

pub(crate) fn inline_message(
    text: impl Into<SharedString>,
    kind: MsgKind,
    cx: &App,
) -> impl IntoElement {
    let (color, icon) = match kind {
        MsgKind::Success => (cx.theme().success, IconName::CircleCheck),
        MsgKind::Warning => (cx.theme().warning, IconName::TriangleAlert),
        MsgKind::Error => (cx.theme().danger, IconName::TriangleAlert),
    };
    h_flex()
        .gap_1()
        .text_sm()
        .text_color(color)
        .child(Icon::new(icon))
        .child(text.into())
}

pub(crate) enum MsgKind {
    Success,
    Warning,
    Error,
}

impl ProjectWizard {
    fn browse_for_csv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose your spreadsheet".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let s = path.to_string_lossy().to_string();
                this.update(cx, |this, cx| {
                    this.set_csv_path(s, cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        let _ = window;
    }

    fn browse_for_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose your files folder".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let s = path.to_string_lossy().to_string();
                this.update(cx, |this, cx| {
                    this.set_folder_path(s, cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        let _ = window;
    }

    fn set_csv_path(&mut self, path: String, _cx: &mut Context<Self>) {
        self.csv_path = path;
        match data::load_csv_preview(&self.csv_path) {
            Ok(preview) => {
                self.csv_preview = Some(preview);
                self.csv_error = None;
            }
            Err(e) => {
                self.csv_error = Some(e.message().into());
                self.csv_preview = None;
            }
        }
        self.revalidate_folder();
    }

    fn set_folder_path(&mut self, path: String, _cx: &mut Context<Self>) {
        self.folder_path = path;
        self.revalidate_folder();
    }

    fn revalidate_folder(&mut self) {
        if self.folder_path.is_empty() {
            self.folder_match = None;
            self.folder_error = None;
            return;
        }
        let result = match self.entry_kind {
            EntryKind::Csv => self.csv_preview.as_ref().map(|preview| {
                data::match_folder(preview, &self.folder_path)
            }),
            EntryKind::Sheet => self
                .sheet_check
                .as_ref()
                .map(|check| data::match_folder_sheet(check.row_count, &self.folder_path)),
            EntryKind::Blank => None,
        };
        match result {
            Some(Ok(m)) => {
                self.folder_match = Some(m);
                self.folder_error = None;
            }
            Some(Err(e)) => {
                self.folder_match = None;
                self.folder_error = Some(e.message().into());
            }
            None => {
                self.folder_match = None;
                self.folder_error = None;
            }
        }
    }

    fn check_sheet_link(&mut self, cx: &mut Context<Self>) {
        let link = self.sheet_link_input.read(cx).value().to_string();
        match data::validate_sheet_link(&link) {
            Ok(check) => {
                self.sheet_check = Some(check);
                self.sheet_error = None;
            }
            Err(e) => {
                self.sheet_error = Some(e.message().into());
                self.sheet_check = None;
            }
        }
        self.revalidate_folder();
    }

    pub(crate) fn render_files_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        match self.entry_kind {
            EntryKind::Csv => self.render_csv_files(window, cx).into_any_element(),
            EntryKind::Sheet => self.render_sheet_files(window, cx).into_any_element(),
            EntryKind::Blank => div().into_any_element(),
        }
    }

    fn render_csv_files(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let csv_display = if self.csv_path.is_empty() {
            "Choose a CSV file…".to_string()
        } else {
            self.csv_path.clone()
        };
        let folder_display = if self.folder_path.is_empty() {
            "Choose your files folder…".to_string()
        } else {
            self.folder_path.clone()
        };

        v_flex()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child("Choose your spreadsheet & files"),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Spreadsheet (CSV)").text_sm())
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
                                    .child(csv_display),
                            )
                            .child(
                                Button::new("browse-csv").label("Browse…").outline().on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.browse_for_csv(window, cx)
                                    }),
                                ),
                            ),
                    )
                    .child(match (&self.csv_preview, &self.csv_error) {
                        (Some(p), _) => inline_message(
                            format!("✓ {} rows, {} columns found", p.row_count(), p.column_count()),
                            MsgKind::Success,
                            cx,
                        )
                        .into_any_element(),
                        (None, Some(e)) => inline_message(e.clone(), MsgKind::Error, cx).into_any_element(),
                        (None, None) => div().into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Files folder").text_sm())
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
                                    .child(folder_display),
                            )
                            .child(
                                Button::new("browse-folder").label("Browse…").outline().on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.browse_for_folder(window, cx)
                                    }),
                                ),
                            ),
                    )
                    .child(self.render_folder_status(cx)),
            )
    }

    fn render_sheet_files(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let folder_display = if self.folder_path.is_empty() {
            "Choose your files folder…".to_string()
        } else {
            self.folder_path.clone()
        };

        v_flex()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child("Connect your Google Sheet"),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Sheet link").text_sm())
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Input::new(&self.sheet_link_input).flex_1())
                            .child(
                                Button::new("check-sheet").label("Check").outline().on_click(
                                    cx.listener(|this, _, _, cx| this.check_sheet_link(cx)),
                                ),
                            ),
                    )
                    .child(match (&self.sheet_check, &self.sheet_error) {
                        (Some(c), _) => inline_message(
                            format!(
                                "✓ Found \"{}\" — {} rows{}",
                                c.title,
                                c.row_count,
                                if c.used_first_tab {
                                    " (using the first tab, 'Sheet1')"
                                } else {
                                    ""
                                }
                            ),
                            MsgKind::Success,
                            cx,
                        )
                        .into_any_element(),
                        (None, Some(e)) => inline_message(e.clone(), MsgKind::Error, cx).into_any_element(),
                        (None, None) => div().into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Files folder").text_sm())
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
                                    .child(folder_display),
                            )
                            .child(
                                Button::new("browse-folder-sheet")
                                    .label("Browse…")
                                    .outline()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse_for_folder(window, cx)
                                    })),
                            ),
                    )
                    .child(self.render_folder_status(cx)),
            )
    }

    fn render_folder_status(&self, cx: &App) -> AnyElement {
        match (&self.folder_match, &self.folder_error) {
            (Some(m), _) if m.matched_rows == m.total_rows => inline_message(
                format!("✓ {} of {} files matched", m.matched_rows, m.total_rows),
                MsgKind::Success,
                cx,
            )
            .into_any_element(),
            (Some(m), _) => inline_message(
                format!(
                    "⚠ Matched {} of {} files — review mismatches",
                    m.matched_rows, m.total_rows
                ),
                MsgKind::Warning,
                cx,
            )
            .into_any_element(),
            (None, Some(e)) => inline_message(e.clone(), MsgKind::Error, cx).into_any_element(),
            (None, None) => div().into_any_element(),
        }
    }
}

//! Stage 3a/3b · Files — the CSV and Google Sheet forks. Both need a files
//! folder and share the same folder-matching validation; only the top field
//! (spreadsheet file vs. sheet link) differs.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::Button;
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Disableable, Icon, IconName, StyledExt, h_flex, v_flex};

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
            // Sheet reuses `csv_preview` (its fetched xlsx is adapted into the
            // same preview shape), so both match folders against real row data.
            EntryKind::Csv | EntryKind::Sheet => self
                .csv_preview
                .as_ref()
                .map(|preview| data::match_folder(preview, &self.folder_path)),
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

    /// `auto_advance` lets the Next button (which doubles as "Check" while the
    /// sheet is unverified) skip the extra click once the check succeeds and
    /// the folder already matches — the explicit "Check" button never does
    /// this, since clicking it isn't a request to leave the step.
    pub(crate) fn check_sheet_link(&mut self, auto_advance: bool, cx: &mut Context<Self>) {
        let link = self.sheet_link_input.read(cx).value().to_string();
        // Fetch is a blocking network call — run it on a background thread so
        // the UI doesn't freeze, then apply the result back on the UI thread.
        let fetch = cx
            .background_executor()
            .spawn(async move { cloud_sync::fetch_sheet(&link) });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let ready = this
                .update(cx, |this, cx| {
                    match result.map(data::SpreadsheetPreview::from) {
                        Ok(preview) => {
                            this.sheet_check = Some(data::SheetCheckResult {
                                title: "Google Sheet".into(),
                                row_count: preview.rows.len(),
                                used_first_tab: true,
                            });
                            this.csv_preview = Some(preview);
                            this.sheet_error = None;
                        }
                        Err(e) => {
                            this.sheet_error = Some(e.to_string().into());
                            this.sheet_check = None;
                            this.csv_preview = None;
                        }
                    }
                    this.revalidate_folder();
                    cx.notify();
                    auto_advance && this.can_advance(cx).is_ok()
                })
                .unwrap_or(false);
            if ready {
                // Brief pause so the "found N rows" success message is visible
                // before the step changes out from under the user.
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(600))
                    .await;
                this.update(cx, |this, cx| {
                    this.advance_past_files();
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn render_files_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let body = match self.entry_kind {
            EntryKind::Csv => self.render_csv_files(window, cx).into_any_element(),
            EntryKind::Sheet => self.render_sheet_files(window, cx).into_any_element(),
            EntryKind::Blank => self.render_blank_files(window, cx).into_any_element(),
        };
        v_flex()
            .gap_3()
            .child(body)
            .child(self.render_skip_files_toggle(cx))
    }

    /// The "Files folder" picker, shared by all three Files variants. Dims and
    /// disables itself when "add files later" is checked — only this field, not
    /// the spreadsheet/sheet input above it. `show_status` appends the
    /// folder-match result (Blank has no spreadsheet to match against).
    fn folder_field(
        &self,
        browse_id: &'static str,
        show_status: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let folder_display = if self.folder_path.is_empty() {
            "Choose your files folder…".to_string()
        } else {
            self.folder_path.clone()
        };
        let dimmed = self.skip_files;
        let status = show_status.then(|| self.render_folder_status(cx));
        v_flex()
            .gap_1()
            .when(dimmed, |el| el.opacity(0.4))
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
                        Button::new(browse_id)
                            .label("Browse…")
                            .outline()
                            .disabled(dimmed)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.browse_for_folder(window, cx)
                            })),
                    ),
            )
            .children(status)
    }

    /// "I'll add files later" — flips `skip_files`, which also skips the Link
    /// step (see `ProjectWizard::skips_link`). Shown on every Files variant.
    fn render_skip_files_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let checked = self.skip_files;
        h_flex()
            .id("skip-files-toggle")
            .gap_1()
            .cursor_pointer()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(if checked { "☑" } else { "☐" })
            .child("I'll add a files folder later — skips the linking step")
            .on_click(cx.listener(|this, _, _, cx| {
                this.skip_files = !this.skip_files;
                cx.notify();
            }))
    }

    fn render_blank_files(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(div().text_lg().font_semibold().child("Add your files"))
            .child(
                Label::new("Point qrate at a folder of files, or skip and add them later.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(self.folder_field("browse-folder-blank", false, cx))
    }

    fn render_csv_files(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let csv_display = if self.csv_path.is_empty() {
            "Choose a CSV file…".to_string()
        } else {
            self.csv_path.clone()
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
                                Button::new("browse-csv")
                                    .label("Browse…")
                                    .outline()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse_for_csv(window, cx)
                                    })),
                            ),
                    )
                    .child(match (&self.csv_preview, &self.csv_error) {
                        (Some(p), _) => inline_message(
                            format!("✓ {} rows, {} columns found", p.rows.len(), p.headers.len()),
                            MsgKind::Success,
                            cx,
                        )
                        .into_any_element(),
                        (None, Some(e)) => {
                            inline_message(e.clone(), MsgKind::Error, cx).into_any_element()
                        }
                        (None, None) => div().into_any_element(),
                    }),
            )
            .child(self.folder_field("browse-folder", true, cx))
    }

    fn render_sheet_files(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                                Button::new("check-sheet")
                                    .label("Check")
                                    .outline()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.check_sheet_link(false, cx)
                                    })),
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
                        (None, Some(e)) => {
                            inline_message(e.clone(), MsgKind::Error, cx).into_any_element()
                        }
                        (None, None) => div().into_any_element(),
                    }),
            )
            .child(self.folder_field("browse-folder-sheet", true, cx))
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

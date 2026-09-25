use gpui::{prelude::FluentBuilder, *};
use gpui_component::alert::Alert;
use gpui_component::button::Button;
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::text::Text;
use gpui_component::{ActiveTheme, Disableable, Selectable, Sizable, StyledExt, h_flex, v_flex};

use file_ingest::duplicates::DuplicatePolicy;

use crate::data;
use crate::wizard::{EntryKind, ProjectWizard};

pub(crate) fn inline_message(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    kind: MsgKind,
) -> impl IntoElement {
    let text = Text::from(text.into());
    match kind {
        MsgKind::Success => Alert::success(id, text),
        MsgKind::Warning => Alert::warning(id, text),
        MsgKind::Error => Alert::error(id, text),
    }
    .small()
}

pub(crate) enum MsgKind {
    Success,
    Warning,
    Error,
}

impl ProjectWizard {
    fn browse_for_local_file(&mut self, cx: &mut Context<Self>) {
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
                    this.set_local_path(s, cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn browse_for_folder(&mut self, cx: &mut Context<Self>) {
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
    }

    pub(crate) fn set_local_path(&mut self, path: String, _cx: &mut Context<Self>) {
        self.local_path = path;
        match data::load_spreadsheet_preview(&self.local_path) {
            Ok(preview) => {
                self.spreadsheet_preview = Some(preview);
                self.local_error = None;
            }
            Err(e) => {
                self.local_error = Some(e.message().into());
                self.spreadsheet_preview = None;
            }
        }
        self.revalidate_folder();
    }

    pub(crate) fn set_folder_path(&mut self, path: String, _cx: &mut Context<Self>) {
        self.import_paths = vec![std::path::PathBuf::from(&path)];
        self.folder_path = path;
        self.revalidate_folder();
    }

    pub(crate) fn set_import_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        _cx: &mut Context<Self>,
    ) {
        if self.entry_kind != EntryKind::Blank {
            if paths.len() == 1 && paths[0].is_dir() {
                self.set_folder_path(paths[0].to_string_lossy().into_owned(), _cx);
            } else {
                self.folder_error = Some(
                    "Multiple files and folders can start a blank project; choose one files folder for a spreadsheet-backed project."
                        .into(),
                );
            }
            return;
        }
        let Ok(mut plan) = file_ingest::plan_paths(
            &paths,
            &file_ingest::PlanOptions {
                recursive: self.recurse_subfolders,
                include_root: self.include_root_folder,
                ..Default::default()
            },
        ) else {
            self.folder_error = Some("Those files or folders couldn't be read.".into());
            return;
        };
        let folder_path = match paths.as_slice() {
            [path] if path.is_dir() => path.to_string_lossy().into_owned(),
            _ => {
                for component in &mut plan.components {
                    component.source_path = component.absolute_path.clone();
                }
                String::new()
            }
        };
        let extra_files = plan
            .components
            .iter()
            .filter(|component| component.kind == file_ingest::EntryKind::File)
            .map(|component| component.title.clone())
            .collect();
        self.import_paths = paths;
        self.folder_path = folder_path;
        self.folder_plan = Some(plan);
        self.folder_match = Some(data::FolderMatch {
            matched_rows: 0,
            total_rows: 0,
            extra_files,
            ambiguous_files: 0,
        });
        self.folder_error = None;
    }

    pub(crate) fn revalidate_folder(&mut self) {
        if self.folder_path.is_empty() {
            self.folder_match = None;
            self.folder_error = None;
            return;
        }
        let result = match self.entry_kind {
            EntryKind::LocalFile | EntryKind::Sheet => {
                self.spreadsheet_preview.as_ref().map(|preview| {
                    data::match_folder(preview, &self.folder_path, self.recurse_subfolders)
                })
            }
            EntryKind::Blank => Some(data::inventory_folder(
                &self.folder_path,
                self.recurse_subfolders,
            )),
        };
        match result {
            Some(Ok(m)) => {
                self.folder_match = Some(m);
                self.folder_error = None;
                self.refresh_folder_plan();
            }
            Some(Err(e)) => {
                self.folder_match = None;
                self.folder_plan = None;
                self.folder_error = Some(e.message().into());
            }
            None => {
                self.folder_match = None;
                self.folder_plan = None;
                self.folder_error = None;
            }
        }
    }

    fn refresh_folder_plan(&mut self) {
        self.folder_plan = file_ingest::plan_paths(
            &self.import_paths,
            &file_ingest::PlanOptions {
                recursive: self.recurse_subfolders,
                include_root: self.include_root_folder,
                ..Default::default()
            },
        )
        .ok();
        if self.folder_path.is_empty()
            && let Some(plan) = &mut self.folder_plan
        {
            for component in &mut plan.components {
                component.source_path = component.absolute_path.clone();
            }
        }
    }

    pub(crate) fn check_sheet_link(&mut self, auto_advance: bool, cx: &mut Context<Self>) {
        let link = self.sheet_link_input.read(cx).value().to_string();
        // Fetch is a blocking network call — run it on a background thread so
        // the UI doesn't freeze, then apply the result back on the UI thread.
        let fetch = cx
            .background_executor()
            .spawn(async move { data_exchange::fetch_sheet(&link) });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let ready = this
                .update(cx, |this, cx| {
                    match result.map(data_exchange::SpreadsheetPreview::from) {
                        Ok(preview) => {
                            this.sheet_check = Some(data::SheetCheckResult {
                                title: "Google Sheet".into(),
                                row_count: preview.rows.len(),
                                used_first_tab: true,
                            });
                            this.spreadsheet_preview = Some(preview);
                            this.sheet_error = None;
                        }
                        Err(e) => {
                            this.sheet_error = Some(e.to_string().into());
                            this.sheet_check = None;
                            this.spreadsheet_preview = None;
                        }
                    }
                    this.revalidate_folder();
                    cx.notify();
                    auto_advance && this.can_advance(cx).is_ok()
                })
                .unwrap_or(false);
            if ready {
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let path_box = |text: String| {
            div()
                .flex_1()
                .min_w(px(0.))
                .px_2()
                .py_1p5()
                .rounded_md()
                .border_1()
                .border_color(border)
                .text_sm()
                .text_color(muted)
                .child(text)
        };
        let (title, browse_id) = match self.entry_kind {
            EntryKind::LocalFile => ("Choose your spreadsheet & files", "browse-folder"),
            EntryKind::Sheet => ("Connect your Google Sheet", "browse-folder-sheet"),
            EntryKind::Blank => ("Add your files", "browse-folder-blank"),
        };
        let selected = self.import_paths.len();
        let ambiguous = self
            .folder_match
            .as_ref()
            .map_or(0, |matched| matched.ambiguous_files);
        let description_profile = self.description_profile;
        let duplicate_policy = self.duplicate_policy;
        let dimmed = self.skip_files;

        let body = v_flex()
            .gap_3()
            .child(div().text_lg().font_semibold().child(title))
            .child(match self.entry_kind {
                EntryKind::LocalFile => {
                    v_flex()
                        .gap_1()
                        .child(Label::new("Spreadsheet (CSV, Excel, ODS)").text_sm())
                        .child(
                            h_flex()
                                .gap_2()
                                .child(path_box(if self.local_path.is_empty() {
                                    "Choose a spreadsheet file…".to_string()
                                } else {
                                    self.local_path.clone()
                                }))
                                .child(
                                    Button::new("browse-local")
                                        .label("Browse…")
                                        .outline()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.browse_for_local_file(cx)
                                        })),
                                ),
                        )
                        .child(match (&self.spreadsheet_preview, &self.local_error) {
                            (Some(p), _) => inline_message(
                                "local-status",
                                format!("{} rows, {} columns found", p.rows.len(), p.headers.len()),
                                MsgKind::Success,
                            )
                            .into_any_element(),
                            (None, Some(e)) => {
                                inline_message("local-status", e.clone(), MsgKind::Error)
                                    .into_any_element()
                            }
                            (None, None) => div().into_any_element(),
                        })
                        .into_any_element()
                }
                EntryKind::Sheet => v_flex()
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
                            "sheet-status",
                            format!(
                                "Found \"{}\" — {} rows{}",
                                c.title,
                                c.row_count,
                                if c.used_first_tab {
                                    " (using the first tab, 'Sheet1')"
                                } else {
                                    ""
                                }
                            ),
                            MsgKind::Success,
                        )
                        .into_any_element(),
                        (None, Some(e)) => {
                            inline_message("sheet-status", e.clone(), MsgKind::Error)
                                .into_any_element()
                        }
                        (None, None) => div().into_any_element(),
                    })
                    .into_any_element(),
                EntryKind::Blank => {
                    Label::new("Point qrate at a folder of files, or skip and add them later.")
                        .text_sm()
                        .text_color(muted)
                        .into_any_element()
                }
            })
            .child(
                v_flex()
                    .gap_1()
                    .when(dimmed, |el| el.opacity(0.4))
                    .child(Label::new("Files folder").text_sm())
                    .child(
                        h_flex()
                            .gap_2()
                            .child(path_box(if self.folder_path.is_empty() {
                                "Choose your files folder…".to_string()
                            } else {
                                self.folder_path.clone()
                            }))
                            .child(
                                Button::new(browse_id)
                                    .label("Browse…")
                                    .outline()
                                    .disabled(dimmed)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.browse_for_folder(cx)),
                                    ),
                            ),
                    )
                    .child(match (&self.folder_match, &self.folder_error) {
                        (Some(m), _) if self.entry_kind == EntryKind::Blank => inline_message(
                            "folder-status",
                            format!(
                                "{} file{} will become table rows",
                                m.extra_files.len(),
                                if m.extra_files.len() == 1 { "" } else { "s" },
                            ),
                            MsgKind::Success,
                        )
                        .into_any_element(),
                        (Some(m), _) if m.matched_rows == m.total_rows => inline_message(
                            "folder-status",
                            format!("{} of {} files matched", m.matched_rows, m.total_rows),
                            MsgKind::Success,
                        )
                        .into_any_element(),
                        (Some(m), _) => inline_message(
                            "folder-status",
                            format!(
                                "Matched {} of {} files — review mismatches",
                                m.matched_rows, m.total_rows
                            ),
                            MsgKind::Warning,
                        )
                        .into_any_element(),
                        (None, Some(e)) => {
                            inline_message("folder-status", e.clone(), MsgKind::Error)
                                .into_any_element()
                        }
                        (None, None) => div().into_any_element(),
                    }),
            )
            .when(
                self.entry_kind == EntryKind::Blank && self.folder_path.is_empty() && selected > 0,
                |files| {
                    files.child(inline_message(
                        "selected-import-paths",
                        format!(
                            "Selected {selected} file{} or folder{}.",
                            if selected == 1 { "" } else { "s" },
                            if selected == 1 { "" } else { "s" }
                        ),
                        MsgKind::Success,
                    ))
                },
            );

        v_flex()
            .id("files-drop-area")
            .gap_3()
            .drag_over::<ExternalPaths>(|style, _, _, cx| {
                style.bg(cx.theme().secondary_hover)
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let paths = paths.paths();
                if this.entry_kind == EntryKind::Blank {
                    this.set_import_paths(paths.to_vec(), cx);
                } else if paths.len() == 1 && paths[0].is_dir() {
                    this.set_folder_path(paths[0].to_string_lossy().into_owned(), cx);
                } else if this.entry_kind == EntryKind::LocalFile
                    && paths.len() == 1
                    && paths[0].is_file()
                {
                    this.set_local_path(paths[0].to_string_lossy().into_owned(), cx);
                } else {
                    this.folder_error = Some(
                        "Drop one folder here. You can drop several individual files into the table after the project opens."
                            .into(),
                    );
                }
                cx.notify();
            }))
            .child(body)
            .when(!self.skip_files, |this| {
                this.child(
                    Checkbox::new("include-root-folder")
                        .label("Create a row for the selected files folder")
                        .checked(self.include_root_folder)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.include_root_folder = *checked;
                            this.refresh_folder_plan();
                            cx.notify();
                        })),
                )
                .child(
                    Label::new(
                        "Leave this off when the folder only contains this project's material.",
                    )
                    .text_sm()
                    .text_color(muted),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .pt_2()
                        .border_t_1()
                        .border_color(border)
                        .child(Label::new("Archival description standard").text_sm())
                        .child(
                            Label::new(
                                "Folder and file rows become archival components using this vocabulary.",
                            )
                            .text_sm()
                            .text_color(muted),
                        )
                        .child(
                            h_flex().gap_1().flex_wrap().children(
                                settings::description::DescriptionProfile::ALL
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, profile)| {
                                        Button::new(("description-profile", index))
                                            .label(profile.label())
                                            .outline()
                                            .selected(profile == description_profile)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.description_profile = profile;
                                                let defaults = profile.defaults();
                                                let label_for = |key: &str, fallback| {
                                                    defaults
                                                        .levels
                                                        .iter()
                                                        .find(|level| level.key == key)
                                                        .map_or(fallback, |level| {
                                                            level.label.clone()
                                                        })
                                                };
                                                let folder_label = label_for(
                                                    &defaults.folder_level_key,
                                                    "Group".to_string(),
                                                );
                                                let file_label = label_for(
                                                    &defaults.file_level_key,
                                                    "Item".to_string(),
                                                );
                                                this.folder_level_input.update(cx, |input, cx| {
                                                    input.set_value(folder_label, window, cx)
                                                });
                                                this.file_level_input.update(cx, |input, cx| {
                                                    input.set_value(file_label, window, cx)
                                                });
                                                cx.notify();
                                            }))
                                    }),
                            ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .gap_1()
                                        .child(Label::new("Folders are called").text_sm())
                                        .child(Input::new(&self.folder_level_input)),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .gap_1()
                                        .child(Label::new("Files are called").text_sm())
                                        .child(Input::new(&self.file_level_input)),
                                ),
                        ),
                )
            })
            .when(!self.skip_files && ambiguous > 0, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .pt_2()
                        .border_t_1()
                        .border_color(border)
                        .child(Label::new("Files named by more than one row").text_sm())
                        .child(
                            Label::new(format!(
                                "{ambiguous} file{} match several rows. qrate never picks for you unless you \
                                 ask it to.",
                                if ambiguous == 1 { "" } else { "s" }
                            ))
                            .text_sm()
                            .text_color(muted),
                        )
                        .child(
                            h_flex().gap_1().flex_wrap().children(
                                [
                                    (
                                        DuplicatePolicy::Skip,
                                        "Leave them for me",
                                        "each file becomes its own row",
                                    ),
                                    (
                                        DuplicatePolicy::Update,
                                        "Link the first row",
                                        "in spreadsheet order",
                                    ),
                                    (DuplicatePolicy::AddAsNew, "Add as new rows", "link nothing"),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(index, (policy, label, hint))| {
                                    Button::new(("duplicate-policy", index))
                                        .label(label)
                                        .tooltip(hint)
                                        .outline()
                                        .selected(policy == duplicate_policy)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.duplicate_policy = policy;
                                            cx.notify();
                                        }))
                                }),
                            ),
                        ),
                )
            })
            .child(
                h_flex()
                    .id("skip-files-toggle")
                    .gap_1()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(muted)
                    .child(if self.skip_files { "☑" } else { "☐" })
                    .child("I'll add a files folder later — skips the linking step")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip_files = !this.skip_files;
                        cx.notify();
                    })),
            )
    }
}

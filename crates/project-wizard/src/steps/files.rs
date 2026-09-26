use gpui::{prelude::FluentBuilder, *};
use gpui_component::alert::Alert;
use gpui_component::button::Button;
use gpui_component::checkbox::Checkbox;
use gpui_component::collapsible::Collapsible;
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::text::Text;
use gpui_component::{ActiveTheme, Icon, IconName, Selectable, Sizable, StyledExt, h_flex, v_flex};

use file_ingest::duplicates::DuplicatePolicy;

use crate::data;
use crate::wizard::{EntryKind, ProjectWizard, option_card};

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

/// A files folder checked against the spreadsheet: the match summary and the rows it plans.
type FolderOutcome =
    Result<(data::FolderMatch, Option<file_ingest::ImportPlan>), data::FolderError>;

/// Everything a folder check reads, compared when its answer lands to tell whether the wizard
/// still wants it. The spreadsheet stands in as its headers and row count.
#[derive(Clone, PartialEq)]
struct FolderCheck {
    folder: String,
    import_paths: Vec<std::path::PathBuf>,
    recursive: bool,
    include_root: bool,
    rows: Option<(Vec<String>, usize)>,
}

impl FolderCheck {
    /// One scan of the folder, used for both the match and the plan. Blocks on the file system.
    fn run(&self, preview: Option<&data_exchange::SpreadsheetPreview>) -> FolderOutcome {
        let inventory = data::scan_folder(&self.folder, self.recursive)?;
        let files = data::file_names(&inventory);
        let matched = match preview {
            Some(preview) => data::match_files(preview, files, &self.folder, self.recursive)?,
            None => data::FolderMatch {
                matched_rows: 0,
                total_rows: 0,
                extra_files: files,
                ambiguous_files: 0,
            },
        };
        let options = file_ingest::PlanOptions {
            recursive: self.recursive,
            include_root: self.include_root,
            ..Default::default()
        };
        let root = std::path::Path::new(&self.folder);
        let plan = match self.import_paths.as_slice() {
            [only] if only == root => Some(file_ingest::plan_inventory(root, inventory, &options)),
            paths => file_ingest::plan_paths(paths, &options).ok(),
        };
        Ok((matched, plan))
    }
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

    /// Read the chosen spreadsheet off the UI thread. Until it lands neither a preview nor an error
    /// is set, which the step shows as "Reading the spreadsheet…" and which blocks Next.
    pub(crate) fn set_local_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.local_path = path.clone();
        self.spreadsheet_preview = None;
        self.local_error = None;
        let read = cx.background_spawn({
            let path = path.clone();
            async move { data::load_spreadsheet_preview(&path) }
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            this.update(cx, |this, cx| {
                // A newer choice is already being read; this answer is for a file nobody wants.
                if this.local_path != path {
                    return;
                }
                match read {
                    Ok(preview) => this.spreadsheet_preview = Some(preview),
                    Err(e) => this.local_error = Some(e.message().into()),
                }
                this.revalidate_folder_in_background(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        self.revalidate_folder_in_background(cx);
    }

    pub(crate) fn set_folder_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.import_paths = vec![std::path::PathBuf::from(&path)];
        self.folder_path = path;
        self.revalidate_folder_in_background(cx);
    }

    pub(crate) fn set_import_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.entry_kind != EntryKind::Blank {
            if paths.len() == 1 && paths[0].is_dir() {
                self.set_folder_path(paths[0].to_string_lossy().into_owned(), cx);
            } else {
                self.folder_error = Some(
                    "Multiple files and folders can start a blank project; choose one files folder for a spreadsheet-backed project."
                        .into(),
                );
            }
            return;
        }
        if let [path] = paths.as_slice()
            && path.is_dir()
        {
            self.set_folder_path(path.to_string_lossy().into_owned(), cx);
            return;
        }
        self.import_paths = paths.clone();
        self.folder_path.clear();
        self.folder_plan = None;
        self.folder_match = None;
        self.folder_error = None;
        self.folder_check_generation = self.folder_check_generation.wrapping_add(1);
        let generation = self.folder_check_generation;
        let options = file_ingest::PlanOptions {
            recursive: self.recurse_subfolders,
            include_root: self.include_root_folder,
            ..Default::default()
        };
        let scan = cx.background_spawn({
            let paths = paths.clone();
            async move { file_ingest::plan_paths(&paths, &options) }
        });
        cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                if this.folder_check_generation != generation || this.import_paths != paths {
                    return;
                }
                match result {
                    Ok(mut plan) => {
                        for component in &mut plan.components {
                            component.source_path = component.absolute_path.clone();
                        }
                        let extra_files = plan
                            .components
                            .iter()
                            .filter(|component| component.kind == file_ingest::EntryKind::File)
                            .map(|component| component.title.clone())
                            .collect();
                        this.folder_plan = Some(plan);
                        this.folder_match = Some(data::FolderMatch {
                            matched_rows: 0,
                            total_rows: 0,
                            extra_files,
                            ambiguous_files: 0,
                        });
                    }
                    Err(_) => {
                        this.folder_error = Some("Those files or folders couldn't be read.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// What a folder check depends on, `None` when there is nothing to check yet: no folder, or a
    /// spreadsheet-backed project whose spreadsheet has not been read.
    fn folder_check(&self) -> Option<FolderCheck> {
        let spreadsheet = self.entry_kind != EntryKind::Blank;
        if self.folder_path.is_empty() || (spreadsheet && self.spreadsheet_preview.is_none()) {
            return None;
        }
        Some(FolderCheck {
            folder: self.folder_path.clone(),
            import_paths: self.import_paths.clone(),
            recursive: self.recurse_subfolders,
            include_root: self.include_root_folder,
            rows: self
                .spreadsheet_preview
                .as_ref()
                .filter(|_| spreadsheet)
                .map(|preview| (preview.headers.clone(), preview.rows.len())),
        })
    }

    fn apply_folder_check(&mut self, outcome: Option<FolderOutcome>) {
        match outcome {
            Some(Ok((matched, plan))) => {
                self.folder_match = Some(matched);
                self.folder_plan = plan;
                self.folder_error = None;
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

    /// Scan the files folder once, match it against the spreadsheet and plan its rows from that
    /// same scan, all off the UI thread. Until the answer lands neither a match nor an error is
    /// set, which the step shows as "Checking the folder…"; an answer for inputs that have since
    /// changed is dropped, since the check they started is already on its way.
    pub(crate) fn revalidate_folder_in_background(&mut self, cx: &mut Context<Self>) {
        self.folder_check_generation = self.folder_check_generation.wrapping_add(1);
        let generation = self.folder_check_generation;
        self.apply_folder_check(None);
        let Some(check) = self.folder_check() else {
            return;
        };
        let preview = check
            .rows
            .is_some()
            .then(|| self.spreadsheet_preview.clone())
            .flatten();
        let run = cx.background_spawn({
            let check = check.clone();
            async move { check.run(preview.as_ref()) }
        });
        cx.spawn(async move |this, cx| {
            let outcome = run.await;
            this.update(cx, |this, cx| {
                if this.folder_check_generation == generation
                    && this.folder_check().as_ref() == Some(&check)
                {
                    this.apply_folder_check(Some(outcome));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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
                    this.revalidate_folder_in_background(cx);
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
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(text)
        };
        // A label with its requirement on the right: "Spreadsheet … Required", "Files … Optional".
        let field_heading = |label: &'static str, tag: Option<&'static str>| {
            h_flex()
                .justify_between()
                .items_baseline()
                .child(Label::new(label).text_sm())
                .when_some(tag, |el, tag| {
                    el.child(div().text_xs().text_color(muted).child(tag))
                })
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
        let skip_files = self.skip_files;
        let checking = !self.import_paths.is_empty()
            && self.folder_match.is_none()
            && self.folder_error.is_none()
            && (self.entry_kind == EntryKind::Blank || self.spreadsheet_preview.is_some());
        // Once files are deferred, the spreadsheet is the only thing this step asks for, so it
        // says so instead of leaving the archivist to wonder whether "later" covered it too.
        let source_tag = skip_files.then_some("Required");

        let source = match self.entry_kind {
            EntryKind::LocalFile => v_flex()
                .gap_1()
                .child(field_heading("Spreadsheet (CSV, Excel, ODS)", source_tag))
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
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.browse_for_local_file(cx)),
                                ),
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
                        inline_message("local-status", e.clone(), MsgKind::Error).into_any_element()
                    }
                    (None, None) if !self.local_path.is_empty() => {
                        Label::new("Reading the spreadsheet…")
                            .text_sm()
                            .text_color(muted)
                            .into_any_element()
                    }
                    (None, None) => div().into_any_element(),
                })
                .into_any_element(),
            EntryKind::Sheet => v_flex()
                .gap_1()
                .child(field_heading("Sheet link", source_tag))
                .child(
                    h_flex()
                        .gap_2()
                        .child(Input::new(&self.sheet_link_input).flex_1())
                        .child(
                            Button::new("check-sheet")
                                .label("Check")
                                .outline()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.check_sheet_link(false, cx)),
                                ),
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
                        inline_message("sheet-status", e.clone(), MsgKind::Error).into_any_element()
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
        };

        // The two-way choice that replaced the "I'll add a files folder later" checkbox at the
        // bottom of the step: the choice now sits beside the thing it decides about.
        let choice = h_flex()
            .gap_2()
            .items_stretch()
            .child(
                option_card(
                    "files-link-now",
                    "Link a files folder",
                    "Match photos and scans to rows now.",
                    !skip_files,
                    cx,
                )
                .flex_1()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.skip_files = false;
                    cx.notify();
                })),
            )
            .child(
                option_card(
                    "files-add-later",
                    "Add files later",
                    if self.entry_kind == EntryKind::Blank {
                        "Start with an empty table. Skips linking."
                    } else {
                        "Import the spreadsheet only. Skips linking."
                    },
                    skip_files,
                    cx,
                )
                .flex_1()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.skip_files = true;
                    cx.notify();
                })),
            );

        let folder_status = match (&self.folder_match, &self.folder_error) {
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
            (Some(m), _)
                if m.matched_rows == m.total_rows
                    && m.extra_files.is_empty()
                    && m.ambiguous_files == 0 =>
            {
                inline_message(
                    "folder-status",
                    format!("{} of {} files matched", m.matched_rows, m.total_rows),
                    MsgKind::Success,
                )
                .into_any_element()
            }
            (Some(m), _) => self.mismatch_breakdown(m, cx).into_any_element(),
            (None, Some(e)) => {
                inline_message("folder-status", e.clone(), MsgKind::Error).into_any_element()
            }
            (None, None) if checking => Label::new("Checking the folder…")
                .text_sm()
                .text_color(muted)
                .into_any_element(),
            (None, None) => div().into_any_element(),
        };

        let files = v_flex()
            .gap_1p5()
            .pt_2p5()
            .border_t_1()
            .border_color(border)
            .when(self.entry_kind != EntryKind::Blank, |el| {
                el.child(field_heading("Files", Some("Optional")))
            })
            .child(choice)
            .map(|el| {
                if skip_files {
                    el.child(self.deferred_files_summary(cx))
                } else {
                    el.child(
                        h_flex()
                            .gap_2()
                            .mt_1()
                            .child(path_box(if self.folder_path.is_empty() {
                                "Choose your files folder…".to_string()
                            } else {
                                self.folder_path.clone()
                            }))
                            .child(Button::new(browse_id).label("Browse…").outline().on_click(
                                cx.listener(|this, _, _, cx| this.browse_for_folder(cx)),
                            )),
                    )
                    .child(folder_status)
                    .when(
                        self.entry_kind == EntryKind::Blank
                            && self.folder_path.is_empty()
                            && selected > 0,
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
                    )
                }
            });

        v_flex()
            .id("files-drop-area")
            .gap_3()
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().secondary_hover))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let paths = paths.paths();
                if this.entry_kind == EntryKind::Blank {
                    this.skip_files = false;
                    this.set_import_paths(paths.to_vec(), cx);
                } else if paths.len() == 1 && paths[0].is_dir() {
                    this.skip_files = false;
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
            .child(div().text_lg().font_semibold().child(title))
            .child(source)
            .child(files)
            .when(!skip_files, |el| el.child(self.render_files_advanced(ambiguous, cx)))
    }

    /// 3b: a partial match, broken down by what happens to each group. Every sentence describes
    /// behaviour qrate already has — unmatched files become rows, missing files reach Problems.
    fn mismatch_breakdown(
        &self,
        m: &data::FolderMatch,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let foreground = cx.theme().foreground;
        let warning = cx.theme().warning;
        let missing = m.total_rows.saturating_sub(m.matched_rows);
        let extra = m.extra_files.len();
        let ambiguous = m.ambiguous_files;
        let plural = |n: usize, one: &'static str, many: &'static str| {
            format!("{n} {}", if n == 1 { one } else { many })
        };
        let line = |count: String, rest: &'static str| {
            div().text_sm().text_color(muted).child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .child(div().text_color(foreground).child(count))
                    .child(rest),
            )
        };
        v_flex()
            .gap_1p5()
            .px_2p5()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(warning.opacity(0.6))
            .bg(warning.opacity(0.1))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .small()
                            .text_color(warning),
                    )
                    .child(format!(
                        "Matched {} of {} files — review mismatches",
                        m.matched_rows, m.total_rows
                    )),
            )
            .child(
                v_flex()
                    .gap_0p5()
                    .pl_6()
                    .when(missing > 0, |el| {
                        el.child(line(
                            plural(missing, "row", "rows"),
                            "name a file that isn't in this folder. They'll be listed in Problems.",
                        ))
                    })
                    .when(extra > 0, |el| {
                        el.child(line(
                            plural(extra, "file", "files"),
                            "aren't named by any row. Each arrives as its own row.",
                        ))
                    })
                    .when(ambiguous > 0, |el| {
                        el.child(
                            h_flex()
                                .flex_wrap()
                                .gap_1()
                                .text_sm()
                                .text_color(muted)
                                .child(
                                    div()
                                        .text_color(foreground)
                                        .child(plural(ambiguous, "file", "files")),
                                )
                                .child(format!(
                                    "{} named by more than one row. {}",
                                    if ambiguous == 1 { "is" } else { "are" },
                                    duplicate_summary(self.duplicate_policy),
                                ))
                                .child(
                                    div()
                                        .id("duplicates-open-advanced")
                                        .cursor_pointer()
                                        .text_color(cx.theme().link)
                                        .hover(|el| el.underline())
                                        .child("Change in Advanced")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_advanced_files = true;
                                            cx.notify();
                                        })),
                                ),
                        )
                    }),
            )
    }

    /// 3c: what "Add files later" means, in place of the folder field it removed.
    fn deferred_files_summary(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let rows = self.spreadsheet_preview.as_ref().map(|p| p.rows.len());
        let first = match (self.entry_kind, rows) {
            (EntryKind::Blank, _) => "The project starts with one empty row to type into.".into(),
            (EntryKind::Sheet, Some(n)) => format!("All {n} rows are imported from the sheet."),
            (EntryKind::Sheet, None) => "Every row is imported from the sheet.".into(),
            (_, Some(n)) => format!("All {n} rows are imported from the spreadsheet."),
            (_, None) => "Every row is imported from the spreadsheet.".into(),
        };
        let second = if self.entry_kind == EntryKind::Blank {
            "No folder is added now."
        } else {
            "No folder is matched, and the Link step is skipped."
        };
        let line = |mark: &'static str, mark_color: Hsla, text: AnyElement| {
            h_flex()
                .gap_2()
                .items_start()
                .child(div().flex_none().text_color(mark_color).child(mark))
                .child(div().flex_1().min_w(px(0.)).child(text))
        };
        v_flex()
            .gap_1p5()
            .px_3()
            .py_2p5()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tiles)
            .text_sm()
            .child(line("✓", cx.theme().success, first.into_any_element()))
            .child(line(
                "–",
                muted,
                div().text_color(muted).child(second).into_any_element(),
            ))
            .child(line(
                "→",
                muted,
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .text_color(muted)
                    .child("Add a folder any time from")
                    .child(
                        div()
                            .text_color(cx.theme().foreground)
                            .child("File ▸ Import Files or Folders…"),
                    )
                    .into_any_element(),
            ))
    }

    /// Folder rows, description standard and duplicate handling, folded into one block that
    /// opens the same way the Columns step's Advanced mapping does. Closed, it reads back the
    /// defaults in one line so nobody has to open it to know what will happen.
    fn render_files_advanced(&self, ambiguous: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let open = self.show_advanced_files;
        let description_profile = self.description_profile;
        let duplicate_policy = self.duplicate_policy;
        let folder_label = self.folder_level_input.read(cx).value().trim().to_string();
        let file_label = self.file_level_input.read(cx).value().trim().to_string();
        let summary = {
            let mut parts = vec![
                format!("Using {}", description_profile.label()),
                format!("folders are {folder_label}, files are {file_label}"),
                if self.include_root_folder {
                    "a row for the selected folder".to_string()
                } else {
                    "no row for the selected folder".to_string()
                },
            ];
            if ambiguous > 0 {
                parts.push(format!(
                    "duplicates: {}",
                    duplicate_label(duplicate_policy).to_lowercase()
                ));
            }
            format!("{}.", parts.join(" · "))
        };

        let content = v_flex()
            .gap_3()
            .mt_2()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Checkbox::new("include-root-folder")
                            .label("Create a row for the selected files folder")
                            .checked(self.include_root_folder)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.include_root_folder = *checked;
                                if !this.import_paths.is_empty() {
                                    if this.folder_path.is_empty() {
                                        this.set_import_paths(this.import_paths.clone(), cx);
                                    } else {
                                        this.revalidate_folder_in_background(cx);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Label::new(
                            "Leave this off when the folder only contains this project's material.",
                        )
                        .text_sm()
                        .text_color(muted),
                    ),
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
                                                    .map_or(fallback, |level| level.label.clone())
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
            .when(ambiguous > 0, |el| {
                el.child(
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
                                    (DuplicatePolicy::Skip, "each file becomes its own row"),
                                    (DuplicatePolicy::Update, "in spreadsheet order"),
                                    (DuplicatePolicy::AddAsNew, "link nothing"),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(index, (policy, hint))| {
                                    Button::new(("duplicate-policy", index))
                                        .label(duplicate_label(policy))
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
            });

        Collapsible::new()
            .open(open)
            .child(
                v_flex()
                    .id("advanced-files-toggle")
                    .cursor_pointer()
                    .gap_1()
                    .p_2p5()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .child(
                        Label::new(if open {
                            "▾ Advanced: folder rows, description standard, duplicates (optional)"
                        } else {
                            "▸ Advanced: folder rows, description standard, duplicates (optional)"
                        })
                        .text_sm(),
                    )
                    .when(!open, |el| {
                        el.child(Label::new(summary).text_sm().text_color(muted))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_advanced_files = !this.show_advanced_files;
                        cx.notify();
                    })),
            )
            .content(content)
    }
}

fn duplicate_label(policy: DuplicatePolicy) -> &'static str {
    match policy {
        DuplicatePolicy::Skip => "Leave them for me",
        DuplicatePolicy::Update => "Link the first row",
        DuplicatePolicy::AddAsNew => "Add as new rows",
    }
}

/// What the chosen duplicate policy does, as the end of a sentence about those files.
fn duplicate_summary(policy: DuplicatePolicy) -> &'static str {
    match policy {
        DuplicatePolicy::Skip => "qrate leaves them for you.",
        DuplicatePolicy::Update => "Each links the first row that names it.",
        DuplicatePolicy::AddAsNew => "Each arrives as a new row, linked to nothing.",
    }
}

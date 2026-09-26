//! The files folder from the grid's side: the banner when the folder has moved, relinking it with
//! a preview, locating one row's file, and what a file dropped from outside the folder becomes.

use std::path::PathBuf;

use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use settings::columns::ColumnType;
use settings::history::Origin;
use settings::project::{CurrentProject, FILES_FOLDER_KEY, IMPORT_OUTSIDE_FILES_KEY};

use super::TablePanel;
use crate::delegate::TableChanged;
use crate::file_links::{self, BaseProblem, FilesBase};
use crate::{photos, relink};

fn files_folder(cx: &App) -> String {
    cx.try_global::<CurrentProject>()
        .and_then(|project| project.data.values.get(FILES_FOLDER_KEY))
        .map(|folder| folder.text().to_string())
        .unwrap_or_default()
}

impl TablePanel {
    /// The one notice that the files folder as a whole is gone or holds almost none of the
    /// project's files, or nothing. Drawn above whichever view is showing.
    pub fn render_files_banner(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some((problem, folder)) = cx
            .try_global::<FilesBase>()
            .filter(|base| !base.dismissed)
            .and_then(|base| base.problem.clone())
        else {
            return Empty.into_any_element();
        };
        let warning = cx.theme().warning;
        h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(warning.opacity(0.12))
            .child(
                Icon::new(IconName::TriangleAlert)
                    .small()
                    .text_color(warning),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .truncate()
                    .child(match problem {
                        BaseProblem::Missing => format!("Files folder not found: {folder}"),
                        BaseProblem::MostlyMissing => {
                            format!("Most linked files are missing from {folder}")
                        }
                    }),
            )
            .child(
                Button::new("files-banner-relink")
                    .small()
                    .label("Relink…")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.choose_files_root(window, cx)),
                    ),
            )
            .child(
                Button::new("files-banner-dismiss")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip("Dismiss")
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.global_mut::<FilesBase>().dismissed = true;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// File ▸ Relink Missing Files…: pick a folder, count what it holds off the UI thread, and
    /// commit only once the archivist has seen the count.
    pub fn choose_files_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose the folder that contains this project's files".into()),
        });
        let cells: Vec<SharedString> = {
            let delegate = self.state.read(cx).delegate();
            (0..delegate.column_count())
                .filter(|&col| delegate.column_type(col) == ColumnType::Filename)
                .flat_map(|col| delegate.column_cells(col))
                .collect()
        };
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(chosen) = paths.into_iter().next() else {
                return;
            };
            let preview = cx
                .background_executor()
                .spawn(async move { relink::preview(&chosen, &file_links::folder_names(&cells)) })
                .await;
            this.update_in(cx, |this, window, cx| {
                if preview.total == 0 {
                    this.set_files_root(preview.chosen, cx);
                    return;
                }
                let (folder, detail) = match preview.instead {
                    Some((nearby, found)) => (
                        nearby.clone(),
                        format!(
                            "None of the {} linked files are in {}. Found {found} in {} instead.",
                            preview.total,
                            preview.chosen.display(),
                            nearby.display()
                        ),
                    ),
                    None => (
                        preview.chosen.clone(),
                        format!(
                            "{} of {} linked files found in {}.",
                            preview.found,
                            preview.total,
                            preview.chosen.display()
                        ),
                    ),
                };
                let answer = window.prompt(
                    PromptLevel::Info,
                    "Relink files",
                    Some(&detail),
                    &["Relink", "Choose another…", "Cancel"],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let answer = answer.await;
                    this.update_in(cx, |this, window, cx| match answer {
                        Ok(0) => this.set_files_root(folder, cx),
                        Ok(1) => this.choose_files_root(window, cx),
                        _ => {}
                    })
                    .ok();
                })
                .detach();
            })
            .ok();
        })
        .detach();
    }

    /// Point the project at `folder` and read it afresh; the walk revalidates when it lands, which
    /// is what clears the missing files and the banner.
    fn set_files_root(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        log::info!("files folder relinked to {}", folder.display());
        CurrentProject::set_text(
            FILES_FOLDER_KEY,
            folder.to_string_lossy().into_owned().into(),
            cx,
        );
        photos::forget(cx);
        self.state.update(cx, |state, cx| {
            photos::refresh(state, None, cx);
            state.refresh(cx);
            cx.notify();
        });
        cx.notify();
    }

    /// Details' "Locate file…": pick the file a row lost, and offer to re-link the other missing
    /// files that sit in the same folder in the same undo step.
    pub fn locate_file(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let delegate = self.state.read(cx).delegate();
        let Some((col, _)) = file_links::missing_file(delegate, row, cx) else {
            return;
        };
        let missing: Vec<(usize, usize, String)> = file_links::missing_files(delegate, cx)
            .into_iter()
            .filter(|(other, _, _)| *other != row)
            .map(|(row, col, value)| (row, col, value.to_string()))
            .collect();
        let root = PathBuf::from(files_folder(cx));
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Locate the file for this row".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(picked) = paths.into_iter().next() else {
                return;
            };
            let dir = picked.parent().map(PathBuf::from).unwrap_or_default();
            let others = {
                let dir = dir.clone();
                cx.background_executor()
                    .spawn(async move { relink::beside(&dir, &missing) })
                    .await
            };
            let mut links = vec![(row, col, relink::link_text(&root, &picked).into())];
            let also: Vec<_> = others
                .iter()
                .map(|(row, col, path)| (*row, *col, relink::link_text(&root, path).into()))
                .collect();
            if !also.is_empty() {
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Info,
                        &format!(
                            "Also relink {} other missing file{} in this folder?",
                            also.len(),
                            if also.len() == 1 { "" } else { "s" }
                        ),
                        Some(&dir.display().to_string()),
                        &["Relink all", "Just this one", "Cancel"],
                        cx,
                    )
                }) else {
                    return;
                };
                match answer.await {
                    Ok(0) => links.extend(also),
                    Ok(1) => {}
                    _ => return,
                }
            }
            this.update(cx, |this, cx| this.write_links(links, Origin::Details, cx))
                .ok();
        })
        .detach();
    }

    /// A file dropped on a Filename cell: links that row to it, after the files-folder question.
    pub fn link_dropped_file(
        &mut self,
        row: usize,
        col: usize,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.place_outside_files(vec![path], window, cx, move |this, paths, _, cx| {
            let root = PathBuf::from(files_folder(cx));
            let links = paths
                .iter()
                .map(|path| (row, col, relink::link_text(&root, path).into()))
                .collect();
            this.write_links(links, Origin::Typed, cx);
        });
    }

    /// Re-link rows as one undo step, then do what a committed edit does. A link the cached walk
    /// cannot see, such as a file just copied in, has the folder walked again.
    fn write_links(
        &mut self,
        links: Vec<(usize, usize, SharedString)>,
        origin: Origin,
        cx: &mut Context<Self>,
    ) {
        if links.is_empty() {
            return;
        }
        let rows: Vec<usize> = links.iter().map(|(row, _, _)| *row).collect();
        let stale = photos::cached_index(&files_folder(cx), cx).is_none_or(|index| {
            links
                .iter()
                .any(|(_, _, link)| index.resolve_cell(link).is_none())
        });
        if stale {
            photos::forget(cx);
        }
        self.state.update(cx, |state, cx| {
            state.delegate_mut().relink(links, origin);
            photos::refresh(state, (!stale).then_some(&rows[..]), cx);
            cx.emit(TableChanged);
            cx.notify();
        });
        settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
        self.schedule_revalidate(cx);
        self.schedule_autosave(cx);
    }

    /// Settle where dropped paths that are not in the files folder should live, per the project's
    /// setting or by asking, then hand the paths to `then`. A copy lands in `imported/` off the UI
    /// thread. With no files folder set there is nothing to be outside of, and nothing is asked.
    pub(super) fn place_outside_files(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Vec<PathBuf>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let folder = files_folder(cx);
        let root = PathBuf::from(&folder);
        let outside: Vec<&PathBuf> = match folder.trim().is_empty() {
            true => Vec::new(),
            false => paths
                .iter()
                .filter(|path| qrate_export::relative_to(&root, path).is_none())
                .collect(),
        };
        if outside.is_empty() {
            then(self, paths, window, cx);
            return;
        }
        let mode = cx
            .try_global::<CurrentProject>()
            .and_then(|project| project.data.values.get(IMPORT_OUTSIDE_FILES_KEY))
            .map(|mode| mode.text().to_string())
            .unwrap_or_default();
        let answer = (mode != "copy" && mode != "link").then(|| {
            let what = match outside.as_slice() {
                [one] => format!(
                    "“{}” is",
                    one.file_name().unwrap_or_default().to_string_lossy()
                ),
                many => format!("{} of the dropped items are", many.len()),
            };
            window.prompt(
                PromptLevel::Info,
                "Files from outside the files folder",
                Some(&format!(
                    "{what} not in {folder}. Copy into {}, or link where it is?",
                    root.join(relink::IMPORTED).display()
                )),
                &["Copy into the project", "Link where it is", "Cancel"],
                cx,
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let copy = match answer {
                Some(answer) => match answer.await {
                    Ok(0) => true,
                    Ok(1) => false,
                    _ => return,
                },
                None => mode == "copy",
            };
            let (paths, failed) = match copy {
                false => (paths, Vec::new()),
                true => {
                    cx.background_executor()
                        .spawn(async move {
                            let mut failed = Vec::new();
                            let placed = paths
                                .into_iter()
                                .filter_map(|path| {
                                    if qrate_export::relative_to(&root, &path).is_some() {
                                        return Some(path);
                                    }
                                    relink::copy_in(&root, &path)
                                        .inspect_err(|error| {
                                            log::error!(
                                                "Could not copy {} into the files folder: {error}",
                                                path.display()
                                            );
                                            failed.push(format!("{}: {error}", path.display()));
                                        })
                                        .ok()
                                })
                                .collect::<Vec<_>>();
                            (placed, failed)
                        })
                        .await
                }
            };
            this.update_in(cx, |this, window, cx| {
                if copy {
                    photos::forget(cx);
                }
                if !failed.is_empty() {
                    drop(window.prompt(
                        PromptLevel::Warning,
                        "Some files were not copied",
                        Some(&failed.join("\n")),
                        &["OK"],
                        cx,
                    ));
                }
                if !paths.is_empty() {
                    then(this, paths, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use std::path::{Path, PathBuf};

    use gpui::{Entity, TestAppContext, VisualTestContext};

    use crate::TablePanel;
    use crate::file_links::{BaseProblem, FilesBase};

    fn tempdir(case: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("qrate-files-panel-test")
            .join(case);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }

    /// One Filename column holding `rows`, linked to `folder`, with the file checks registered
    /// and autosave off so the temp project is never written.
    fn open<'a>(
        cx: &'a mut TestAppContext,
        folder: &Path,
        rows: &[&str],
        values: &[(&str, &str)],
    ) -> (Entity<TablePanel>, &'a mut VisualTestContext) {
        let folder = folder.to_string_lossy().into_owned();
        let values: Vec<(String, String)> = [
            (settings::AUTOSAVE_KEY, "off"),
            (settings::project::FILES_FOLDER_KEY, folder.as_str()),
        ]
        .into_iter()
        .chain(values.iter().copied())
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        let rows: Vec<Vec<String>> = rows.iter().map(|row| vec![row.to_string()]).collect();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::project::CurrentProject {
                file: std::env::temp_dir().join("qrate-files-panel.qrate"),
                data: settings::project::ProjectData {
                    name: "T".into(),
                    columns: vec![settings::project::ProjectColumn {
                        name: "File".into(),
                        data_type: "Filename".into(),
                        notes: String::new(),
                    }],
                    headers: vec!["File".into()],
                    row_ids: (1..=rows.len() as i64).collect(),
                    rows,
                    values: values
                        .into_iter()
                        .map(|(key, value)| (key, settings::Val::Text(value.into())))
                        .collect(),
                },
            });
            diagnostics::AsyncValidators::register(
                crate::file_links::SOURCE,
                crate::file_links::check,
                cx,
            );
        });
        let (panel, cx) = cx.add_window_view(TablePanel::new);
        cx.run_until_parked();
        (panel, cx)
    }

    fn cells(panel: &Entity<TablePanel>, cx: &mut VisualTestContext) -> Vec<String> {
        panel.read_with(cx, |panel, cx| {
            let delegate = panel.state.read(cx).delegate();
            (0..delegate.row_count())
                .map(|row| {
                    delegate
                        .cell(row, 0)
                        .map(|c| c.to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
    }

    fn sources(panel: &Entity<TablePanel>, cx: &mut VisualTestContext) -> Vec<Option<String>> {
        panel.read_with(cx, |panel, cx| {
            let structure = panel.state.read(cx).delegate().row_structure();
            structure
                .iter()
                .map(|row| row.source_path.clone())
                .collect()
        })
    }

    /// Let the debounced revalidation after an edit run, and the walk it may start land.
    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
    }

    fn problem(cx: &mut VisualTestContext) -> Option<BaseProblem> {
        cx.update(|_, cx| {
            cx.try_global::<FilesBase>()
                .and_then(|base| base.problem.as_ref().map(|(problem, _)| *problem))
        })
    }

    fn findings(cx: &mut VisualTestContext, severity: diagnostics::Severity) -> usize {
        cx.update(|_, cx| {
            diagnostics::Diagnostics::all(cx)
                .iter()
                .filter(|d| d.severity == severity)
                .filter(|d| {
                    d.source == diagnostics::Source::Validator(crate::file_links::SOURCE.into())
                })
                .count()
        })
    }

    /// A folder that is gone is one banner, not a problem per row; pointing the project at the
    /// right folder clears the banner and brings back the rows that really are missing.
    #[gpui::test]
    fn a_missing_folder_is_one_banner_until_it_is_relinked(cx: &mut TestAppContext) {
        let dir = tempdir("banner");
        touch(&dir.join("found").join("1.jpg"));
        let (panel, cx) = open(cx, &dir.join("moved"), &["1.jpg", "2.jpg", "3.jpg"], &[]);

        assert_eq!(problem(cx), Some(BaseProblem::Missing));
        assert_eq!(findings(cx, diagnostics::Severity::Error), 0);

        panel.update(cx, |panel, cx| panel.set_files_root(dir.join("found"), cx));
        cx.run_until_parked();
        assert_eq!(problem(cx), None);
        assert_eq!(
            findings(cx, diagnostics::Severity::Error),
            2,
            "the two files that are truly gone"
        );
    }

    /// Five names with none of them in the folder reads as the wrong folder, not five problems.
    #[gpui::test]
    fn a_folder_holding_almost_none_of_the_names_is_a_banner(cx: &mut TestAppContext) {
        let dir = tempdir("mostly");
        touch(&dir.join("unrelated.jpg"));
        let (_panel, cx) = open(
            cx,
            &dir,
            &["1.jpg", "2.jpg", "3.jpg", "4.jpg", "5.jpg"],
            &[],
        );
        assert_eq!(problem(cx), Some(BaseProblem::MostlyMissing));
        assert_eq!(findings(cx, diagnostics::Severity::Error), 0);
    }

    #[gpui::test]
    fn relinking_shows_the_count_before_it_commits(cx: &mut TestAppContext) {
        let dir = tempdir("relink");
        touch(&dir.join("new").join("1.jpg"));
        let (panel, cx) = open(cx, &dir.join("old"), &["1.jpg", "2.jpg"], &[]);

        panel.update_in(cx, |panel, window, cx| panel.choose_files_root(window, cx));
        cx.simulate_path_prompt_response(|_| Some(vec![dir.join("new")]));
        cx.run_until_parked();
        let (_, detail) = cx.pending_prompt().expect("the count is shown first");
        assert!(
            detail.starts_with("1 of 2 linked files found in"),
            "{detail}"
        );
        cx.simulate_prompt_answer("Relink");
        cx.run_until_parked();

        let folder = cx.update(|_, cx| {
            cx.global::<settings::project::CurrentProject>()
                .data
                .values
                .get(settings::project::FILES_FOLDER_KEY)
                .map(|v| v.text().to_string())
        });
        assert_eq!(folder, Some(dir.join("new").to_string_lossy().into_owned()));
    }

    /// Locating one row's file offers the other missing files beside it, and the whole relink,
    /// cells and `source_path` together, comes back with one undo.
    #[gpui::test]
    fn locating_a_file_relinks_its_neighbours_as_one_undo_step(cx: &mut TestAppContext) {
        let dir = tempdir("locate");
        touch(&dir.join("files").join("keep.jpg"));
        touch(&dir.join("elsewhere").join("gone.jpg"));
        touch(&dir.join("elsewhere").join("b.jpg"));
        let (panel, cx) = open(
            cx,
            &dir.join("files"),
            &["gone.jpg", "b.jpg", "keep.jpg"],
            &[],
        );

        panel.update_in(cx, |panel, window, cx| panel.locate_file(0, window, cx));
        cx.simulate_path_prompt_response(|_| Some(vec![dir.join("elsewhere").join("gone.jpg")]));
        cx.run_until_parked();
        cx.simulate_prompt_answer("Relink all");
        cx.run_until_parked();

        let linked = |name: &str| file_ingest::normalized_path(&dir.join("elsewhere").join(name));
        assert_eq!(
            cells(&panel, cx),
            [linked("gone.jpg"), linked("b.jpg"), "keep.jpg".into()]
        );
        assert_eq!(
            sources(&panel, cx),
            [Some(linked("gone.jpg")), Some(linked("b.jpg")), None]
        );

        cx.update(|_, cx| crate::history_step(false, cx));
        assert_eq!(cells(&panel, cx), ["gone.jpg", "b.jpg", "keep.jpg"]);
        assert_eq!(
            sources(&panel, cx),
            [None, None, None],
            "one undo takes it all back"
        );
    }

    #[gpui::test]
    fn a_file_dropped_from_outside_is_copied_in_when_the_project_says_so(cx: &mut TestAppContext) {
        let dir = tempdir("drop-copy");
        touch(&dir.join("files").join("keep.jpg"));
        touch(&dir.join("elsewhere").join("keep.jpg"));
        let (panel, cx) = open(
            cx,
            &dir.join("files"),
            &["", "keep.jpg"],
            &[(settings::project::IMPORT_OUTSIDE_FILES_KEY, "copy")],
        );

        panel.update_in(cx, |panel, window, cx| {
            panel.link_dropped_file(0, 0, dir.join("elsewhere").join("keep.jpg"), window, cx)
        });
        settle(cx);
        assert!(
            !cx.has_pending_prompt(),
            "the setting answers instead of a prompt"
        );
        assert_eq!(cells(&panel, cx)[0], "imported/keep.jpg");
        assert!(dir.join("files/imported/keep.jpg").is_file());
        assert_eq!(
            findings(cx, diagnostics::Severity::Error),
            0,
            "the copy is found in the folder"
        );
    }

    #[gpui::test]
    fn a_file_linked_where_it_is_stays_absolute_and_is_reported(cx: &mut TestAppContext) {
        let dir = tempdir("drop-link");
        touch(&dir.join("files").join("keep.jpg"));
        touch(&dir.join("elsewhere").join("loose.jpg"));
        let (panel, cx) = open(cx, &dir.join("files"), &["", "keep.jpg"], &[]);

        panel.update_in(cx, |panel, window, cx| {
            panel.link_dropped_file(0, 0, dir.join("elsewhere").join("loose.jpg"), window, cx)
        });
        cx.run_until_parked();
        cx.simulate_prompt_answer("Link where it is");
        settle(cx);

        let loose = file_ingest::normalized_path(&dir.join("elsewhere").join("loose.jpg"));
        assert_eq!(cells(&panel, cx)[0], loose);
        assert_eq!(sources(&panel, cx)[0].as_deref(), Some(loose.as_str()));
        assert_eq!(
            findings(cx, diagnostics::Severity::Warning),
            1,
            "reported as outside the files folder"
        );
    }

    /// With no files folder there is nothing to be outside of: the drop links, and nothing asks.
    #[gpui::test]
    fn without_a_files_folder_a_drop_never_asks(cx: &mut TestAppContext) {
        let dir = tempdir("drop-no-folder");
        touch(&dir.join("elsewhere").join("loose.jpg"));
        let (panel, cx) = open(cx, Path::new(""), &[""], &[]);

        panel.update_in(cx, |panel, window, cx| {
            panel.link_dropped_file(0, 0, dir.join("elsewhere").join("loose.jpg"), window, cx)
        });
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt());
        assert_eq!(
            cells(&panel, cx)[0],
            file_ingest::normalized_path(&dir.join("elsewhere").join("loose.jpg"))
        );
        assert_eq!(problem(cx), None, "no folder, no banner");
    }
}

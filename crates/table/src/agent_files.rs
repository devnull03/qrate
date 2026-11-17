use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ai::agent::{RequestError, Response, ResultSet, TableRevision};
use gpui::{App, Global, ParentElement as _, Styled as _, Task, div};
use gpui_component::{ActiveTheme as _, WindowExt as _, dialog::DialogButtonProps, v_flex};
use settings::history::Origin;
use settings::project::CurrentProject;

use crate::TableStateHandle;

#[derive(Clone, Debug)]
struct Grant {
    path: PathBuf,
    directory: bool,
}

impl Grant {
    fn resolve(path: &Path) -> Result<Self, RequestError> {
        let path = path
            .canonicalize()
            .map_err(|error| RequestError::FileAccessFailed(error.to_string()))?;
        let metadata = path
            .metadata()
            .map_err(|error| RequestError::FileAccessFailed(error.to_string()))?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(RequestError::FileAccessFailed(
                "Only regular files and directories can be granted".into(),
            ));
        }
        Ok(Self {
            path,
            directory: metadata.is_dir(),
        })
    }

    fn covers(&self, target: &Grant) -> bool {
        // A replaced symlink must not silently redirect an existing grant.
        self.path.canonicalize().ok().as_ref() == Some(&self.path)
            && (self.path == target.path || (self.directory && target.path.starts_with(&self.path)))
    }
}

#[derive(Default)]
struct FileGrants {
    generation: u64,
    paths: BTreeMap<(PathBuf, String), Vec<Grant>>,
}
impl Global for FileGrants {}

struct PendingAccess;
impl Global for PendingAccess {}

#[derive(Clone, Copy)]
enum Answer {
    Allow,
    Deny,
    TimedOut,
}

struct Context {
    project: PathBuf,
    revision: TableRevision,
    generation: u64,
    roots: Vec<Grant>,
    implicit: Vec<PathBuf>,
}

fn context(agent: &str, cx: &App) -> Result<Context, RequestError> {
    let project = cx
        .try_global::<CurrentProject>()
        .ok_or(RequestError::ProjectUnavailable)?;
    let state = cx
        .try_global::<TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
        .ok_or(RequestError::TableUnavailable)?;
    let mut implicit = project
        .file
        .parent()
        .map(Path::to_path_buf)
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(folder) = project.data.values.get(settings::project::FILES_FOLDER_KEY) {
        let folder = PathBuf::from(folder.text().as_ref());
        if !folder.as_os_str().is_empty() {
            implicit.push(folder);
        }
    }
    Ok(Context {
        project: project.file.clone(),
        revision: TableRevision(state.read(cx).delegate().values_generation()),
        generation: cx
            .try_global::<FileGrants>()
            .map_or(0, |grants| grants.generation),
        roots: cx
            .try_global::<FileGrants>()
            .and_then(|grants| grants.paths.get(&(project.file.clone(), agent.to_owned())))
            .cloned()
            .unwrap_or_default(),
        implicit,
    })
}

fn allowed(context: &Context, target: &Grant) -> bool {
    context.roots.iter().any(|grant| grant.covers(target))
        || context
            .implicit
            .iter()
            .filter_map(|path| Grant::resolve(path).ok())
            .any(|grant| grant.covers(target))
}

fn unchanged(project: &Path, cx: &App) -> Result<(), RequestError> {
    if cx
        .try_global::<CurrentProject>()
        .is_some_and(|current| current.file == project)
    {
        Ok(())
    } else {
        Err(RequestError::ProjectUnavailable)
    }
}

fn response(grant: Grant, revision: TableRevision) -> Response {
    Response {
        revision,
        result: ResultSet::FileAccess {
            path: grant.path.to_string_lossy().into_owned(),
            directory: grant.directory,
        },
    }
}

pub(super) fn check_access(
    agent: &str,
    path: String,
    cx: &mut App,
) -> Task<Result<Response, RequestError>> {
    let context = match context(agent, cx) {
        Ok(context) => context,
        Err(error) => return Task::ready(Err(error)),
    };
    let task = cx.background_executor().spawn(async move {
        let grant = Grant::resolve(&input_path(&context, &path))?;
        if !allowed(&context, &grant) {
            return Err(RequestError::FileAccessRequired(path));
        }
        Ok::<_, RequestError>((context, grant))
    });
    cx.spawn(async move |cx| {
        let (context, grant) = task.await?;
        cx.update(|cx| {
            unchanged(&context.project, cx)?;
            if cx
                .try_global::<FileGrants>()
                .map_or(0, |grants| grants.generation)
                != context.generation
            {
                return Err(RequestError::FileAccessDenied);
            }
            Ok(response(grant, context.revision))
        })
    })
}

fn input_path(context: &Context, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        context
            .project
            .parent()
            .unwrap_or(Path::new("."))
            .join(path)
    }
}

pub(super) fn request_access(
    agent: &str,
    path: String,
    reason: String,
    cx: &mut App,
) -> Task<Result<Response, RequestError>> {
    let context = match context(agent, cx) {
        Ok(context) => context,
        Err(error) => return Task::ready(Err(error)),
    };
    let background = cx.background_executor().spawn(async move {
        let grant = Grant::resolve(&input_path(&context, &path))?;
        let existing = allowed(&context, &grant);
        Ok::<_, RequestError>((context, grant, existing))
    });
    let agent = agent.to_owned();
    cx.spawn(async move |cx| {
        let (context, grant, existing) = background.await?;
        if existing {
            return cx.update(|cx| {
                unchanged(&context.project, cx)?;
                if cx.try_global::<FileGrants>().map_or(0, |grants| grants.generation) != context.generation { return Err(RequestError::FileAccessDenied); }
                Ok(response(grant, context.revision))
            });
        }
        let (answer, answers) = async_channel::bounded(1);
        cx.update(|cx| {
            unchanged(&context.project, cx)?;
            if cx.has_global::<PendingAccess>() { return Err(RequestError::FileAccessPending); }
            let window = cx.active_window().ok_or(RequestError::NoWindow)?;
            let path = grant.path.to_string_lossy().into_owned();
            let title = format!("Allow {agent} to read this {}?", if grant.directory { "folder" } else { "file" });
            let send = answer.clone();
            window.update(cx, move |_, window, cx| {
                window.open_alert_dialog(cx, move |dialog, _, cx| {
                    let muted = cx.theme().muted_foreground;
                    let (yes, no, close) = (send.clone(), send.clone(), send.clone());
                    dialog.title(title.clone())
                        .description(v_flex().gap_2()
                            .child(div().child(path.clone()))
                            .child(div().child(reason.clone()))
                            .child(div().text_sm().text_color(muted).child("Read only. Access lasts until qrate closes. Revoke it from the Agent panel’s session menu.")))
                        .button_props(DialogButtonProps::default().ok_text("Allow read access").show_cancel(true).cancel_text("Deny"))
                        .on_ok(move |_, _, _| { let _ = yes.try_send(Answer::Allow); true })
                        .on_cancel(move |_, _, _| { let _ = no.try_send(Answer::Deny); true })
                        .on_close(move |_, _, _| { let _ = close.try_send(Answer::Deny); })
                });
            }).map_err(|_| RequestError::NoWindow)?;
            cx.set_global(PendingAccess);
            Ok::<_, RequestError>(())
        })?;
        let timer = cx.background_executor().timer(Duration::from_secs(10 * 60));
        cx.background_executor().spawn(async move {
            timer.await;
            let _ = answer.try_send(Answer::TimedOut);
        }).detach();
        let answer = answers.recv().await.unwrap_or(Answer::Deny);
        cx.update(|cx| {
            cx.remove_global::<PendingAccess>();
            unchanged(&context.project, cx)?;
            match answer {
                Answer::Allow => {
                    let grants = cx.default_global::<FileGrants>();
                    grants.generation += 1;
                    grants.paths.entry((context.project, agent.clone())).or_default().push(grant.clone());
                    log::info!("Read access granted to {agent}");
                    Ok(response(grant, context.revision))
                }
                Answer::Deny => Err(RequestError::FileAccessDenied),
                Answer::TimedOut => { if let Some(window) = cx.active_window() { let _ = window.update(cx, |_, window, cx| window.close_all_dialogs(cx)); } Err(RequestError::FileAccessTimedOut) }
            }
        })
    })
}

pub(super) fn forget_access(agent: &str, cx: &mut App) -> Result<Response, RequestError> {
    let context = context(agent, cx)?;
    let grants = cx.default_global::<FileGrants>();
    grants.generation += 1;
    let count = grants
        .paths
        .remove(&(context.project, agent.to_owned()))
        .map_or(0, |grants| grants.len());
    Ok(Response {
        revision: context.revision,
        result: ResultSet::FileAccessRevoked { count },
    })
}

/// Revoke explicit read grants for all agents; project files remain readable as before.
pub fn revoke_agent_file_access(cx: &mut App) {
    let grants = cx.default_global::<FileGrants>();
    grants.generation += 1;
    grants.paths.clear();
    log::info!("The archivist revoked agent file access");
}

pub(super) fn import_files(
    agent: &str,
    revision: TableRevision,
    paths: Vec<String>,
    recursive: bool,
    cx: &mut App,
) -> Task<Result<Response, RequestError>> {
    let context = match context(agent, cx) {
        Ok(context) => context,
        Err(error) => return Task::ready(Err(error)),
    };
    if revision != context.revision {
        return Task::ready(Err(RequestError::StaleRevision));
    }
    let config = settings::description::DescriptionConfig::from_values(
        &cx.global::<CurrentProject>().data.values,
    );
    let task = cx.background_executor().spawn(async move {
        let mut inputs = Vec::with_capacity(paths.len());
        for path in paths {
            let grant = Grant::resolve(&input_path(&context, &path))?;
            if !allowed(&context, &grant) {
                return Err(RequestError::FileAccessRequired(path));
            }
            inputs.push(grant.path);
        }
        let plan = file_ingest::plan_paths(
            &inputs,
            &file_ingest::PlanOptions {
                recursive,
                include_root: true,
                folder_level_key: &config.folder_level_key,
                file_level_key: &config.file_level_key,
            },
        )
        .map_err(|error| RequestError::FileImportFailed(format!("{error:?}")))?;
        if plan.components.len() > ai::agent::MAX_NEW_ROWS {
            return Err(RequestError::TooManyNewRows);
        }
        for component in &plan.components {
            let grant = Grant::resolve(&component.absolute_path)?;
            if !allowed(&context, &grant) {
                return Err(RequestError::FileAccessRequired(
                    component.absolute_path.to_string_lossy().into_owned(),
                ));
            }
        }
        Ok::<_, RequestError>((context, plan))
    });
    let agent = agent.to_owned();
    cx.spawn(async move |cx| {
        let (context, plan) = task.await?;
        cx.update(|cx| {
            unchanged(&context.project, cx)?;
            if cx
                .try_global::<FileGrants>()
                .map_or(0, |grants| grants.generation)
                != context.generation
            {
                return Err(RequestError::FileAccessDenied);
            }
            let state = cx
                .global::<TableStateHandle>()
                .0
                .upgrade()
                .ok_or(RequestError::TableUnavailable)?;
            if TableRevision(state.read(cx).delegate().values_generation()) != revision {
                return Err(RequestError::StaleRevision);
            }
            let current = cx.global::<CurrentProject>();
            let folder = current
                .data
                .values
                .get(settings::project::FILES_FOLDER_KEY)
                .map(|value| PathBuf::from(value.text().as_ref()));
            let delegate = state.read(cx).delegate();
            let title = (0..delegate.column_count())
                .find(|col| delegate.column_type(*col) == settings::columns::ColumnType::Title);
            let file = (0..delegate.column_count())
                .find(|col| delegate.column_type(*col) == settings::columns::ColumnType::Filename);
            let existing = delegate.existing_components(file, folder.as_deref());
            let resolved = file_ingest::duplicates::resolve(
                plan,
                &existing,
                settings::filenames::keys,
                file_ingest::duplicates::DuplicatePolicy::Skip,
            );
            let at = delegate.row_count();
            let skipped = resolved
                .resolutions
                .iter()
                .filter(|resolution| {
                    matches!(resolution, file_ingest::duplicates::Resolution::Skip { .. })
                })
                .count();
            let warnings = resolved
                .plan
                .warnings
                .iter()
                .take(20)
                .map(|warning| format!("{warning:?}"))
                .collect();
            let origin = Origin::Agent {
                agent: agent.clone(),
                fix: "Import linked files".into(),
            };
            let added = state.update(cx, |state, cx| {
                let added = state.delegate_mut().append_components(
                    &resolved,
                    title,
                    file,
                    None,
                    folder.as_deref(),
                    origin.clone(),
                );
                crate::photos::refresh(state, None, cx);
                state.refresh(cx);
                cx.emit(crate::TableChanged);
                cx.notify();
                added
            });
            let ids = state.read(cx).delegate().row_ids().to_vec();
            diagnostics::Diagnostics::align_note_rows(diagnostics::DATASET_MAIN, &ids, origin, cx);
            settings::dirty::mark(settings::dirty::PROJECT_DATA, cx);
            crate::revalidate_now(cx);
            crate::autosave(cx);
            Ok(Response {
                revision: TableRevision(state.read(cx).delegate().values_generation()),
                result: ResultSet::ImportedFiles {
                    added,
                    skipped,
                    rows: (at..at + added).collect(),
                    warnings,
                },
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{
        FileGrants, Grant, check_access, import_files, request_access, revoke_agent_file_access,
    };
    use crate::{TablePanel, TableStateHandle};
    use ai::agent::{RequestError, ResultSet, TableRevision};
    use gpui::{AppContext as _, TestAppContext};
    use settings::project::{CurrentProject, ProjectColumn, ProjectSpec};

    struct DialogHost(gpui::Entity<TablePanel>);
    impl gpui::Render for DialogHost {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{ParentElement as _, Styled as _};
            gpui::div()
                .size_full()
                .child(self.0.clone())
                .children(gpui_component::Root::render_dialog_layer(window, cx))
        }
    }

    fn open(case: &str, cx: &mut TestAppContext) -> (std::path::PathBuf, gpui::AnyWindowHandle) {
        let root = std::env::temp_dir()
            .join("qrate-agent-file-access")
            .join(case);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        for name in ["one.txt", "two.txt"] {
            std::fs::write(root.join("outside").join(name), name).unwrap();
        }
        let file = root.join("project/p.qrate");
        let columns = vec![
            ProjectColumn {
                name: "Title".into(),
                data_type: "Title".into(),
                notes: String::new(),
            },
            ProjectColumn {
                name: "File".into(),
                data_type: "Filename".into(),
                notes: String::new(),
            },
        ];
        let headers = vec!["Title".to_owned(), "File".to_owned()];
        let rows = vec![vec!["Existing".to_owned(), "existing.txt".to_owned()]];
        settings::project::create_project_file(
            &file,
            &ProjectSpec {
                name: "Access test",
                source: "CSV",
                columns: &columns,
                headers: &headers,
                rows: &rows,
                ..Default::default()
            },
        )
        .unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(CurrentProject {
                file: file.clone(),
                data: settings::project::load_project_file(&file).unwrap(),
            });
        });
        let window = cx.add_window(|window, cx| {
            let table = cx.new(|cx| TablePanel::new(window, cx));
            window.activate_window();
            let host = cx.new(|_| DialogHost(table));
            gpui_component::Root::new(host, window, cx)
        });
        cx.run_until_parked();
        (root, window.into())
    }

    fn revision(cx: &gpui::App) -> TableRevision {
        let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
        TableRevision(state.read(cx).delegate().values_generation())
    }

    #[gpui::test]
    async fn read_access_needs_consent_and_can_be_revoked(cx: &mut TestAppContext) {
        let (root, window) = open("consent", cx);
        let target = root.join("outside").to_string_lossy().into_owned();
        let denied = cx.update(|cx| check_access("pi", target.clone(), cx));
        assert!(matches!(
            denied.await,
            Err(RequestError::FileAccessRequired(_))
        ));
        let request = cx.update(|cx| {
            request_access("pi", target.clone(), "Read files for new rows".into(), cx)
        });
        cx.run_until_parked();
        {
            let visual = &mut gpui::VisualTestContext::from_window(window, cx);
            visual.simulate_keystrokes("escape");
            visual.run_until_parked();
        }
        assert_eq!(request.await, Err(RequestError::FileAccessDenied));
        let request = cx.update(|cx| {
            request_access("pi", target.clone(), "Read files for new rows".into(), cx)
        });
        cx.run_until_parked();
        {
            let visual = &mut gpui::VisualTestContext::from_window(window, cx);
            visual.simulate_keystrokes("enter");
            visual.run_until_parked();
        }
        assert!(matches!(
            request.await.unwrap().result,
            ResultSet::FileAccess {
                directory: true,
                ..
            }
        ));
        let child = root.join("outside/one.txt").to_string_lossy().into_owned();
        let check = cx.update(|cx| check_access("pi", child.clone(), cx));
        assert!(check.await.is_ok());
        let other = cx.update(|cx| check_access("codex", child.clone(), cx));
        assert!(matches!(
            other.await,
            Err(RequestError::FileAccessRequired(_))
        ));
        cx.update(revoke_agent_file_access);
        let check = cx.update(|cx| check_access("pi", child, cx));
        assert!(matches!(
            check.await,
            Err(RequestError::FileAccessRequired(_))
        ));
    }

    #[gpui::test]
    async fn importing_files_reuses_attachment_and_duplicate_handling_and_logs_agent(
        cx: &mut TestAppContext,
    ) {
        let (root, _) = open("import", cx);
        cx.update(|cx| {
            let file = cx.global::<CurrentProject>().file.clone();
            cx.default_global::<FileGrants>().paths.insert(
                (file, "pi".into()),
                vec![Grant::resolve(&root.join("outside")).unwrap()],
            );
        });
        let paths = ["one.txt", "two.txt"]
            .map(|name| {
                root.join("outside")
                    .join(name)
                    .to_string_lossy()
                    .into_owned()
            })
            .to_vec();
        let imported = cx.update(|cx| import_files("pi", revision(cx), paths.clone(), false, cx));
        assert!(matches!(
            imported.await.unwrap().result,
            ResultSet::ImportedFiles {
                added: 2,
                skipped: 0,
                ..
            }
        ));
        cx.run_until_parked();
        cx.update(|cx| {
            let file = &cx.global::<CurrentProject>().file;
            let history = settings::history::entries_after(file, 0).unwrap();
            assert!(matches!(&history.last().unwrap().origin, settings::history::Origin::Agent { agent, .. } if agent == "pi"));
            let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
            assert_eq!(state.read(cx).delegate().row_count(), 3);
            assert!(state.read(cx).delegate().row_structure().iter().filter(|row| row.source_path.is_some()).count() >= 2);
        });
        let repeated = cx.update(|cx| import_files("pi", revision(cx), paths, false, cx));
        assert!(matches!(
            repeated.await.unwrap().result,
            ResultSet::ImportedFiles {
                added: 0,
                skipped: 2,
                ..
            }
        ));
        cx.update(|cx| crate::history_step(false, cx));
        cx.update(|cx| {
            let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
            assert_eq!(
                state.read(cx).delegate().row_count(),
                1,
                "one undo removes the whole import"
            );
        });
    }

    #[gpui::test]
    async fn an_import_refuses_revoked_access_and_a_stale_table(cx: &mut TestAppContext) {
        let (root, _) = open("stale-and-revoked", cx);
        let path = root.join("outside/one.txt").to_string_lossy().into_owned();
        let denied =
            cx.update(|cx| import_files("pi", revision(cx), vec![path.clone()], false, cx));
        assert!(matches!(
            denied.await,
            Err(RequestError::FileAccessRequired(_))
        ));
        cx.update(|cx| {
            let file = cx.global::<CurrentProject>().file.clone();
            cx.default_global::<FileGrants>().paths.insert(
                (file, "pi".into()),
                vec![Grant::resolve(&root.join("outside")).unwrap()],
            );
        });
        let revoked = cx.update(|cx| {
            let task = import_files("pi", revision(cx), vec![path.clone()], false, cx);
            revoke_agent_file_access(cx);
            task
        });
        assert_eq!(revoked.await, Err(RequestError::FileAccessDenied));
        cx.update(|cx| {
            let file = cx.global::<CurrentProject>().file.clone();
            cx.default_global::<FileGrants>().paths.insert(
                (file, "pi".into()),
                vec![Grant::resolve(&root.join("outside")).unwrap()],
            );
        });
        let stale = cx.update(|cx| {
            let task = import_files("pi", revision(cx), vec![path], false, cx);
            crate::structural(
                crate::Structural::RenameColumn {
                    col: 0,
                    name: "Caption".into(),
                },
                settings::history::Origin::Structure,
                cx,
            );
            task
        });
        assert_eq!(stale.await, Err(RequestError::StaleRevision));
        cx.update(|cx| {
            let state = cx.global::<TableStateHandle>().0.upgrade().unwrap();
            assert_eq!(state.read(cx).delegate().row_count(), 1);
        });
    }

    #[test]
    fn grants_are_canonical_and_file_grants_do_not_expand_to_siblings() {
        let root = std::env::temp_dir().join("qrate-grant-boundary");
        std::fs::create_dir_all(root.join("allowed")).unwrap();
        std::fs::create_dir_all(root.join("allowed-extra")).unwrap();
        std::fs::write(root.join("allowed/one.txt"), "one").unwrap();
        std::fs::write(root.join("allowed/two.txt"), "two").unwrap();
        let directory = Grant::resolve(&root.join("allowed")).unwrap();
        let file = Grant::resolve(&root.join("allowed/one.txt")).unwrap();
        assert!(directory.covers(&file));
        assert!(!directory.covers(&Grant::resolve(&root.join("allowed-extra")).unwrap()));
        assert!(!file.covers(&Grant::resolve(&root.join("allowed/two.txt")).unwrap()));
        #[cfg(unix)]
        {
            let link = root.join("allowed/escape");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(root.join("allowed-extra"), &link).unwrap();
            assert!(!directory.covers(&Grant::resolve(&link).unwrap()));
        }
    }
}

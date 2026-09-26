//! Watching the files folder while the project is open: files that appear, change, move or vanish
//! update the walk rows resolve against, and files no row links to are counted for the status
//! bar's "New files" button.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use file_ingest::duplicates::ExistingComponent;
use gpui::*;
use notify::Watcher as _;
use settings::columns::ColumnType;
use settings::project::{CurrentProject, IGNORED_NEW_FILES_KEY};

use super::TablePanel;
use super::files::files_folder;
use crate::photos::{self, PhotoIndex, comparable};

/// How long the folder has to be quiet before a burst of changes is read, and how long a new
/// file's size and modified time must hold still before it counts as written.
pub const SETTLE: Duration = Duration::from_millis(500);

/// Files in the files folder that no row links to and that the archivist has not ignored.
#[derive(Default)]
pub struct NewFiles {
    pub paths: Vec<PathBuf>,
    /// Files and folders qrate copied in itself this session, as [`comparable`] paths.
    own: Vec<String>,
    /// Copies into the files folder still running. Nothing is counted while one is.
    copying: usize,
    generation: u64,
}

impl Global for NewFiles {}

impl NewFiles {
    /// A copy into the files folder starts, or ends having placed `copied`.
    pub(super) fn copying(started: bool, copied: &[PathBuf], cx: &mut App) {
        let new_files = cx.default_global::<NewFiles>();
        match started {
            true => new_files.copying += 1,
            false => new_files.copying = new_files.copying.saturating_sub(1),
        }
        new_files
            .own
            .extend(copied.iter().map(|path| comparable(path)));
        if !started {
            recount(cx);
        }
    }

    /// Take `paths` out of the count, once imported or, with `ignore`, for good.
    pub(super) fn settle(paths: &[PathBuf], ignore: bool, cx: &mut App) {
        let settled: HashSet<String> = paths.iter().map(|path| comparable(path)).collect();
        cx.default_global::<NewFiles>()
            .paths
            .retain(|path| !settled.contains(&comparable(path)));
        if !ignore {
            return;
        }
        let root = PathBuf::from(files_folder(cx));
        let mut ignored = ignored(cx);
        ignored.extend(
            paths
                .iter()
                .map(|path| crate::relink::link_text(&root, path)),
        );
        ignored.sort_unstable();
        ignored.dedup();
        log::info!("ignoring {} new file(s) in the files folder", paths.len());
        CurrentProject::set_text(
            IGNORED_NEW_FILES_KEY,
            serde_json::to_string(&ignored)
                .unwrap_or_else(|_| "[]".into())
                .into(),
            cx,
        );
    }
}

/// The paths the project keeps out of the "New files" count, relative to the files folder.
fn ignored(cx: &App) -> Vec<String> {
    cx.try_global::<CurrentProject>()
        .and_then(|project| project.data.values.get(IGNORED_NEW_FILES_KEY))
        .and_then(|value| serde_json::from_str(&value.text()).ok())
        .unwrap_or_default()
}

/// The watcher on one files folder, and the changes it has reported that are not read yet.
pub(super) struct FolderWatch {
    folder: String,
    watcher: Option<notify::RecommendedWatcher>,
    pending: HashSet<PathBuf>,
    /// Files seen once, with what they looked like, waiting to look the same a second time.
    unsettled: HashMap<PathBuf, Stamp>,
    _events: Task<()>,
    reading: Option<Task<()>>,
}

type Stamp = (u64, Option<SystemTime>);

/// What one read of the pending changes found.
struct Read {
    unsettled: HashMap<PathBuf, Stamp>,
    /// Files still being written, to be looked at again.
    pending: Vec<PathBuf>,
    /// Files that settled or went away, whose previews are stale.
    touched: Vec<PathBuf>,
    /// The walk it was measured against, and that walk with the changes applied.
    index: Option<(Arc<PhotoIndex>, PhotoIndex)>,
}

/// Names that are never an archivist's file: hidden files and folders, OS and Office litter, and
/// downloads or copies that are not finished.
pub(crate) fn is_transient(relative: &Path) -> bool {
    let hidden = relative.components().any(|part| {
        let name = part.as_os_str().to_string_lossy();
        name.starts_with('.') || name.starts_with("~$") || file_ingest::is_ignored(&name)
    });
    let name = relative
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    hidden
        || matches!(name.as_str(), "thumbs.db" | "desktop.ini")
        || name.ends_with('~')
        || [".tmp", ".part", ".partial", ".crdownload", ".download"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

/// Whether `path` looks as it did at the last look, remembering how it looks now when not.
fn settled(unsettled: &mut HashMap<PathBuf, Stamp>, path: &Path, now: Stamp) -> bool {
    let same = unsettled.get(path) == Some(&now);
    match same {
        true => unsettled.remove(path),
        false => unsettled.insert(path.to_path_buf(), now),
    };
    same
}

fn stamp(meta: &std::fs::Metadata) -> Stamp {
    (meta.len(), meta.modified().ok())
}

/// Look at every changed path under `root` and apply what has settled to `index`. A folder is
/// compared with the walk: files under it the walk lacks are new, and walked files it no longer
/// holds are gone. Blocks on the file system.
fn read(
    root: &Path,
    paths: Vec<PathBuf>,
    mut unsettled: HashMap<PathBuf, Stamp>,
    index: Option<Arc<PhotoIndex>>,
) -> Read {
    let held: HashSet<String> = index
        .iter()
        .flat_map(|index| index.files.iter().map(|file| comparable(file)))
        .collect();
    let (mut added, mut removed, mut pending, mut touched) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for path in paths {
        let Some(relative) = qrate_export::relative_to(root, &path) else {
            continue;
        };
        let Ok(meta) = std::fs::metadata(&path) else {
            unsettled.retain(|file, _| !file.starts_with(&path));
            removed.push(relative);
            touched.push(path);
            continue;
        };
        let candidates: Vec<PathBuf> = match (meta.is_dir(), &index) {
            (false, _) => vec![path],
            (true, None) => Vec::new(),
            (true, Some(index)) => {
                let found: Vec<PathBuf> = file_ingest::scan(&path, true)
                    .map(|inventory| {
                        inventory
                            .files()
                            .filter_map(|entry| {
                                qrate_export::relative_to(root, &path.join(&entry.relative_path))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let on_disk: HashSet<String> = found.iter().map(|file| comparable(file)).collect();
                let folder = comparable(&relative);
                for file in &index.files {
                    let file_key = comparable(file);
                    if photos::within(&file_key, &folder) && !on_disk.contains(&file_key) {
                        removed.push(file.clone());
                        touched.push(root.join(file));
                    }
                }
                found
                    .into_iter()
                    .filter(|file| !held.contains(&comparable(file)))
                    .map(|file| root.join(file))
                    .collect()
            }
        };
        for file in candidates {
            let Some(relative) = qrate_export::relative_to(root, &file) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&file) else {
                continue;
            };
            if is_transient(&relative) {
                continue;
            }
            match settled(&mut unsettled, &file, stamp(&meta)) {
                true => {
                    added.push(relative);
                    touched.push(file);
                }
                false => pending.push(file),
            }
        }
    }
    let index = index.and_then(|index| Some((index.clone(), index.changed(&added, &removed)?)));
    Read {
        unsettled,
        pending,
        touched,
        index,
    }
}

/// Start watching `root`, sending the paths of every change to `events`.
fn start(
    root: &Path,
    events: std::sync::mpsc::Sender<Vec<PathBuf>>,
) -> notify::Result<notify::RecommendedWatcher> {
    let whole = root.to_path_buf();
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event) if event.need_rescan() => {
                events.send(vec![whole.clone()]).ok();
            }
            Ok(event) if !event.kind.is_access() => {
                events.send(event.paths).ok();
            }
            Ok(_) => {}
            Err(error) => log::warn!("The files folder watcher reported an error: {error}"),
        })?;
    watcher.watch(root, notify::RecursiveMode::Recursive)?;
    Ok(watcher)
}

/// The files under `root` (`files` relative to it) that no row links to, less `skip`: ignored
/// files and qrate's own copies, as [`comparable`] paths of files or folders. Sorted.
///
/// A row links a file the way an import would find it again: by the path it was imported from,
/// or by a name the file answers to. The file a row is showing counts too, which covers a project
/// whose file column is not declared.
pub(crate) fn unclaimed(
    root: &Path,
    files: &[PathBuf],
    existing: &[ExistingComponent],
    shown: &[PathBuf],
    skip: &[String],
) -> Vec<PathBuf> {
    let names: HashSet<&str> = existing
        .iter()
        .flat_map(|component| component.filename_keys.iter().map(String::as_str))
        .collect();
    let linked: HashSet<String> = existing
        .iter()
        .filter_map(|component| component.absolute_source.as_deref())
        .chain(shown.iter().map(PathBuf::as_path))
        .map(comparable)
        .collect();
    let mut found: Vec<PathBuf> = files
        .iter()
        .filter(|relative| !is_transient(relative))
        .filter(|relative| {
            let name = relative
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            !std::iter::once(comparable(relative))
                .chain(settings::filenames::keys(&name))
                .any(|key| names.contains(key.as_str()))
        })
        .map(|relative| root.join(relative))
        .filter(|path| {
            let path = comparable(path);
            !linked.contains(&path) && !skip.iter().any(|skip| photos::within(&path, skip))
        })
        .collect();
    found.sort_unstable();
    found
}

/// Count the files folder's new files again, off the UI thread, from the cached walk. Nothing
/// changes before the folder has been walked or while qrate is copying files into it.
pub(crate) fn recount(cx: &mut App) {
    let folder = files_folder(cx);
    if folder.trim().is_empty() {
        if cx
            .try_global::<NewFiles>()
            .is_some_and(|n| !n.paths.is_empty())
        {
            cx.global_mut::<NewFiles>().paths.clear();
        }
        return;
    }
    let Some(index) = photos::cached_index(&folder, cx) else {
        return;
    };
    if cx.try_global::<NewFiles>().is_some_and(|n| n.copying > 0) {
        return;
    }
    let Some(state) = cx
        .try_global::<crate::TableStateHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    let root = PathBuf::from(&folder);
    let delegate = state.read(cx).delegate();
    let existing: Vec<ExistingComponent> = std::iter::once(None)
        .chain(
            (0..delegate.column_count())
                .filter(|&col| delegate.column_type(col) == ColumnType::Filename)
                .map(Some),
        )
        .flat_map(|col| delegate.existing_components(col, Some(&root)))
        .collect();
    let shown = delegate.linked_files();
    let ignored: Vec<String> = ignored(cx)
        .iter()
        .map(|relative| comparable(&root.join(relative)))
        .collect();
    let new_files = cx.default_global::<NewFiles>();
    new_files.generation += 1;
    let generation = new_files.generation;
    let skip: Vec<String> = ignored.into_iter().chain(new_files.own.clone()).collect();
    cx.spawn(async move |cx| {
        let paths = cx
            .background_executor()
            .spawn(async move { unclaimed(&root, &index.files, &existing, &shown, &skip) })
            .await;
        cx.update(|cx| {
            let current = cx.global::<NewFiles>();
            if current.generation == generation && current.paths != paths {
                cx.global_mut::<NewFiles>().paths = paths;
            }
        });
    })
    .detach();
}

impl TablePanel {
    /// Watch the open project's files folder, replacing the watcher when the folder changes and
    /// dropping it when there is none.
    pub(super) fn sync_watch(&mut self, cx: &mut Context<Self>) {
        let folder = files_folder(cx);
        if self
            .watch
            .as_ref()
            .map_or("", |watch| watch.folder.as_str())
            == folder
        {
            return;
        }
        self.watch = None;
        cx.set_global(NewFiles::default());
        if folder.trim().is_empty() {
            return;
        }
        let root = PathBuf::from(&folder);
        let events = cx.spawn(async move |this, cx| {
            let (sender, events) = std::sync::mpsc::channel();
            let started = cx
                .background_executor()
                .spawn(async move { root.is_dir().then(|| (start(&root, sender), root)) })
                .await;
            let watcher = match started {
                None => return,
                Some((Ok(watcher), _)) => watcher,
                Some((Err(error), root)) => {
                    log::warn!(
                        "Could not watch the files folder {}; new and removed files will show \
                         after the project is reopened: {error}",
                        root.display()
                    );
                    return;
                }
            };
            let kept = this.update(cx, |this, _| {
                if let Some(watch) = this.watch.as_mut() {
                    watch.watcher = Some(watcher);
                }
            });
            if kept.is_err() {
                return;
            }
            loop {
                cx.background_executor().timer(SETTLE).await;
                let paths: Vec<PathBuf> = events.try_iter().flatten().collect();
                if this.update(cx, |this, cx| this.watched(paths, cx)).is_err() {
                    break;
                }
            }
        });
        self.watch = Some(FolderWatch {
            folder,
            watcher: None,
            pending: HashSet::new(),
            unsettled: HashMap::new(),
            _events: events,
            reading: None,
        });
    }

    /// One [`SETTLE`] of the watcher's reports. Changes are read once a whole one passes without
    /// any, and files still being written wait for the next quiet one.
    pub(super) fn watched(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(watch) = self.watch.as_mut() else {
            return;
        };
        let root = Path::new(&watch.folder);
        let changed: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| {
                qrate_export::relative_to(root, path)
                    .is_some_and(|relative| !is_transient(&relative))
            })
            .collect();
        let quiet = changed.is_empty();
        watch.pending.extend(changed);
        if !quiet || watch.pending.is_empty() || watch.reading.is_some() {
            return;
        }
        let folder = watch.folder.clone();
        let paths: Vec<PathBuf> = watch.pending.drain().collect();
        let unsettled = std::mem::take(&mut watch.unsettled);
        let index = photos::cached_index(&folder, cx);
        watch.reading = Some(cx.spawn(async move |this, cx| {
            let root = PathBuf::from(&folder);
            let read = cx
                .background_executor()
                .spawn(async move { read(&root, paths, unsettled, index) })
                .await;
            this.update(cx, |this, cx| this.apply_watched(folder, read, cx))
                .ok();
        }));
    }

    /// Settled changes reach the walk, the previews and the checks; unsettled ones wait again.
    fn apply_watched(&mut self, folder: String, read: Read, cx: &mut Context<Self>) {
        let Some(watch) = self.watch.as_mut().filter(|watch| watch.folder == folder) else {
            return;
        };
        watch.reading = None;
        watch.unsettled = read.unsettled;
        watch.pending.extend(read.pending);
        for path in &read.touched {
            preview::forget(path, cx);
        }
        if let Some((base, index)) = read.index
            && photos::cached_index(&folder, cx).is_some_and(|now| Arc::ptr_eq(&now, &base))
        {
            log::debug!(
                "files folder now holds {} files after {} change(s)",
                index.files.len(),
                read.touched.len()
            );
            photos::remember(folder, Arc::new(index), cx);
            self.state
                .update(cx, |state, cx| photos::refresh(state, None, cx));
            self.schedule_revalidate(cx);
        }
    }

    /// The status bar's "New files" button: the import prompt, for exactly the files it counts.
    pub fn import_new_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx
            .try_global::<NewFiles>()
            .map(|new_files| new_files.paths.clone())
            .unwrap_or_default();
        if !paths.is_empty() {
            self.import_paths(paths, true, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use file_ingest::duplicates::ExistingComponent;

    use super::super::files::tests as files;
    use super::{is_transient, read, start, unclaimed};
    use crate::photos::PhotoIndex;

    fn tempdir(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("qrate-watch-test").join(case);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn litter_and_unfinished_files_are_never_counted() {
        for name in [
            ".DS_Store",
            "._1.jpg",
            ".hidden.jpg",
            ".git/objects/ab",
            "~$report.docx",
            "scan.tmp",
            "scan.jpg.part",
            "scan.jpg.crdownload",
            "Thumbs.db",
            "box/desktop.ini",
            "notes.txt~",
        ] {
            assert!(is_transient(Path::new(name)), "{name}");
        }
        for name in ["1.jpg", "box/2020_04_001.tif", "part.jpg", "temp/1.jpg"] {
            assert!(!is_transient(Path::new(name)), "{name}");
        }
    }

    /// A file counts only once two looks agree on its size and time; one still growing waits.
    #[test]
    fn a_file_is_added_once_it_holds_still() {
        let dir = tempdir("settle");
        touch(&dir.join("old.jpg"), "x");
        let index = Arc::new(PhotoIndex::build(dir.to_str().unwrap()));
        let file = dir.join("new.jpg");
        touch(&file, "first");

        let first = read(
            &dir,
            vec![file.clone()],
            HashMap::new(),
            Some(index.clone()),
        );
        assert_eq!(
            first.pending,
            std::slice::from_ref(&file),
            "seen once is not settled"
        );
        assert!(first.index.is_none());

        std::fs::write(&file, "first and more").unwrap();
        let second = read(&dir, first.pending, first.unsettled, Some(index.clone()));
        assert_eq!(
            second.pending,
            std::slice::from_ref(&file),
            "it grew, so it waits again"
        );

        let third = read(&dir, second.pending, second.unsettled, Some(index.clone()));
        assert!(third.pending.is_empty());
        let (base, changed) = third.index.expect("the walk changed");
        assert!(Arc::ptr_eq(&base, &index));
        assert!(changed.resolve_cell("new.jpg").is_some());
        assert!(changed.resolve_cell("old.jpg").is_some());
    }

    /// A folder the watcher names, or the whole folder once the OS has dropped events, is compared
    /// with the walk: what it no longer holds is gone at once, what is new is added once it holds
    /// still, and what the walk already has is left alone.
    #[test]
    fn a_rescanned_folder_is_compared_with_the_walk() {
        let dir = tempdir("rescan");
        touch(&dir.join("a.jpg"), "x");
        touch(&dir.join("b.jpg"), "x");
        let index = Arc::new(PhotoIndex::build(dir.to_str().unwrap()));
        std::fs::remove_file(dir.join("b.jpg")).unwrap();
        touch(&dir.join("c.jpg"), "x");

        let first = read(&dir, vec![dir.clone()], HashMap::new(), Some(index.clone()));
        let (_, changed) = first.index.expect("b.jpg is gone");
        assert_eq!(changed.files, paths(&["a.jpg"]));
        assert_eq!(first.pending, [dir.join("c.jpg")]);
        assert_eq!(first.touched, [dir.join("b.jpg")], "a.jpg was not touched");

        let second = read(&dir, first.pending, first.unsettled, Some(index));
        let (_, changed) = second.index.expect("c.jpg arrived");
        assert!(changed.resolve_cell("c.jpg").is_some());
    }

    /// A deleted file, or a folder moved away with its files, leaves the walk at once.
    #[test]
    fn a_removed_folder_takes_its_files_out_of_the_walk() {
        let dir = tempdir("remove");
        touch(&dir.join("box").join("1.jpg"), "x");
        touch(&dir.join("box").join("2.jpg"), "x");
        touch(&dir.join("keep.jpg"), "x");
        let index = Arc::new(PhotoIndex::build(dir.to_str().unwrap()));
        std::fs::remove_dir_all(dir.join("box")).unwrap();

        let read = read(&dir, vec![dir.join("box")], HashMap::new(), Some(index));
        let (_, changed) = read.index.expect("the walk changed");
        assert_eq!(changed.files, paths(&["keep.jpg"]));
        assert_eq!(read.touched, [dir.join("box")]);
    }

    /// A modified file that is already in the walk changes nothing about which files exist, but
    /// is still reported so its previews are dropped.
    #[test]
    fn a_rewritten_file_is_touched_without_changing_the_walk() {
        let dir = tempdir("rewrite");
        let file = dir.join("1.jpg");
        touch(&file, "x");
        let index = Arc::new(PhotoIndex::build(dir.to_str().unwrap()));
        let first = read(
            &dir,
            vec![file.clone()],
            HashMap::new(),
            Some(index.clone()),
        );
        let second = read(&dir, first.pending, first.unsettled, Some(index));
        assert!(second.index.is_none());
        assert_eq!(second.touched, [file]);
    }

    fn row(key: u64, source: Option<PathBuf>, cell: &str) -> ExistingComponent {
        ExistingComponent {
            key,
            absolute_source: source,
            filename_keys: settings::filenames::lookup_keys(cell),
        }
    }

    #[test]
    fn new_files_are_those_no_row_links_and_nobody_set_aside() {
        let root = Path::new("/archive");
        let files = paths(&[
            "named.jpg",
            "2020_04_001_002.jpg",
            "imported/by-path.jpg",
            "shown.jpg",
            "ignored.jpg",
            "imported/copied.jpg",
            "copied-folder/inner.jpg",
            "Thumbs.db",
            "fresh.jpg",
            "box/fresh.tif",
        ]);
        let existing = [
            row(1, None, "named.JPG"),
            row(2, None, "2020_04_001"),
            row(3, Some(root.join("imported/by-path.jpg")), ""),
        ];
        let shown = [root.join("shown.jpg")];
        let skip: Vec<String> = ["ignored.jpg", "imported/copied.jpg", "copied-folder"]
            .iter()
            .map(|relative| crate::photos::comparable(&root.join(relative)))
            .collect();

        assert_eq!(
            unclaimed(root, &files, &existing, &shown, &skip),
            [root.join("box/fresh.tif"), root.join("fresh.jpg")]
        );
    }

    /// The real watcher on a real folder: creating a file is reported with its path.
    #[test]
    fn the_watcher_reports_a_new_file() {
        let dir = tempdir("notify");
        let (sender, events) = std::sync::mpsc::channel();
        let _watcher = start(&dir, sender).expect("watching a temp folder");
        touch(&dir.join("arrived.jpg"), "x");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let paths = events
                .recv_timeout(left)
                .expect("an event for the new file");
            if paths.iter().any(|path| path.ends_with("arrived.jpg")) {
                return;
            }
        }
        panic!("no event for the new file");
    }

    fn new_files(cx: &mut gpui::VisualTestContext) -> Vec<PathBuf> {
        cx.update(|_, cx| {
            cx.try_global::<super::NewFiles>()
                .map(|new_files| new_files.paths.clone())
                .unwrap_or_default()
        })
    }

    /// Reported changes are read after the folder goes quiet, and a file after a second look.
    fn report(
        panel: &gpui::Entity<crate::TablePanel>,
        paths: Vec<PathBuf>,
        cx: &mut gpui::VisualTestContext,
    ) {
        panel.update(cx, |panel, cx| panel.watched(paths, cx));
        for _ in 0..4 {
            files::settle(cx);
        }
    }

    fn watching(
        panel: &gpui::Entity<crate::TablePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> Option<(String, bool)> {
        panel.read_with(cx, |panel, _| {
            panel
                .watch
                .as_ref()
                .map(|watch| (watch.folder.clone(), watch.watcher.is_some()))
        })
    }

    #[gpui::test]
    fn a_file_that_turns_up_links_its_row_and_an_unmatched_one_is_counted(
        cx: &mut gpui::TestAppContext,
    ) {
        let dir = files::tempdir("watch-arrive");
        files::touch(&dir.join("1.jpg"));
        let (panel, cx) = files::open(cx, &dir, &["1.jpg", "2.jpg"], &[]);
        assert_eq!(files::findings(cx, diagnostics::Severity::Error), 1);
        assert!(new_files(cx).is_empty(), "every file is linked");

        files::touch(&dir.join("2.jpg"));
        files::touch(&dir.join("loose.jpg"));
        files::touch(&dir.join("Thumbs.db"));
        report(
            &panel,
            vec![
                dir.join("2.jpg"),
                dir.join("loose.jpg"),
                dir.join("Thumbs.db"),
            ],
            cx,
        );
        assert_eq!(
            files::findings(cx, diagnostics::Severity::Error),
            0,
            "the row's file arrived"
        );
        assert_eq!(new_files(cx), [dir.join("loose.jpg")]);

        std::fs::remove_file(dir.join("1.jpg")).unwrap();
        report(&panel, vec![dir.join("1.jpg")], cx);
        assert_eq!(
            files::findings(cx, diagnostics::Severity::Error),
            1,
            "a deleted file is missing again"
        );
    }

    /// Ignoring the new files takes them out of the count and keeps them out, in the project.
    #[gpui::test]
    fn ignored_new_files_stay_ignored(cx: &mut gpui::TestAppContext) {
        let dir = files::tempdir("watch-ignore");
        files::touch(&dir.join("1.jpg"));
        files::touch(&dir.join("sub").join("stray.jpg"));
        let (panel, cx) = files::open(cx, &dir, &["1.jpg"], &[]);
        assert_eq!(
            new_files(cx),
            [dir.join("sub").join("stray.jpg")],
            "found by the scan on open"
        );

        panel.update_in(cx, |panel, window, cx| panel.import_new_files(window, cx));
        cx.run_until_parked();
        cx.simulate_prompt_answer("Ignore");
        cx.run_until_parked();
        assert!(new_files(cx).is_empty());
        let ignored = cx.update(|_, cx| super::ignored(cx));
        assert_eq!(ignored, ["sub/stray.jpg"]);

        cx.update(|_, cx| super::recount(cx));
        files::settle(cx);
        assert!(new_files(cx).is_empty(), "a recount does not bring it back");
        assert_eq!(files::cells(&panel, cx), ["1.jpg"], "nothing was imported");
    }

    #[gpui::test]
    fn importing_new_files_adds_their_rows(cx: &mut gpui::TestAppContext) {
        let dir = files::tempdir("watch-import");
        files::touch(&dir.join("1.jpg"));
        files::touch(&dir.join("2.jpg"));
        let (panel, cx) = files::open(cx, &dir, &["1.jpg"], &[]);
        assert_eq!(new_files(cx), [dir.join("2.jpg")]);

        panel.update_in(cx, |panel, window, cx| panel.import_new_files(window, cx));
        cx.run_until_parked();
        cx.simulate_prompt_answer("Import");
        files::settle(cx);
        assert_eq!(files::cells(&panel, cx), ["1.jpg", "2.jpg"]);
        assert!(new_files(cx).is_empty());
    }

    /// The watcher moves with the files folder and stops when the project has none.
    #[gpui::test]
    fn the_watch_follows_the_files_folder(cx: &mut gpui::TestAppContext) {
        let dir = files::tempdir("watch-lifecycle");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        let (panel, cx) = files::open(cx, &dir.join("a"), &[], &[]);
        let folder = |name: &str| dir.join(name).to_string_lossy().into_owned();
        assert_eq!(watching(&panel, cx), Some((folder("a"), true)));

        let relink = |to: String, cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| {
                settings::project::CurrentProject::set_text(
                    settings::project::FILES_FOLDER_KEY,
                    to.into(),
                    cx,
                )
            });
            cx.run_until_parked();
        };
        relink(folder("b"), cx);
        assert_eq!(watching(&panel, cx), Some((folder("b"), true)));
        relink(folder("gone"), cx);
        assert_eq!(
            watching(&panel, cx),
            Some((folder("gone"), false)),
            "a folder that is not there is not watched"
        );
        relink(String::new(), cx);
        assert_eq!(watching(&panel, cx), None);
    }

    /// A file qrate copies into `imported/` for an import is never offered as new, even while the
    /// import is still being asked about, or after it is cancelled.
    #[gpui::test]
    fn a_file_qrate_copied_in_is_not_new(cx: &mut gpui::TestAppContext) {
        let dir = files::tempdir("watch-own-copy");
        files::touch(&dir.join("files").join("1.jpg"));
        files::touch(&dir.join("elsewhere").join("dropped.jpg"));
        let (panel, cx) = files::open(
            cx,
            &dir.join("files"),
            &["1.jpg"],
            &[(settings::project::IMPORT_OUTSIDE_FILES_KEY, "copy")],
        );

        panel.update_in(cx, |panel, window, cx| {
            panel.import_external_paths(vec![dir.join("elsewhere").join("dropped.jpg")], window, cx)
        });
        cx.run_until_parked();
        let copy = dir.join("files").join("imported").join("dropped.jpg");
        assert!(copy.is_file());
        report(&panel, vec![copy], cx);
        assert!(
            new_files(cx).is_empty(),
            "not while the import is asked about"
        );

        cx.simulate_prompt_answer("Cancel");
        cx.update(|_, cx| super::recount(cx));
        files::settle(cx);
        assert!(new_files(cx).is_empty(), "nor after it is cancelled");
    }
}

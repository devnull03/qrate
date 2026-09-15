//! Visual search for the open table: installing the model, keeping its index current for the rows'
//! linked files, and turning a query into ranked rows.
//!
//! Shared across windows as a global, because the model holds 600 MB and one copy is plenty.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Global, SharedString, Task};
use settings::project::{CurrentProject, FILES_FOLDER_KEY, VisualEntry};
use visual_search::Clip;

/// Files embedded per model call. Larger batches barely help on a CPU and delay the progress count.
const BATCH: usize = 16;

/// The most rows a visual search keeps, however broad the search.
const MAX_RESULTS: usize = 500;

/// The breadth a new search starts at, and the range its slider allows.
pub(crate) const BREADTH: f32 = 0.15;
pub(crate) const BREADTH_RANGE: std::ops::RangeInclusive<f32> = 0.02..=0.4;

/// How many of `scores` (best first) to keep: those within `breadth` of the best, proportionally.
///
/// ponytail: a proportional gap from the best hit, so text queries (best around 0.3) and find
/// similar (best 1.0) share one slider. Tuned on one collection; calibrate per model if it misjudges.
pub(crate) fn cutoff(scores: impl IntoIterator<Item = f32>, breadth: f32) -> usize {
    let mut scores = scores.into_iter();
    let Some(best) = scores.next() else {
        return 0;
    };
    let floor = best * (1.0 - breadth);
    (1 + scores.take_while(|&score| score >= floor).count()).min(MAX_RESULTS)
}

#[derive(Clone, PartialEq)]
pub(crate) enum Status {
    Missing,
    Downloading(u64),
    Indexing { done: usize, total: usize },
    Ready,
    Failed(SharedString),
}

/// Each linked file's vector, with the file length it was embedded at.
///
/// ponytail: staleness is judged on length alone, so a copied project keeps its vectors when file
/// times change. Store a content hash if same-length edits turn out to matter.
type Index = HashMap<PathBuf, (u64, Vec<f32>)>;

pub(crate) struct Visual {
    pub status: Status,
    clip: Option<Arc<Clip>>,
    index: Arc<Mutex<Index>>,
    /// The `.qrate` file `index` belongs to. Another project starts from its own stored index.
    project: Option<PathBuf>,
    /// Files the indexing job has already been handed this session, so asking again is free.
    seen: HashSet<PathBuf>,
    job: Option<Task<()>>,
    /// A "find similar" request waiting for the search bar to pick it up.
    pub similar: Option<PathBuf>,
}

impl Global for Visual {}

fn model_dir() -> Option<PathBuf> {
    Some(
        settings::data_dir()?
            .join("models")
            .join(visual_search::MODEL_DIR),
    )
}

pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<Visual>() {
        return;
    }
    let installed = model_dir().is_some_and(|dir| visual_search::installed(&dir));
    cx.set_global(Visual {
        status: if installed {
            Status::Ready
        } else {
            Status::Missing
        },
        clip: None,
        index: Arc::default(),
        project: None,
        seen: HashSet::new(),
        job: None,
        similar: None,
    });
}

/// For writes only: every call notifies the global's observers, which re-run the search bar.
fn state(cx: &mut App) -> &mut Visual {
    cx.global_mut::<Visual>()
}

pub(crate) fn status(cx: &App) -> Status {
    cx.global::<Visual>().status.clone()
}

/// What the search bar says instead of a match count while visual search cannot answer yet.
pub(crate) fn status_label(status: &Status) -> Option<SharedString> {
    match status {
        Status::Missing => Some("Model not installed".into()),
        Status::Downloading(done) => Some(
            format!(
                "Downloading {}%",
                done * 100 / visual_search::DOWNLOAD_SIZE.max(1)
            )
            .into(),
        ),
        Status::Indexing { done, total } => Some(format!("Indexing {done} / {total}").into()),
        Status::Ready => None,
        Status::Failed(reason) => Some(reason.clone()),
    }
}

pub(crate) fn download_label() -> SharedString {
    format!(
        "Download model ({} MB)",
        visual_search::DOWNLOAD_SIZE / 1_000_000
    )
    .into()
}

/// Fetch the model, reporting progress into [`Status::Downloading`].
pub(crate) fn install(cx: &mut App) {
    let Some(dir) = model_dir() else {
        return;
    };
    if matches!(status(cx), Status::Downloading(_)) {
        return;
    }
    state(cx).status = Status::Downloading(0);
    let progress = Arc::new(AtomicU64::new(0));
    let finished = Arc::new(AtomicBool::new(false));
    let download = cx.background_executor().spawn({
        let (progress, finished) = (progress.clone(), finished.clone());
        async move {
            let result =
                visual_search::download(&dir, &|bytes| progress.store(bytes, Ordering::Relaxed));
            finished.store(true, Ordering::Relaxed);
            result
        }
    });
    let job = cx.spawn(async move |cx| {
        while !finished.load(Ordering::Relaxed) {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            cx.update(|cx| {
                state(cx).status = Status::Downloading(progress.load(Ordering::Relaxed))
            });
        }
        let result = download.await;
        cx.update(|cx| {
            let visual = state(cx);
            visual.job = None;
            visual.status = match result {
                Ok(()) => Status::Ready,
                Err(err) => {
                    log::error!("could not download the visual search model: {err:#}");
                    Status::Failed("Download failed".into())
                }
            };
        });
    });
    state(cx).job = Some(job);
}

/// Embed any of `paths` the index does not already hold at their current length, storing the
/// vectors in the open project. Returns at once; progress shows in [`Status::Indexing`].
pub(crate) fn index(paths: Vec<PathBuf>, cx: &mut App) {
    let project = cx.try_global::<CurrentProject>().map(|project| {
        let folder = project
            .data
            .values
            .get(FILES_FOLDER_KEY)
            .map(|v| PathBuf::from(v.text().to_string()))
            .unwrap_or_default();
        (project.file.clone(), folder)
    });
    let visual = cx.global::<Visual>();
    let Some(dir) = model_dir() else {
        return;
    };
    if visual.status != Status::Ready || visual.job.is_some() {
        return;
    }
    let switched = visual.project != project.as_ref().map(|(file, _)| file.clone());
    let fresh: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| switched || !visual.seen.contains(path))
        .collect();
    if fresh.is_empty() {
        return;
    }
    let clip = visual.clip.clone();
    let visual = state(cx);
    if switched {
        visual.project = project.as_ref().map(|(file, _)| file.clone());
        visual.seen.clear();
        visual.index = Arc::default();
    }
    visual.seen.extend(fresh.iter().cloned());
    let index = visual.index.clone();
    let job = cx.spawn(async move |cx| {
        let prepared = cx
            .background_executor()
            .spawn({
                let (index, project) = (index.clone(), project.clone());
                async move {
                    let clip = match clip {
                        Some(clip) => clip,
                        None => Arc::new(Clip::load(&dir).map_err(|err| format!("{err:#}"))?),
                    };
                    if switched && let Some((file, folder)) = &project {
                        match settings::project::read_visual_index(file, visual_search::MODEL) {
                            Ok(stored) => lock(&index).extend(
                                stored
                                    .into_iter()
                                    .map(|(path, len, vector)| (folder.join(path), (len, vector))),
                            ),
                            Err(err) => log::warn!(
                                "could not read the project's visual search index, rebuilding it: {err:#}"
                            ),
                        }
                    }
                    // Stat outside the lock: searches rank rows on the UI thread through this index.
                    let stale: Vec<(PathBuf, u64)> = fresh
                        .into_iter()
                        .filter_map(|path| {
                            let len = std::fs::metadata(&path).ok()?.len();
                            let kept = lock(&index).get(&path).map(|(kept, _)| *kept);
                            (kept != Some(len)).then_some((path, len))
                        })
                        .collect();
                    Ok::<_, String>((clip, stale))
                }
            })
            .await;
        let (clip, stale) = match prepared {
            Ok(prepared) => prepared,
            Err(err) => {
                log::error!("could not load the visual search model: {err}");
                cx.update(|cx| {
                    let visual = state(cx);
                    visual.job = None;
                    visual.status = Status::Failed("Model failed to load".into());
                });
                return;
            }
        };
        cx.update(|cx| state(cx).clip = Some(clip.clone()));

        let total = stale.len();
        for (at, batch) in stale.chunks(BATCH).enumerate() {
            cx.update(|cx| {
                state(cx).status = Status::Indexing {
                    done: at * BATCH,
                    total,
                }
            });
            let (clip, index, project, batch) =
                (clip.clone(), index.clone(), project.clone(), batch.to_vec());
            cx.background_executor()
                .spawn(async move {
                    let embedded = embed_batch(&clip, batch);
                    if let Some((file, folder)) = &project {
                        let stored: Vec<VisualEntry> = embedded
                            .iter()
                            .map(|(path, len, vector)| (stored_path(path, folder), *len, vector.clone()))
                            .collect();
                        if let Err(err) =
                            settings::project::write_visual_index(file, visual_search::MODEL, &stored)
                        {
                            log::warn!("could not save visual search vectors to the project, they will be rebuilt: {err:#}");
                        }
                    }
                    lock(&index).extend(
                        embedded
                            .into_iter()
                            .map(|(path, len, vector)| (path, (len, vector))),
                    );
                })
                .await;
        }
        if total > 0 {
            log::info!("visual search indexed {total} files");
        }
        cx.update(|cx| {
            let visual = state(cx);
            visual.job = None;
            visual.status = Status::Ready;
        });
    });
    state(cx).job = Some(job);
}

fn lock(index: &Mutex<Index>) -> std::sync::MutexGuard<'_, Index> {
    index.lock().unwrap_or_else(|err| err.into_inner())
}

/// `path` relative to the files folder with `/` separators, so a project moved with its files, or
/// to another platform, still finds its vectors. Absolute when the file lives elsewhere.
fn stored_path(path: &Path, folder: &Path) -> String {
    match path.strip_prefix(folder) {
        Ok(relative) if !folder.as_os_str().is_empty() => relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        _ => path.to_string_lossy().into_owned(),
    }
}

fn embed_batch(clip: &Clip, batch: Vec<(PathBuf, u64)>) -> Vec<(PathBuf, u64, Vec<f32>)> {
    let (files, images): (Vec<_>, Vec<_>) = batch
        .into_iter()
        .filter_map(|(path, len)| {
            let pixels = preview::thumbnail_pixels(&path, preview::CARD, 0)?;
            Some(((path, len), pixels))
        })
        .unzip();
    if images.is_empty() {
        return Vec::new();
    }
    match clip.embed_images(&images) {
        Ok(vectors) => files
            .into_iter()
            .zip(vectors)
            .map(|((path, len), vector)| (path, len, vector))
            .collect(),
        Err(err) => {
            log::warn!("visual search skipped {} files: {err:#}", files.len());
            Vec::new()
        }
    }
}

/// What the query should be compared against: a description, or another file's own vector.
pub(crate) enum Query {
    Text(String),
    Like(PathBuf),
}

/// A scoring function over files for [`crate::delegate::QrateTableDelegate::ranked_rows`], or
/// `None` when the model is not loaded or the query cannot be embedded.
pub(crate) fn scorer(
    query: Query,
    cx: &App,
) -> Task<Option<impl Fn(&Path) -> Option<f32> + use<>>> {
    let visual = cx.global::<Visual>();
    let (clip, index) = (visual.clip.clone(), visual.index.clone());
    cx.background_executor().spawn(async move {
        let vector = match query {
            Query::Text(text) => clip?
                .embed_text(&text)
                .map_err(|err| log::warn!("could not embed a visual search query: {err:#}"))
                .ok()?,
            Query::Like(path) => lock(&index).get(&path)?.1.clone(),
        };
        Some(move |path: &Path| {
            let index = lock(&index);
            Some(visual_search::similarity(&vector, &index.get(path)?.1))
        })
    })
}

/// Ask the search bar to show the files that look most like `path`.
pub(crate) fn find_similar(path: PathBuf, cx: &mut App) {
    state(cx).similar = Some(path);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{cutoff, stored_path};

    #[test]
    fn stored_paths_are_relative_to_the_files_folder() {
        let folder = Path::new("/archive/files");
        assert_eq!(
            stored_path(&folder.join("box 1").join("a.jpg"), folder),
            "box 1/a.jpg"
        );
        assert_eq!(
            stored_path(Path::new("/elsewhere/b.jpg"), folder),
            "/elsewhere/b.jpg"
        );
        assert_eq!(
            stored_path(Path::new("/x/c.jpg"), Path::new("")),
            "/x/c.jpg"
        );
    }

    #[test]
    fn cutoff_keeps_hits_near_the_best_one() {
        assert_eq!(cutoff([], 0.15), 0);
        assert_eq!(cutoff([0.30, 0.28, 0.26, 0.20], 0.15), 3);
        assert_eq!(
            cutoff([0.30, 0.28, 0.26, 0.20], 0.02),
            1,
            "the best hit always stays"
        );
        assert_eq!(
            cutoff([1.0, 0.9, 0.5], 0.15),
            2,
            "find similar starts from itself"
        );
        assert_eq!(cutoff(vec![0.3; 900], 0.4), 500, "capped");
    }
}

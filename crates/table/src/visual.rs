//! Visual search for the open table: installing the model, keeping its index current for the rows'
//! linked files, and turning a query into ranked rows.
//!
//! Shared across windows as a global, because the model holds 600 MB and one copy is plenty.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Global, SharedString, Task};
use visual_search::{Clip, Index, Stamp};

/// Files embedded per model call. Larger batches barely help on a CPU and delay the progress count.
const BATCH: usize = 16;

/// Batches between index saves, about a minute of CPU work, so quitting mid-index keeps progress.
const SAVE_EVERY: usize = 40;

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

pub(crate) struct Visual {
    pub status: Status,
    clip: Option<Arc<Clip>>,
    index: Arc<Mutex<Index>>,
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

fn index_file() -> Option<PathBuf> {
    Some(settings::data_dir()?.join("visual-index.bin"))
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
        index: Arc::new(Mutex::new(Index::new(visual_search::MODEL))),
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

/// Embed any of `paths` the index does not already hold for their current contents. Returns at
/// once; progress shows in [`Status::Indexing`].
pub(crate) fn index(paths: Vec<PathBuf>, cx: &mut App) {
    let visual = cx.global::<Visual>();
    if visual.status != Status::Ready || visual.job.is_some() {
        return;
    }
    let fresh: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| !visual.seen.contains(path))
        .collect();
    let (Some(dir), Some(file)) = (model_dir(), index_file()) else {
        return;
    };
    if fresh.is_empty() {
        return;
    }
    let (index, clip) = (visual.index.clone(), visual.clip.clone());
    state(cx).seen.extend(fresh.iter().cloned());
    let job = cx.spawn(async move |cx| {
        let prepared = cx
            .background_executor()
            .spawn({
                let index = index.clone();
                let file = file.clone();
                async move {
                    let clip = match clip {
                        Some(clip) => clip,
                        None => Arc::new(Clip::load(&dir).map_err(|err| format!("{err:#}"))?),
                    };
                    // Stat outside the lock: searches rank rows on the UI thread through this index.
                    let stamped: Vec<(PathBuf, Stamp)> = fresh
                        .into_iter()
                        .filter_map(|path| Stamp::of(&path).map(|stamp| (path, stamp)))
                        .collect();
                    let empty = lock(&index).is_empty();
                    let loaded = empty.then(|| Index::load(&file, visual_search::MODEL));
                    let mut index = lock(&index);
                    if let Some(loaded) = loaded {
                        *index = loaded;
                    }
                    let stale: Vec<(PathBuf, Stamp)> = stamped
                        .into_iter()
                        .filter(|(path, stamp)| !index.is_current(path, *stamp))
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
            let (clip, index, file, batch) =
                (clip.clone(), index.clone(), file.clone(), batch.to_vec());
            let last = (at + 1) * BATCH >= total;
            cx.background_executor()
                .spawn(async move {
                    embed_batch(&clip, &index, batch);
                    if last || at % SAVE_EVERY == SAVE_EVERY - 1 {
                        save(&index, &file);
                    }
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

fn save(index: &Mutex<Index>, file: &Path) {
    if let Err(err) = lock(index).save(file) {
        log::warn!("could not save the visual search index, it will be rebuilt next launch: {err}");
    }
}

fn embed_batch(clip: &Clip, index: &Mutex<Index>, batch: Vec<(PathBuf, Stamp)>) {
    let (files, images): (Vec<_>, Vec<_>) = batch
        .into_iter()
        .filter_map(|(path, stamp)| {
            let pixels = preview::thumbnail_pixels(&path, preview::CARD, 0)?;
            Some(((path, stamp), pixels))
        })
        .unzip();
    if images.is_empty() {
        return;
    }
    match clip.embed_images(&images) {
        Ok(vectors) => {
            let mut index = lock(index);
            for ((path, stamp), vector) in files.into_iter().zip(vectors) {
                index.insert(path, stamp, vector);
            }
        }
        Err(err) => log::warn!("visual search skipped {} files: {err:#}", files.len()),
    }
}

/// What the query should be compared against: a description, or another file's own vector.
pub(crate) enum Query {
    Text(String),
    Like(PathBuf),
}

/// A scoring function over files for [`crate::delegate::QrateTableDelegate::ranked_matches`], or
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
            Query::Like(path) => lock(&index).vector(&path)?.to_vec(),
        };
        Some(move |path: &Path| {
            let index = lock(&index);
            Some(visual_search::similarity(&vector, index.vector(path)?))
        })
    })
}

/// Ask the search bar to show the files that look most like `path`.
pub(crate) fn find_similar(path: PathBuf, cx: &mut App) {
    state(cx).similar = Some(path);
}

#[cfg(test)]
mod tests {
    use super::cutoff;

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

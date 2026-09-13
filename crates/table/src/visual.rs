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

/// How many rows a visual search returns, best first.
///
/// ponytail: a fixed count, not a similarity threshold. CLIP scores cluster tightly, so a cut-off
/// needs tuning against real collections first.
pub(crate) const RESULTS: usize = 50;

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
                    let mut index = index.lock().unwrap_or_else(|err| err.into_inner());
                    if index.is_empty() {
                        *index = Index::load(&file, visual_search::MODEL);
                    }
                    let stale: Vec<(PathBuf, Stamp)> = fresh
                        .into_iter()
                        .filter_map(|path| Stamp::of(&path).map(|stamp| (path, stamp)))
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
            let (clip, index, batch) = (clip.clone(), index.clone(), batch.to_vec());
            cx.background_executor()
                .spawn(async move { embed_batch(&clip, &index, batch) })
                .await;
        }
        if total > 0 {
            cx.background_executor()
                .spawn(async move {
                    let index = index.lock().unwrap_or_else(|err| err.into_inner());
                    if let Err(err) = index.save(&file) {
                        log::warn!(
                            "could not save the visual search index, it will be rebuilt next launch: {err}"
                        );
                    }
                })
                .await;
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
            let mut index = index.lock().unwrap_or_else(|err| err.into_inner());
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
            Query::Like(path) => index
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .vector(&path)?
                .to_vec(),
        };
        Some(move |path: &Path| {
            let index = index.lock().unwrap_or_else(|err| err.into_inner());
            Some(visual_search::similarity(&vector, index.vector(path)?))
        })
    })
}

/// Ask the search bar to show the files that look most like `path`.
pub(crate) fn find_similar(path: PathBuf, cx: &mut App) {
    state(cx).similar = Some(path);
}

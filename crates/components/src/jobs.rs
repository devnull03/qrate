//! The components as the running app sees them: one [`Store`] under qrate's data folder, and the
//! [`Components`] global that runs installs, so every place that offers one shows the same job.
//! Observe it with `cx.observe_global::<Components>`.

use std::{
    collections::HashMap,
    fs,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow, bail};
use futures::{FutureExt as _, StreamExt as _, channel::mpsc, future::Shared};
use gpui::{App, AppContext as _, Global, SharedString, Task};
use settings::AppSettings;
use updater::Source;

use crate::{CLIP_DOWNLOAD, CLIP_SOURCE_KEY, ClipSource, ComponentId, Manifest, Removal, Store};

/// What a failed install tells the archivist. The cause goes to the log.
const FAILED: &str =
    "The download failed. Check the connection, or the download source in Settings.";

/// `<data dir>/components`, the store the whole app reads. `None` without a data folder.
pub fn store() -> Option<&'static Store> {
    static STORE: OnceLock<Option<Store>> = OnceLock::new();
    STORE
        .get_or_init(|| Some(Store::new(settings::data_dir()?.join("components"))))
        .as_ref()
}

/// The one reader of the download source that updates, the plugin catalog and components share.
pub fn source(cx: &App) -> Source {
    let setting = AppSettings::get(cx)
        .values
        .get(updater::DOWNLOAD_SOURCE_KEY)
        .map(|value| value.text());
    Source::choose(
        std::env::var(updater::DOWNLOAD_SOURCE_ENV).ok().as_deref(),
        setting.as_deref(),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Not installed. `download` is its size, once a manifest has said.
    Missing {
        download: Option<u64>,
    },
    Downloading {
        received: u64,
        total: u64,
    },
    /// Downloaded and verified, now unpacking.
    Installing,
    Installed {
        version: String,
    },
    /// Installed for another version of qrate, so this one does not use it.
    UpdateRequired,
    Failed(SharedString),
}

pub struct Components {
    store: &'static Store,
    manifest: Option<Arc<Manifest>>,
    jobs: HashMap<ComponentId, Job>,
    failed: HashMap<ComponentId, SharedString>,
}

impl Global for Components {}

struct Job {
    received: u64,
    total: u64,
    cancel: Arc<AtomicBool>,
    done: Shared<Task<Result<(), SharedString>>>,
}

/// Call once per process, after the single-instance hand-off and before anything loads a
/// component: the sweep deletes every folder without a receipt, which would include an install
/// another running qrate is in the middle of.
pub fn init(cx: &mut App) {
    let Some(store) = store() else {
        log::warn!("qrate has no data folder, so it cannot install optional components");
        return;
    };
    if cx.has_global::<Components>() {
        return;
    }
    store.sweep();
    cx.set_global(Components {
        store,
        manifest: None,
        jobs: HashMap::new(),
        failed: HashMap::new(),
    });
    let Some(models) = settings::data_dir().map(|dir| dir.join("models")) else {
        return;
    };
    cx.spawn(async move |cx| {
        let adopted = cx
            .background_spawn(async move {
                let adopted = store.adopt_clip(&models.join("clip-vit-base-patch32"));
                let _ = fs::remove_dir(&models);
                adopted
            })
            .await;
        match adopted {
            Ok(true) => cx.update_global::<Components, _>(|_, _| {}),

            Ok(false) => {}
            Err(error) => {
                log::warn!("could not move the visual search model into place: {error:#}")
            }
        }
    })
    .detach();
}

pub fn state(id: ComponentId, cx: &App) -> State {
    let components = cx.try_global::<Components>();
    if let Some(job) = components.and_then(|components| components.jobs.get(&id)) {
        return if job.total > 0 && job.received >= job.total {
            State::Installing
        } else {
            State::Downloading {
                received: job.received,
                total: job.total,
            }
        };
    }
    if let Some(reason) = components.and_then(|components| components.failed.get(&id)) {
        return State::Failed(reason.clone());
    }
    let store = components.map(|components| components.store).or_else(store);
    if let Some(receipt) = store.and_then(|store| store.receipt(id)) {
        return match store.and_then(|store| store.locate(id)) {
            Some(_) => State::Installed {
                version: receipt.version,
            },
            None => State::UpdateRequired,
        };
    }
    let listed = components
        .and_then(|components| components.manifest.as_ref())
        .and_then(|manifest| manifest.component(id))
        .map(|component| component.asset.size);
    State::Missing {
        download: listed.or((id == ComponentId::Clip).then_some(CLIP_DOWNLOAD)),
    }
}

/// Installs `id` in the background. A second call while one runs joins it rather than starting
/// another.
pub fn install(id: ComponentId, cx: &mut App) -> Task<Result<()>> {
    let Some(components) = cx.try_global::<Components>() else {
        return Task::ready(Err(anyhow!(
            "qrate has no data folder to install {} into",
            id.name()
        )));
    };
    if let Some(job) = components.jobs.get(&id) {
        let done = job.done.clone();
        return cx.spawn(async move |_| done.await.map_err(anyhow::Error::msg));
    }
    let (store, known) = (components.store, components.manifest.clone());
    let source = source(cx);
    let clip = ClipSource::from_setting(
        AppSettings::get(cx)
            .values
            .get(CLIP_SOURCE_KEY)
            .map(|value| value.text())
            .as_deref(),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let (progress, mut updates) = mpsc::unbounded();
    let work = cx.background_spawn({
        let cancel = cancel.clone();
        async move {
            let manifest = match known {
                Some(manifest) => Ok(manifest),
                None => store.fetch_manifest(&source).map(Arc::new),
            };
            // The CLIP weights also come from Hugging Face, which needs no manifest.
            let result = match &manifest {
                Err(error) if id != ComponentId::Clip => {
                    Err(anyhow!("could not read the components manifest: {error:#}"))
                }
                _ => {
                    let mut sent = 0;
                    store.install(
                        id,
                        manifest.as_deref().ok(),
                        &source,
                        clip,
                        &mut |received, total| {
                            if received == total
                                || received < sent
                                || received - sent >= total / 100
                            {
                                sent = received;
                                let _ = progress.unbounded_send((received, total));
                            }
                        },
                        &cancel,
                    )
                }
            };
            (manifest.ok(), result)
        }
    });
    let done = cx
        .spawn({
            let cancel = cancel.clone();
            async move |cx| {
                while let Some((received, total)) = updates.next().await {
                    cx.update(|cx| {
                        if let Some(job) = cx.global_mut::<Components>().jobs.get_mut(&id) {
                            (job.received, job.total) = (received, total);
                        }
                    });
                }
                let (manifest, result) = work.await;
                cx.update(|cx| {
                    let components = cx.global_mut::<Components>();
                    components.jobs.remove(&id);
                    components.manifest = manifest.or(components.manifest.take());
                    match &result {
                        Err(error) if !cancel.load(Ordering::Relaxed) => {
                            log::error!("could not install {}: {error:#}", id.name());
                            components.failed.insert(id, FAILED.into());
                        }
                        _ => {
                            components.failed.remove(&id);
                        }
                    }
                });
                result
                    .map(drop)
                    .map_err(|error| SharedString::from(format!("{error:#}")))
            }
        })
        .shared();
    cx.global_mut::<Components>().jobs.insert(
        id,
        Job {
            received: 0,
            total: 0,
            cancel,
            done: done.clone(),
        },
    );
    cx.spawn(async move |_| done.await.map_err(anyhow::Error::msg))
}

/// Stops an install of `id`. It ends at the next chunk, and what was downloaded is kept to resume.
pub fn cancel(id: ComponentId, cx: &App) {
    if let Some(job) = cx
        .try_global::<Components>()
        .and_then(|components| components.jobs.get(&id))
    {
        job.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn remove(id: ComponentId, cx: &mut App) -> Result<Removal> {
    let components = cx
        .try_global::<Components>()
        .context("qrate has no data folder for optional components")?;
    if components.jobs.contains_key(&id) {
        bail!("{} is being installed; cancel that first", id.name());
    }
    let removal = components.store.remove(id)?;
    cx.global_mut::<Components>().failed.remove(&id);
    Ok(removal)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, collections::HashMap, fs, rc::Rc};

    use gpui::{SharedString, TestAppContext};
    use settings::AppSettings;

    use super::{Components, FAILED, State, cancel, install, remove, state};
    use crate::{
        ComponentId, Removal,
        tests::{
            APP, KEY_ID, Mirror, RANGE, file_url, gz, listing, payload, sign, signing_key,
            store_for, tar_of,
        },
    };

    #[gpui::test]
    async fn an_install_runs_once_reports_progress_and_can_be_cancelled(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let archive = tar_of(&[("pdfium.dll", &[1_u8; 4096], 0o644)]);
        let assets = ["pdfium", "ffmpeg", "agent"].map(|id| {
            let mut asset = mirror.publish(
                &format!("component-{id}-1-test.tar.gz"),
                &gz(&archive),
                archive.len() as u64,
                "tar.gz",
            );
            if id == "agent" {
                asset["sha256"] = "0".repeat(64).into();
            }
            listing(id, "1", RANGE, vec![asset])
        });
        let envelope = sign(&signing_key(), KEY_ID, &payload(assets.to_vec()));
        fs::write(
            mirror.path("components.json"),
            serde_json::to_vec(&envelope).unwrap(),
        )
        .unwrap();
        let store = Box::leak(Box::new(store_for(temp.path(), APP)));
        cx.update(|cx| {
            let mut settings = AppSettings::default();
            settings.values.insert(
                updater::DOWNLOAD_SOURCE_KEY.into(),
                settings::Val::Text(file_url(mirror.dir.path()).into()),
            );
            cx.set_global(settings);
            cx.set_global(Components {
                store,
                manifest: None,
                jobs: HashMap::new(),
                failed: HashMap::new(),
            });
        });
        let notified = Rc::new(Cell::new(0));
        let _observer = cx.update({
            let notified = notified.clone();
            |cx| cx.observe_global::<Components>(move |_| notified.set(notified.get() + 1))
        });
        let pdfium = ComponentId::Pdfium;
        assert_eq!(
            cx.update(|cx| state(pdfium, cx)),
            State::Missing { download: None }
        );

        let first = cx.update(|cx| install(pdfium, cx));
        let second = cx.update(|cx| install(pdfium, cx));
        assert_eq!(cx.update(|cx| cx.global::<Components>().jobs.len()), 1);
        assert!(matches!(
            cx.update(|cx| state(pdfium, cx)),
            State::Downloading { .. }
        ));
        let refused = cx.update(|cx| remove(pdfium, cx)).unwrap_err();
        assert!(refused.to_string().contains("being installed"), "{refused}");
        cx.run_until_parked();
        first.await.unwrap();
        second.await.unwrap();

        assert_eq!(
            cx.update(|cx| state(pdfium, cx)),
            State::Installed {
                version: "1".into()
            }
        );
        assert!(notified.get() > 0, "observers hear about the install");
        let download = cx.update(|cx| {
            cx.global::<Components>()
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.component(ComponentId::Ffmpeg))
                .map(|component| component.asset.size)
        });
        assert_eq!(
            cx.update(|cx| state(ComponentId::Ffmpeg, cx)),
            State::Missing { download }
        );

        // A cancelled install goes back to missing, not failed.
        let cancelled = cx.update(|cx| {
            let task = install(ComponentId::Ffmpeg, cx);
            cancel(ComponentId::Ffmpeg, cx);
            task
        });
        cx.run_until_parked();
        assert!(cancelled.await.is_err());
        assert_eq!(
            cx.update(|cx| state(ComponentId::Ffmpeg, cx)),
            State::Missing { download }
        );

        assert_eq!(
            cx.update(|cx| remove(pdfium, cx)).unwrap(),
            Removal::Removed
        );
        assert!(matches!(
            cx.update(|cx| state(pdfium, cx)),
            State::Missing { .. }
        ));

        // A download that does not match its signed digest is reported as failed.
        let failed = cx.update(|cx| install(ComponentId::Agent, cx));
        cx.run_until_parked();
        assert!(failed.await.is_err());
        assert_eq!(
            cx.update(|cx| state(ComponentId::Agent, cx)),
            State::Failed(SharedString::from(FAILED))
        );
    }
}

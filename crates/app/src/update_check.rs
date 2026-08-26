//! Signed background updates and the GPUI-facing updater state.

use std::{
    fs,
    io::{Read as _, Write as _},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Result, ensure};
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use semver::Version;
use settings::AppSettings;
use updater::{InstallKind, Installation, JOB_NAME, ReleaseChannel, SignedEnvelope, UpdateJob};

const BETA_FEED: &str = "https://qrate.dvnl.work/updates/beta.json";
const STABLE_FEED: &str = "https://qrate.dvnl.work/updates/stable.json";
const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub fn automatic_updates(cx: &App) -> bool {
    AppSettings::get(cx)
        .values
        .get(updater::AUTO_UPDATE_KEY)
        .map(|value| value.bool())
        .unwrap_or(true)
}

#[derive(Clone, Debug)]
pub enum UpdateStatus {
    Disabled(Arc<str>),
    Idle,
    Checking,
    Downloading {
        version: Version,
        received: u64,
        total: Option<u64>,
    },
    Ready {
        version: Version,
        release_notes_url: String,
    },
    Error {
        stage: &'static str,
        message: Arc<str>,
        manual: bool,
    },
}

impl UpdateStatus {
    pub fn visible(&self) -> bool {
        matches!(
            self,
            Self::Downloading { .. } | Self::Ready { .. } | Self::Error { manual: true, .. }
        )
    }
}

pub struct AutoUpdater {
    status: UpdateStatus,
    installation: Option<Installation>,
    pending: bool,
    dismissed: bool,
    staged: Option<StagedUpdate>,
    _settings_sub: gpui::Subscription,
    poll_task: Option<Task<()>>,
}

#[derive(Clone)]
struct StagedUpdate {
    envelope: SignedEnvelope,
    path: PathBuf,
    version: Version,
}

struct GlobalUpdater(Entity<AutoUpdater>);
impl Global for GlobalUpdater {}

enum DownloadEvent {
    Found(Version),
    Progress { received: u64, total: Option<u64> },
    Done(Result<StagedUpdate>),
}

impl AutoUpdater {
    fn new(cx: &mut Context<Self>) -> Self {
        let installation = updater::detect_installation().ok();
        let status = match &installation {
            Some(installation) if installation.marker.kind == InstallKind::WindowsMsi => {
                UpdateStatus::Disabled("Updates are managed by your administrator".into())
            }
            Some(_) => UpdateStatus::Idle,
            None => UpdateStatus::Disabled("This source or unmarked build updates manually".into()),
        };
        let _settings_sub = cx.observe_global::<AppSettings>(|this, cx| {
            if automatic_updates(cx) && matches!(this.status, UpdateStatus::Idle) {
                this.poll(false, cx);
            }
            cx.notify();
        });
        Self {
            status,
            installation,
            pending: false,
            dismissed: false,
            staged: None,
            _settings_sub,
            poll_task: None,
        }
    }

    pub fn get(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalUpdater>()
            .map(|global| global.0.clone())
    }

    pub fn status(&self) -> &UpdateStatus {
        &self.status
    }

    pub fn visible(&self) -> bool {
        !self.dismissed && self.status.visible()
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.dismissed = true;
        cx.notify();
    }

    pub fn poll(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(installation) = self.installation.clone() else {
            if manual {
                self.status =
                    UpdateStatus::Disabled("This source or unmarked build updates manually".into());
                cx.notify();
            }
            return;
        };
        if !installation.marker.kind.self_managed() {
            return;
        }
        self.pending = true;
        self.dismissed = false;
        self.status = UpdateStatus::Checking;
        cx.notify();

        let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is SemVer");
        let feed = match ReleaseChannel::for_version(&current) {
            ReleaseChannel::Beta => BETA_FEED,
            ReleaseChannel::Stable => STABLE_FEED,
        };
        let (tx, rx) = async_channel::unbounded();
        cx.background_spawn(async move {
            let result = fetch_and_stage(
                feed,
                &installation,
                &current,
                |version| {
                    let _ = tx.try_send(DownloadEvent::Found(version));
                },
                |received, total| {
                    let _ = tx.try_send(DownloadEvent::Progress { received, total });
                },
            );
            let _ = tx.send(DownloadEvent::Done(result)).await;
        })
        .detach();

        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                let done = matches!(event, DownloadEvent::Done(_));
                this.update(cx, |this, cx| match event {
                    DownloadEvent::Found(version) => {
                        this.status = UpdateStatus::Downloading {
                            version,
                            received: 0,
                            total: None,
                        };
                        cx.notify();
                    }
                    DownloadEvent::Progress { received, total } => {
                        if let UpdateStatus::Downloading { version, .. } = &this.status {
                            this.status = UpdateStatus::Downloading {
                                version: version.clone(),
                                received,
                                total,
                            };
                            cx.notify();
                        }
                    }
                    DownloadEvent::Done(Ok(staged)) => {
                        this.pending = false;
                        this.status = UpdateStatus::Ready {
                            version: staged.version.clone(),
                            release_notes_url: updater::verify_envelope(&staged.envelope)
                                .map(|(manifest, _)| manifest.release_notes_url)
                                .unwrap_or_default(),
                        };
                        this.staged = Some(staged);
                        cx.notify();
                    }
                    DownloadEvent::Done(Err(error)) => {
                        this.pending = false;
                        if manual {
                            this.status = UpdateStatus::Error {
                                stage: "update",
                                message: format!("{error:#}").into(),
                                manual: true,
                            };
                        } else {
                            log::info!("automatic update check failed: {error:#}");
                            this.status = UpdateStatus::Idle;
                        }
                        cx.notify();
                    }
                })
                .ok()?;
                if done {
                    break;
                }
            }
            Some(())
        })
        .detach();
    }

    fn start_polling(&mut self, cx: &mut Context<Self>) {
        if automatic_updates(cx) {
            self.poll(false, cx);
        }
        self.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this
                    .update(cx, |this, cx| {
                        if automatic_updates(cx) {
                            this.poll(false, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

pub fn restart(cx: &mut App) -> Result<()> {
    let entity = AutoUpdater::get(cx).context("updater is unavailable")?;
    let (staged, installation) = {
        let updater = entity.read(cx);
        (
            updater
                .staged
                .clone()
                .context("no verified update is ready")?,
            updater
                .installation
                .clone()
                .context("installation is not managed")?,
        )
    };
    let updates = updater::updates_dir()?;
    let helper_source = helper_path(&installation);
    let helper_name = if cfg!(target_os = "windows") {
        "qrate-update-helper.exe"
    } else {
        "qrate-update-helper"
    };
    let helper = updates.join(staged.version.to_string()).join(helper_name);
    fs::create_dir_all(helper.parent().context("helper has no parent")?)?;
    fs::copy(&helper_source, &helper)
        .with_context(|| format!("copy update helper from {}", helper_source.display()))?;
    let job = UpdateJob {
        schema: 1,
        envelope: staged.envelope,
        artifact_path: staged.path,
        install_root: installation.root,
        executable: installation.executable,
        expected_current_version: installation.marker.packaged_version,
        target_version: staged.version,
    };
    updater::write_json_atomic(&updater::updates_dir()?.join(JOB_NAME), &job)?;
    cx.set_restart_path(helper);
    cx.restart();
    Ok(())
}

fn helper_path(installation: &Installation) -> PathBuf {
    #[cfg(target_os = "windows")]
    return installation.root.join("qrate-update-helper.exe");
    #[cfg(target_os = "macos")]
    return installation
        .root
        .join("Contents/Helpers/qrate-update-helper");
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    return installation.root.join("qrate-update-helper");
}

fn fetch_and_stage(
    feed: &str,
    installation: &Installation,
    current: &Version,
    mut found: impl FnMut(Version),
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<StagedUpdate> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("qrate-updater")
        .timeout(Duration::from_secs(30))
        .build()?;
    let envelope: SignedEnvelope = client.get(feed).send()?.error_for_status()?.json()?;
    let Some((manifest, artifact)) = updater::validate_update(&envelope, installation, current)?
    else {
        anyhow::bail!("qrate is up to date");
    };
    found(manifest.version.clone());
    let dir = updater::updates_dir()?.join(manifest.version.to_string());
    fs::create_dir_all(&dir)?;
    let filename = artifact
        .url
        .rsplit('/')
        .next()
        .context("artifact URL has no filename")?;
    let final_path = dir.join(filename);
    if final_path.exists() && updater::verify_artifact(&final_path, &artifact).is_ok() {
        return Ok(StagedUpdate {
            envelope,
            path: final_path,
            version: manifest.version,
        });
    }
    let partial = final_path.with_extension("partial");
    let mut response = client.get(&artifact.url).send()?.error_for_status()?;
    let total = response.content_length();
    if let Some(total) = total {
        ensure!(
            total == artifact.size,
            "server content length differs from signed manifest"
        );
    }
    let mut output = fs::File::create(&partial)?;
    let mut received = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        received += read as u64;
        ensure!(received <= artifact.size, "download exceeded signed size");
        output.write_all(&buffer[..read])?;
        progress(received, total);
    }
    output.sync_all()?;
    updater::verify_artifact(&partial, &artifact)?;
    fs::rename(&partial, &final_path)?;
    Ok(StagedUpdate {
        envelope,
        path: final_path,
        version: manifest.version,
    })
}

pub fn init(cx: &mut App) {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is SemVer");
    match updater::mark_healthy(&current) {
        Ok(Some(receipt)) if receipt.status == updater::ReceiptStatus::Failed => {
            log::error!("previous update failed: {}", receipt.message)
        }
        Err(error) => log::warn!("could not finalize previous update: {error:#}"),
        _ => {}
    }
    let updater = cx.new(AutoUpdater::new);
    cx.set_global(GlobalUpdater(updater.clone()));
    updater.update(cx, |updater, cx| updater.start_polling(cx));
}

pub fn check_now(cx: &mut App) {
    if let Some(updater) = AutoUpdater::get(cx) {
        updater.update(cx, |updater, cx| updater.poll(true, cx));
    }
}

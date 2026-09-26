//! The optional parts qrate installs when an archivist first needs one: PDFium, ffmpeg, the Pi
//! agent runtime and the CLIP weights (`docs/dev/components-plan.md`).
//!
//! Each release publishes a signed `components.json` beside its assets. An install downloads an
//! archive checked against the size and SHA-256 signed there, unpacks it into
//! `<root>/<id>/<version>`, and writes the receipt last. A folder without a receipt is not
//! installed, and [`Store::sweep`] deletes it at the next start. Every [`Store`] operation works
//! on its root, so the tests run on temporary folders; the app's store and the gpui side live in
//! `jobs.rs`.

mod jobs;

use std::{
    fs,
    io::Read,
    iter,
    path::{Component as PathPart, Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::VerifyingKey;
use semver::{BuildMetadata, Op, Version, VersionReq};
use serde::{Deserialize, Serialize};
use updater::{RELEASE_DOWNLOADS, SignedEnvelope, Source};

pub use jobs::{
    Components, Found, State, automatic_updates, cancel, found_by, init, install, refresh, remove,
    source, state, store,
};

pub const MANIFEST_NAME: &str = "components.json";
/// The app-wide setting that holds a [`ClipSource`].
pub const CLIP_SOURCE_KEY: &str = "clip_source";

const KIND: &str = "qrate-components";
const SCHEMA: u32 = 1;
const RECEIPT_SCHEMA: u32 = 1;
const DEV_KEY_ENV: &str = "QRATE_DEV_SIGNING_KEY";
const DEV_KEY_ID: &str = "qrate-dev";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_DOWNLOAD: u64 = 2 * 1024 * 1024 * 1024;
const MAX_INSTALLED: u64 = 4 * 1024 * 1024 * 1024;
const KEEP_PARTIAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The CLIP weights `visual-search` runs, as Hugging Face publishes them at one pinned revision.
const CLIP_VERSION: &str = "b33cedf";
const CLIP_UPSTREAM: &str = "https://huggingface.co/openai/clip-vit-base-patch32/resolve/b33cedfd0df4e43b8238760678fcc89e1a0d38b3/";
const CLIP_FILES: [(&str, u64, &str); 2] = [
    (
        "tokenizer.json",
        2_224_041,
        "b556ac8c99757ffb677208af34bc8c6721572114111a6e0aaf5fa69ff0b8d842",
    ),
    (
        "model.safetensors",
        605_157_884,
        "99d28a652e6ec46629ab7047a0ac82c69b1fe11e0ce672c43af65d3a9a3fc05d",
    ),
];
/// Bytes the CLIP weights take from Hugging Face, for a prompt shown before any manifest is read.
pub const CLIP_DOWNLOAD: u64 = CLIP_FILES[0].1 + CLIP_FILES[1].1;

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Moves on every install and removal, so an answer cached under an older value is stale.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// The answer `find` gave at `generation`, found again once an install or removal has moved it.
/// The slot stays locked while `find` runs, so two threads never look at once.
pub fn remember<T: Clone>(
    slot: &Mutex<Option<(u64, T)>>,
    generation: u64,
    find: impl FnOnce() -> T,
) -> T {
    let mut slot = slot.lock().unwrap_or_else(|error| error.into_inner());
    match &*slot {
        Some((at, found)) if *at == generation => found.clone(),
        _ => {
            let found = find();
            *slot = Some((generation, found.clone()));
            found
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId {
    Pdfium,
    Ffmpeg,
    Agent,
    Clip,
}

impl ComponentId {
    pub const ALL: [Self; 4] = [Self::Pdfium, Self::Ffmpeg, Self::Agent, Self::Clip];

    /// Its name in `components.json`, in asset names and on disk.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pdfium => "pdfium",
            Self::Ffmpeg => "ffmpeg",
            Self::Agent => "agent",
            Self::Clip => "clip",
        }
    }

    /// What the archivist calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdfium => "PDF preview",
            Self::Ffmpeg => "Video preview",
            Self::Agent => "Assistant runtime",
            Self::Clip => "Visual search model",
        }
    }
}

/// Where the CLIP weights come from. Every release also carries a copy, so visual search survives
/// Hugging Face moving them; whichever source is not chosen is tried when the chosen one fails.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ClipSource {
    #[default]
    HuggingFace,
    GitHub,
}

impl ClipSource {
    /// Reads the value stored under [`CLIP_SOURCE_KEY`]. Anything else means the default.
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("github") => Self::GitHub,
            _ => Self::HuggingFace,
        }
    }

    pub fn setting(self) -> &'static str {
        match self {
            Self::HuggingFace => "hugging-face",
            Self::GitHub => "github",
        }
    }
}

/// A verified `components.json`, reduced to what this computer can install.
#[derive(Clone, Debug)]
pub struct Manifest {
    pub app_version: Version,
    pub components: Vec<Component>,
}

impl Manifest {
    pub fn component(&self, id: ComponentId) -> Option<&Component> {
        self.components.iter().find(|component| component.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct Component {
    pub id: ComponentId,
    pub version: String,
    pub app: VersionReq,
    pub license: String,
    /// The asset for this operating system and architecture.
    pub asset: Asset,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub installed_size: u64,
    pub archive: Archive,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum Archive {
    #[serde(rename = "tar")]
    Tar,
    #[serde(rename = "tar.gz")]
    TarGz,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Receipt {
    pub schema: u32,
    pub id: ComponentId,
    pub version: String,
    /// The qrate versions this copy works with, as the manifest it came from said.
    pub app: VersionReq,
    /// The main download: the release asset, or the weights file from Hugging Face.
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    pub installed_at_unix_seconds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Removal {
    Removed,
    AfterRestart,
}

#[derive(Deserialize)]
struct Payload {
    schema: u32,
    app_version: Version,
    components: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Listing {
    version: String,
    app: VersionReq,
    license: String,
    assets: Vec<PlatformAsset>,
}

#[derive(Deserialize)]
struct PlatformAsset {
    os: String,
    arch: String,
    #[serde(flatten)]
    asset: Asset,
}

/// One way to get a component: the files to download and how each one lands in its folder.
struct Origin {
    version: String,
    app: VersionReq,
    fetches: Vec<Fetch>,
}

struct Fetch {
    url: String,
    size: u64,
    sha256: String,
    unpack: Unpack,
}

enum Unpack {
    Archive(Archive, u64),
    File(&'static str),
}

/// The components folder of one qrate version: `<data dir>/components` in the app.
pub struct Store {
    root: PathBuf,
    app: Version,
    /// A signing key trusted besides the release key: the developer's in a debug build.
    extra_key: Option<(String, VerifyingKey)>,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            app: Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("the workspace version is SemVer"),
            extra_key: dev_key(),
        }
    }

    fn verify(&self, envelope: &SignedEnvelope) -> Result<Manifest> {
        let payload = match &self.extra_key {
            Some((id, key)) if envelope.key_id == *id => {
                updater::verify_payload_with(key, id, envelope)
            }
            _ => updater::verify_payload(envelope),
        }
        .context("components manifest")?;
        parse_manifest(
            &payload,
            &self.app,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    }

    /// This version's signed manifest, read from its own release tag through `source`. When that
    /// fails, the copy verified last time is used, so sizes still show offline.
    pub fn fetch_manifest(&self, source: &Source) -> Result<Manifest> {
        let url = format!("{RELEASE_DOWNLOADS}v{}/{MANIFEST_NAME}", self.app);
        let cache = self.root.join(format!("manifest-{}.json", self.app));
        let fetched = (|| -> Result<(SignedEnvelope, Manifest)> {
            let client = updater::client()?;
            let bytes = updater::try_mirror(&source.github(&url), &url, |at| {
                updater::read_bytes(&client, at, MAX_MANIFEST_BYTES)
            })?;
            let envelope: SignedEnvelope = serde_json::from_slice(&bytes)
                .with_context(|| format!("{url} is not a signed manifest"))?;
            let manifest = self.verify(&envelope)?;
            Ok((envelope, manifest))
        })();
        match fetched {
            Ok((envelope, manifest)) => {
                if let Err(error) = updater::write_json_atomic(&cache, &envelope) {
                    log::warn!(
                        "could not keep a copy of the components manifest at {}: {error:#}",
                        cache.display()
                    );
                }
                Ok(manifest)
            }
            Err(error) => {
                let cached = fs::read(&cache)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<SignedEnvelope>(&bytes).ok())
                    .and_then(|envelope| self.verify(&envelope).ok());
                let Some(manifest) = cached else {
                    return Err(error);
                };
                log::warn!("could not read {url}, so using the copy verified earlier: {error:#}");
                Ok(manifest)
            }
        }
    }

    pub fn receipt(&self, id: ComponentId) -> Option<Receipt> {
        let bytes = fs::read(self.receipt_path(id)).ok()?;
        serde_json::from_slice::<Receipt>(&bytes)
            .ok()
            .filter(|receipt| {
                receipt.schema == RECEIPT_SCHEMA
                    && receipt.id == id
                    && valid_version(&receipt.version)
            })
    }

    /// The installed folder of `id`, when its receipt allows this version of qrate. Reads two
    /// small files and never the network.
    pub fn locate(&self, id: ComponentId) -> Option<PathBuf> {
        let receipt = self
            .receipt(id)
            .filter(|receipt| compatible(&receipt.app, &self.app))?;
        let dir = self.root.join(id.name()).join(receipt.version);
        dir.is_dir().then_some(dir)
    }

    /// Downloads, checks and unpacks `id`, then records it. Until the receipt is written the
    /// previous version stays installed; after it, that version is deleted. `clip` decides which
    /// CLIP source goes first, and is ignored for every other component.
    pub fn install(
        &self,
        id: ComponentId,
        manifest: Option<&Manifest>,
        source: &Source,
        clip: ClipSource,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<Receipt> {
        let mut failure = None;
        for origin in origins(id, manifest, clip) {
            if let Some(error) = failure.take() {
                log::warn!("{error:#}; trying {} instead", origin.fetches[0].url);
            }
            match self.install_from(id, &origin, source, progress, cancel) {
                Ok(receipt) => return Ok(receipt),
                Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
                Err(error) => {
                    failure = Some(error.context(format!("could not install {}", id.name())));
                }
            }
        }
        Err(failure.unwrap_or_else(|| {
            anyhow!(
                "no {} download is published for this version of qrate on this computer",
                id.name()
            )
        }))
    }

    fn install_from(
        &self,
        id: ComponentId,
        origin: &Origin,
        source: &Source,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<Receipt> {
        let home = self.root.join(id.name());
        let downloads = self.root.join("downloads");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&downloads)?;
        let staging = tempfile::Builder::new()
            .prefix(".staging-")
            .tempdir_in(&home)?;
        let total = origin.fetches.iter().map(|fetch| fetch.size).sum();
        let mut done = 0;
        for fetch in &origin.fetches {
            let file = downloads.join(&fetch.sha256);
            updater::try_mirror(&source.github(&fetch.url), &fetch.url, |url| {
                updater::download_verified(
                    url,
                    fetch.size,
                    &fetch.sha256,
                    &file,
                    |received, _| progress(done + received, total),
                    cancel,
                )
            })
            .with_context(|| format!("download {}", fetch.url))?;
            done += fetch.size;
            match fetch.unpack {
                Unpack::Archive(archive, limit) => {
                    let unpacked = unpack(&file, archive, staging.path(), limit);
                    let _ = fs::remove_file(&file);
                    unpacked.with_context(|| format!("unpack {}", fetch.url))?;
                }
                Unpack::File(name) => fs::rename(&file, staging.path().join(name))?,
            }
        }
        ensure!(!cancel.load(Ordering::Relaxed), "install cancelled");

        let target = home.join(&origin.version);
        if target.exists() {
            fs::rename(&target, home.join(trash_name()))
                .context("the installed copy is in use; restart qrate and install it again")?;
        }
        fs::rename(staging.path(), &target)?;
        let main = origin.fetches.last().context("nothing to download")?;
        let receipt = Receipt {
            schema: RECEIPT_SCHEMA,
            id,
            version: origin.version.clone(),
            app: origin.app.clone(),
            url: main.url.clone(),
            sha256: main.sha256.clone(),
            bytes: total,
            installed_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        };
        if let Err(error) = updater::write_json_atomic(&self.receipt_path(id), &receipt) {
            let _ = fs::remove_dir_all(&target);
            return Err(error).context("record the installed component");
        }
        GENERATION.fetch_add(1, Ordering::AcqRel);
        prune(&home, Some(&origin.version));
        log::info!("installed {} {}", id.name(), origin.version);
        Ok(receipt)
    }

    /// Records the CLIP weights an older qrate downloaded into `legacy` as installed, moving them
    /// rather than fetching 607 MB again. Weights that do not hash to the pins are deleted.
    pub fn adopt_clip(&self, legacy: &Path) -> Result<bool> {
        self.adopt(legacy, &CLIP_FILES)
    }

    fn adopt(&self, legacy: &Path, files: &[(&str, u64, &str)]) -> Result<bool> {
        let id = ComponentId::Clip;
        if !legacy.is_dir() || self.receipt(id).is_some() {
            return Ok(false);
        }
        for (name, size, sha256) in files {
            let path = legacy.join(name);
            let pinned = path.is_file()
                && updater::sha256_file(&path)
                    .with_context(|| format!("check {}", path.display()))?
                    == ((*sha256).to_owned(), *size);
            if !pinned {
                log::info!("deleting incomplete CLIP weights at {}", legacy.display());
                fs::remove_dir_all(legacy)?;
                return Ok(false);
            }
        }
        let home = self.root.join(id.name());
        let target = home.join(CLIP_VERSION);
        fs::create_dir_all(&home)?;
        if target.exists() {
            fs::rename(&target, home.join(trash_name()))?;
        }
        fs::rename(legacy, &target)?;
        let (name, _, sha256) = files.last().context("no files to adopt")?;
        let receipt = Receipt {
            schema: RECEIPT_SCHEMA,
            id,
            version: CLIP_VERSION.into(),
            app: VersionReq::STAR,
            url: format!("{CLIP_UPSTREAM}{name}"),
            sha256: (*sha256).into(),
            bytes: files.iter().map(|(_, size, _)| size).sum(),
            installed_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        };
        if let Err(error) = updater::write_json_atomic(&self.receipt_path(id), &receipt) {
            let _ = fs::rename(&target, legacy);
            return Err(error).context("record the adopted CLIP weights");
        }
        GENERATION.fetch_add(1, Ordering::AcqRel);
        prune(&home, Some(CLIP_VERSION));
        log::info!("adopted the CLIP weights from {}", legacy.display());
        Ok(true)
    }

    /// Uninstalls `id`. qrate treats it as gone at once, but on Windows a loaded PDFium or a
    /// running Pi cannot be moved, so its files then stay until [`Store::sweep`] at the next start.
    pub fn remove(&self, id: ComponentId) -> Result<Removal> {
        let home = self.root.join(id.name());
        let installed = self.receipt(id).map(|receipt| home.join(receipt.version));
        match fs::remove_file(self.receipt_path(id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error).with_context(|| format!("remove the {} receipt", id.name()));
            }
            _ => {}
        }
        GENERATION.fetch_add(1, Ordering::AcqRel);
        let Some(dir) = installed.filter(|dir| dir.exists()) else {
            return Ok(Removal::Removed);
        };
        let trash = home.join(trash_name());
        if fs::rename(&dir, &trash).is_err() {
            log::info!(
                "{} is in use, so it is deleted when qrate next starts",
                id.name()
            );
            return Ok(Removal::AfterRestart);
        }
        let _ = fs::remove_dir_all(&trash);
        log::info!("removed {}", id.name());
        Ok(Removal::Removed)
    }

    /// For startup, before anything loads a component: deletes every folder no receipt names
    /// (removals that had to wait, replaced versions, interrupted installs), the manifests of other
    /// versions, and downloads nobody resumed for a week.
    pub fn sweep(&self) {
        for id in ComponentId::ALL {
            let receipt = self.receipt(id);
            prune(
                &self.root.join(id.name()),
                receipt.as_ref().map(|receipt| receipt.version.as_str()),
            );
        }
        let current = format!("manifest-{}.json", self.app);
        for entry in fs::read_dir(&self.root).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("manifest-") && name != current {
                let _ = fs::remove_file(entry.path());
            }
        }
        for entry in fs::read_dir(self.root.join("downloads"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let resumable = entry.file_name().to_string_lossy().ends_with(".partial")
                && entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < KEEP_PARTIAL));
            if !resumable {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    fn receipt_path(&self, id: ComponentId) -> PathBuf {
        self.root
            .join("receipts")
            .join(format!("{}.json", id.name()))
    }
}

/// The ways to install `id`, in the order to try them.
fn origins(id: ComponentId, manifest: Option<&Manifest>, clip: ClipSource) -> Vec<Origin> {
    let release = manifest
        .and_then(|manifest| manifest.component(id))
        .map(|component| Origin {
            version: component.version.clone(),
            app: component.app.clone(),
            fetches: vec![Fetch {
                url: component.asset.url.clone(),
                size: component.asset.size,
                sha256: component.asset.sha256.clone(),
                unpack: Unpack::Archive(component.asset.archive, component.asset.installed_size),
            }],
        });
    if id != ComponentId::Clip {
        return release.into_iter().collect();
    }
    let upstream = Origin {
        version: CLIP_VERSION.into(),
        app: VersionReq::STAR,
        fetches: CLIP_FILES
            .iter()
            .map(|(name, size, sha256)| Fetch {
                url: format!("{CLIP_UPSTREAM}{name}"),
                size: *size,
                sha256: (*sha256).into(),
                unpack: Unpack::File(name),
            })
            .collect(),
    };
    match clip {
        ClipSource::HuggingFace => iter::once(upstream).chain(release).collect(),
        ClipSource::GitHub => release.into_iter().chain(iter::once(upstream)).collect(),
    }
}

fn parse_manifest(payload: &[u8], app: &Version, os: &str, arch: &str) -> Result<Manifest> {
    let value: serde_json::Value =
        serde_json::from_slice(payload).context("the components manifest is not JSON")?;
    ensure!(
        value.get("kind").and_then(|kind| kind.as_str()) == Some(KIND),
        "the signed payload is not a components manifest"
    );
    let payload: Payload = serde_json::from_value(value).context("invalid components manifest")?;
    ensure!(
        payload.schema == SCHEMA,
        "unsupported components manifest schema {}",
        payload.schema
    );
    ensure!(
        payload.app_version == *app,
        "the components manifest is for qrate {}, not {app}",
        payload.app_version
    );
    let mut components: Vec<Component> = Vec::new();
    for entry in payload.components {
        let Some(id) = entry
            .get("id")
            .and_then(|id| id.as_str())
            .and_then(|name| ComponentId::ALL.into_iter().find(|id| id.name() == name))
        else {
            continue;
        };
        if components.iter().any(|component| component.id == id) {
            continue;
        }
        let name = id.name();
        let listing: Listing = serde_json::from_value(entry)
            .with_context(|| format!("invalid {name} entry in the components manifest"))?;
        ensure!(
            valid_version(&listing.version),
            "invalid {name} version {:?}",
            listing.version
        );
        if !compatible(&listing.app, app) {
            log::warn!(
                "the manifest's {name} is for qrate {}, not {app}",
                listing.app
            );
            continue;
        }
        let Some(asset) = listing
            .assets
            .into_iter()
            .find(|asset| {
                (asset.os == os || asset.os == "any") && (asset.arch == arch || asset.arch == "any")
            })
            .map(|asset| asset.asset)
        else {
            continue;
        };
        ensure!(
            asset.url.starts_with(RELEASE_DOWNLOADS),
            "{name} downloads from an unexpected host"
        );
        ensure!(is_sha256(&asset.sha256), "invalid {name} digest");
        ensure!(
            (1..=MAX_DOWNLOAD).contains(&asset.size)
                && (1..=MAX_INSTALLED).contains(&asset.installed_size),
            "invalid {name} size"
        );
        components.push(Component {
            id,
            version: listing.version,
            app: listing.app,
            license: listing.license,
            asset,
        });
    }
    Ok(Manifest {
        app_version: payload.app_version,
        components,
    })
}

/// Plain version order with comparison operators only, so a beta update keeps its components.
fn compatible(req: &VersionReq, version: &Version) -> bool {
    req.comparators.iter().all(|comparator| {
        let bound = Version {
            major: comparator.major,
            minor: comparator.minor.unwrap_or(0),
            patch: comparator.patch.unwrap_or(0),
            pre: comparator.pre.clone(),
            build: BuildMetadata::EMPTY,
        };
        let order = version.cmp_precedence(&bound);
        match comparator.op {
            Op::Exact => order.is_eq(),
            Op::Greater => order.is_gt(),
            Op::GreaterEq => order.is_ge(),
            Op::Less => order.is_lt(),
            Op::LessEq => order.is_le(),
            _ => false,
        }
    })
}

/// Unpacks a verified tar into `dest`. Refuses anything that could land outside it (absolute
/// paths, `..`, links) and stops once the files would pass the signed `limit`.
fn unpack(archive: &Path, kind: Archive, dest: &Path, limit: u64) -> Result<()> {
    let file = fs::File::open(archive)?;
    let reader: Box<dyn Read> = match kind {
        Archive::Tar => Box::new(file),
        Archive::TarGz => Box::new(flate2::read::GzDecoder::new(file)),
    };
    let mut tar = tar::Archive::new(reader);
    let mut expanded = 0_u64;
    for entry in tar.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
            continue;
        }
        let path = entry.path()?.into_owned();
        let mut relative = PathBuf::new();
        for part in path.components() {
            match part {
                PathPart::Normal(part) => relative.push(part),
                PathPart::CurDir => {}
                _ => bail!("the archive has an unsafe path {}", path.display()),
            }
        }
        if relative.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&relative);
        if kind.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        ensure!(
            kind.is_file(),
            "the archive has a link or special file at {}",
            path.display()
        );
        expanded = expanded.saturating_add(entry.size());
        ensure!(
            expanded <= limit,
            "the archive expands past its signed size"
        );
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&out)?;
    }
    Ok(())
}

/// Deletes everything in a component's folder except the version `keep`. Whatever is in use stays
/// for the next sweep.
fn prune(home: &Path, keep: Option<&str>) {
    for entry in fs::read_dir(home).into_iter().flatten().flatten() {
        if keep.is_some_and(|keep| entry.file_name() == keep) {
            continue;
        }
        let path = entry.path();
        let removed = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if let Err(error) = removed {
            log::debug!("{} stays until the next start: {error}", path.display());
        }
    }
}

fn trash_name() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!(".trash-{now}")
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A version is also a folder name, and the leading dot is kept for staging and trash.
fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && !version.starts_with('.')
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// Debug builds also trust the public key in `QRATE_DEV_SIGNING_KEY` (base64url) under the key id
/// `qrate-dev`, so a manifest signed on a developer's machine can be tested. Release builds do not.
fn dev_key() -> Option<(String, VerifyingKey)> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let value = std::env::var(DEV_KEY_ENV).ok()?;
    let key = URL_SAFE_NO_PAD
        .decode(value.trim().trim_end_matches('='))
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok());
    if key.is_none() {
        log::warn!("ignoring {DEV_KEY_ENV}: it is not a base64url Ed25519 public key");
    }
    Some((DEV_KEY_ID.into(), key?))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write as _,
        path::{Path, PathBuf},
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer as _, SigningKey};
    use semver::{Version, VersionReq};
    use serde_json::{Value, json};
    use tar::{EntryType, Header};
    use updater::{RELEASE_DOWNLOADS, SignedEnvelope, Source};

    use super::{
        CLIP_VERSION, ClipSource, ComponentId, KEEP_PARTIAL, Manifest, Removal, Store, compatible,
        generation, origins, remember,
    };

    pub(crate) const APP: &str = "0.6.0-beta.1";
    pub(crate) const RANGE: &str = ">=0.6.0-0, <0.7.0-0";
    pub(crate) const KEY_ID: &str = "qrate-test";

    pub(crate) fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    pub(crate) fn store_for(root: &Path, app: &str) -> Store {
        Store {
            root: root.to_path_buf(),
            app: Version::parse(app).unwrap(),
            extra_key: Some((KEY_ID.into(), signing_key().verifying_key())),
        }
    }

    pub(crate) fn sign(key: &SigningKey, key_id: &str, payload: &Value) -> SignedEnvelope {
        let bytes = serde_json::to_vec(payload).unwrap();
        SignedEnvelope {
            schema: 1,
            key_id: key_id.into(),
            signature_base64: STANDARD.encode(key.sign(&bytes).to_bytes()),
            payload_base64: STANDARD.encode(bytes),
        }
    }

    pub(crate) fn payload(components: Vec<Value>) -> Value {
        json!({
            "kind": "qrate-components",
            "schema": 1,
            "app_version": APP,
            "published_at": "2026-10-01T00:00:00Z",
            "components": components,
        })
    }

    pub(crate) fn listing(id: &str, version: &str, app: &str, assets: Vec<Value>) -> Value {
        json!({ "id": id, "version": version, "app": app, "license": "MIT", "assets": assets })
    }

    fn release_url(name: &str) -> String {
        format!("{RELEASE_DOWNLOADS}v{APP}/{name}")
    }

    pub(crate) fn file_url(path: &Path) -> String {
        let path = path.to_string_lossy().replace('\\', "/");
        format!("file:///{}", path.trim_start_matches('/'))
    }

    /// A download source folder. `publish` puts a file where it stands in for this release.
    pub(crate) struct Mirror {
        pub(crate) dir: tempfile::TempDir,
    }

    impl Mirror {
        pub(crate) fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        pub(crate) fn source(&self) -> Source {
            Source::parse(Some(&file_url(self.dir.path()))).unwrap()
        }

        pub(crate) fn path(&self, name: &str) -> PathBuf {
            self.dir
                .path()
                .join(format!("github/devnull03/qrate/releases/download/v{APP}"))
                .join(name)
        }

        /// Publishes `bytes` as `name` and returns its asset entry for this computer.
        pub(crate) fn publish(
            &self,
            name: &str,
            bytes: &[u8],
            installed: u64,
            archive: &str,
        ) -> Value {
            let path = self.path(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            let (sha256, size) = updater::sha256_file(&path).unwrap();
            json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "url": release_url(name),
                "size": size,
                "sha256": sha256,
                "installed_size": installed,
                "archive": archive,
            })
        }
    }

    pub(crate) fn tar_of(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, body, mode) in files {
            let mut header = Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(*mode);
            header.set_entry_type(EntryType::Regular);
            builder.append_data(&mut header, name, *body).unwrap();
        }
        builder.into_inner().unwrap()
    }

    pub(crate) fn gz(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn verified(store: &Store, components: Vec<Value>) -> Manifest {
        store
            .verify(&sign(&signing_key(), KEY_ID, &payload(components)))
            .unwrap()
    }

    /// PDFium `version` published on the mirror under a new name, and the manifest that lists it.
    fn pdfium(mirror: &Mirror, store: &Store, version: &str, archive: &[u8]) -> Manifest {
        static PUBLISHED: AtomicUsize = AtomicUsize::new(0);
        let n = PUBLISHED.fetch_add(1, Ordering::Relaxed);
        let name = format!("component-pdfium-{version}-test-{n}.tar.gz");
        let asset = mirror.publish(&name, &gz(archive), archive.len() as u64, "tar.gz");
        verified(store, vec![listing("pdfium", version, RANGE, vec![asset])])
    }

    fn attempt(
        store: &Store,
        id: ComponentId,
        manifest: &Manifest,
        mirror: &Mirror,
    ) -> Result<(), String> {
        store
            .install(
                id,
                Some(manifest),
                &mirror.source(),
                ClipSource::GitHub,
                &mut |_, _| {},
                &AtomicBool::new(false),
            )
            .map(drop)
            .map_err(|error| format!("{error:#}"))
    }

    #[test]
    fn keeps_this_computers_assets_from_a_signed_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let store = store_for(temp.path(), APP);
        let here = json!({
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "url": release_url("here.tar.gz"), "size": 10, "sha256": "a".repeat(64),
            "installed_size": 20, "archive": "tar.gz",
        });
        let mut elsewhere = here.clone();
        elsewhere["os"] = json!("plan9");
        elsewhere["url"] = json!(release_url("elsewhere.tar.gz"));
        let mut anywhere = here.clone();
        anywhere["os"] = json!("any");
        anywhere["arch"] = json!("any");
        anywhere["archive"] = json!("tar");

        let manifest = verified(
            &store,
            vec![
                listing(
                    "pdfium",
                    "chromium-7881",
                    RANGE,
                    vec![elsewhere.clone(), here],
                ),
                listing("clip", "b33cedf", ">=0.6.0-0", vec![anywhere]),
                json!({ "id": "scanner", "whatever": [1, 2, 3] }),
                listing("ffmpeg", "n8.1", ">=0.7.0-0", vec![elsewhere.clone()]),
                listing("agent", "0.84.2", RANGE, vec![elsewhere]),
            ],
        );

        let ids: Vec<_> = manifest.components.iter().map(|c| c.id).collect();
        assert_eq!(ids, [ComponentId::Pdfium, ComponentId::Clip]);
        let pdfium = manifest.component(ComponentId::Pdfium).unwrap();
        assert_eq!(pdfium.asset.url, release_url("here.tar.gz"));
        assert_eq!(pdfium.app, VersionReq::parse(RANGE).unwrap());
        assert_eq!(manifest.app_version, Version::parse(APP).unwrap());
    }

    #[test]
    fn refuses_manifests_the_key_did_not_sign_for_this_version() {
        let temp = tempfile::tempdir().unwrap();
        let store = store_for(temp.path(), APP);
        let key = signing_key();
        let asset = json!({
            "os": "any", "arch": "any", "url": release_url("a.tar"), "size": 1,
            "sha256": "a".repeat(64), "installed_size": 1, "archive": "tar",
        });
        let good = payload(vec![listing("clip", "b33cedf", RANGE, vec![asset.clone()])]);
        assert!(store.verify(&sign(&key, KEY_ID, &good)).is_ok());

        let tampered = SignedEnvelope {
            payload_base64: STANDARD.encode(serde_json::to_vec(&payload(vec![])).unwrap()),
            ..sign(&key, KEY_ID, &good)
        };
        assert!(store.verify(&tampered).is_err());
        let stranger = SigningKey::from_bytes(&[9; 32]);
        assert!(store.verify(&sign(&stranger, KEY_ID, &good)).is_err());
        // Only the embedded release key answers to the release key id.
        assert!(
            store
                .verify(&sign(&key, updater::UPDATE_KEY_ID, &good))
                .is_err()
        );

        let update = json!({
            "channel": "beta", "version": APP, "published_at": "", "release_notes_url": "",
            "artifacts": [],
        });
        let mut other_kind = good.clone();
        other_kind["kind"] = json!("qrate-update");
        let mut other_version = good.clone();
        other_version["app_version"] = json!("0.6.0-beta.2");
        let mut future = good.clone();
        future["schema"] = json!(2);
        let mut offsite = asset.clone();
        offsite["url"] = json!("https://example.com/a.tar");
        let mut short_digest = asset;
        short_digest["sha256"] = json!("abc");
        for refused in [
            update,
            other_kind,
            other_version,
            future,
            payload(vec![listing("clip", "b33cedf", RANGE, vec![offsite])]),
            payload(vec![listing("clip", "b33cedf", RANGE, vec![short_digest])]),
            payload(vec![listing("clip", "../up", RANGE, vec![])]),
        ] {
            assert!(
                store.verify(&sign(&key, KEY_ID, &refused)).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn compatibility_is_plain_version_order() {
        let check = |req: &str, version: &str| {
            compatible(
                &VersionReq::parse(req).unwrap(),
                &Version::parse(version).unwrap(),
            )
        };
        assert!(check(RANGE, "0.6.0-beta.1"));
        assert!(check(RANGE, "0.6.3"));
        assert!(!check(RANGE, "0.7.0-alpha.1"));
        assert!(!check(RANGE, "0.5.9"));
        assert!(check("*", "0.6.0-beta.1"));
        // SemVer's own matching refuses this one. Here a later beta keeps its components.
        assert!(check(">=0.6.0-beta.1", "0.6.1-beta.1"));
        assert!(check("=0.6.0", "0.6.0"));
        assert!(!check("=0.6.0", "0.6.1"));
        assert!(check(">0.6.0, <=0.7.0", "0.7.0"));
        // Caret, tilde and wildcards say nothing plain about pre-releases, so they match nothing.
        assert!(!check("^0.6", "0.6.1"));
        assert!(!check("0.6.*", "0.6.1"));
    }

    #[test]
    fn installs_from_a_local_mirror_and_finds_it_again() {
        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let store = store_for(temp.path(), APP);
        let manifest = pdfium(
            &mirror,
            &store,
            "chromium-7881",
            &tar_of(&[
                ("./bin/tool", b"#!/bin/sh\n", 0o755),
                ("./LICENSE.txt", b"license", 0o644),
            ]),
        );
        assert_eq!(store.locate(ComponentId::Pdfium), None);
        let before = generation();

        let mut last = (0, 0);
        let receipt = store
            .install(
                ComponentId::Pdfium,
                Some(&manifest),
                &mirror.source(),
                ClipSource::default(),
                &mut |received, total| last = (received, total),
                &AtomicBool::new(false),
            )
            .unwrap();

        let asset = &manifest.component(ComponentId::Pdfium).unwrap().asset;
        assert_eq!(receipt.version, "chromium-7881");
        assert_eq!(receipt.url, asset.url);
        assert_eq!(receipt.sha256, asset.sha256);
        assert_eq!(receipt.bytes, asset.size);
        assert_eq!(last, (asset.size, asset.size));
        assert!(generation() > before);
        assert_eq!(store.receipt(ComponentId::Pdfium), Some(receipt));
        let dir = store.locate(ComponentId::Pdfium).unwrap();
        assert_eq!(dir, temp.path().join("pdfium").join("chromium-7881"));
        assert_eq!(fs::read(dir.join("LICENSE.txt")).unwrap(), b"license");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(dir.join("bin/tool"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "executables stay executable");
        }
        // The archive goes once it is unpacked. Only the installed tree is kept.
        assert_eq!(
            fs::read_dir(temp.path().join("downloads")).unwrap().count(),
            0
        );

        // A new version replaces the old one.
        let next = pdfium(
            &mirror,
            &store,
            "chromium-7900",
            &tar_of(&[("f", b"x", 0o644)]),
        );
        attempt(&store, ComponentId::Pdfium, &next, &mirror).unwrap();
        assert_eq!(
            store.locate(ComponentId::Pdfium).unwrap(),
            temp.path().join("pdfium").join("chromium-7900")
        );
        assert!(!dir.exists());
    }

    #[test]
    fn reads_the_manifest_through_the_download_source_and_keeps_a_copy() {
        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let store = store_for(temp.path(), APP);
        let asset = mirror.publish("component-clip.tar", &tar_of(&[]), 1, "tar");
        let envelope = sign(
            &signing_key(),
            KEY_ID,
            &payload(vec![listing("clip", "b33cedf", RANGE, vec![asset])]),
        );
        fs::write(
            mirror.path("components.json"),
            serde_json::to_vec(&envelope).unwrap(),
        )
        .unwrap();

        let manifest = store.fetch_manifest(&mirror.source()).unwrap();
        assert!(manifest.component(ComponentId::Clip).is_some());
        assert!(temp.path().join(format!("manifest-{APP}.json")).exists());

        // A manifest that cannot be read falls back to the copy verified last time,
        fs::write(mirror.path("components.json"), b"not a manifest").unwrap();
        let cached = store.fetch_manifest(&mirror.source()).unwrap();
        assert!(cached.component(ComponentId::Clip).is_some());

        // and with no such copy, the failure is reported.
        let fresh = tempfile::tempdir().unwrap();
        assert!(
            store_for(fresh.path(), APP)
                .fetch_manifest(&mirror.source())
                .is_err()
        );
    }

    #[test]
    fn refuses_unsafe_archives_and_keeps_the_installed_version() {
        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let store = store_for(temp.path(), APP);
        let good = pdfium(&mirror, &store, "1", &tar_of(&[("a", b"one", 0o644)]));
        attempt(&store, ComponentId::Pdfium, &good, &mirror).unwrap();
        let installed = store.locate(ComponentId::Pdfium).unwrap();

        let mut escape = tar::Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.as_gnu_mut().unwrap().name[..7].copy_from_slice(b"../evil");
        header.set_size(4);
        header.set_mode(0o644);
        header.set_entry_type(EntryType::Regular);
        header.set_cksum();
        escape.append(&header, &b"evil"[..]).unwrap();

        let mut link = tar::Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        link.append_link(&mut header, "lib", "/etc").unwrap();

        let ten = gz(&tar_of(&[("a", b"0123456789", 0o644)]));
        let mut too_big = mirror.publish("component-pdfium-2-big.tar.gz", &ten, 9, "tar.gz");
        too_big["installed_size"] = json!(9);
        let mut wrong_digest = mirror.publish("component-pdfium-2-sum.tar.gz", &ten, 10, "tar.gz");
        wrong_digest["sha256"] = json!("0".repeat(64));

        let attempts = [
            (
                pdfium(&mirror, &store, "2", &escape.into_inner().unwrap()),
                "unsafe path",
            ),
            (
                pdfium(&mirror, &store, "2", &link.into_inner().unwrap()),
                "link or special file",
            ),
            (
                verified(&store, vec![listing("pdfium", "2", RANGE, vec![too_big])]),
                "past its signed size",
            ),
            (
                verified(
                    &store,
                    vec![listing("pdfium", "2", RANGE, vec![wrong_digest])],
                ),
                "checksum mismatch",
            ),
        ];
        for (manifest, reason) in attempts {
            let error = attempt(&store, ComponentId::Pdfium, &manifest, &mirror).unwrap_err();
            assert!(error.contains(reason), "{error}");
            assert_eq!(store.locate(ComponentId::Pdfium).as_ref(), Some(&installed));
            assert!(!temp.path().join("pdfium/2").exists());
            assert!(!temp.path().join("evil").exists());
        }
        assert_eq!(fs::read(installed.join("a")).unwrap(), b"one");
    }

    #[test]
    fn a_cancelled_install_records_nothing_and_the_next_one_finishes() {
        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let store = store_for(temp.path(), APP);
        let weights = vec![3_u8; 1024 * 1024];
        let asset = mirror.publish(
            "component-clip-b33cedf-any-any.tar",
            &tar_of(&[("model.safetensors", &weights, 0o644)]),
            weights.len() as u64,
            "tar",
        );
        let manifest = verified(
            &store,
            vec![listing("clip", "b33cedf", ">=0.6.0-0", vec![asset])],
        );

        let cancel = AtomicBool::new(false);
        let result = store.install(
            ComponentId::Clip,
            Some(&manifest),
            &mirror.source(),
            ClipSource::GitHub,
            &mut |received, _| {
                if received >= 256 * 1024 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert!(result.is_err());
        assert_eq!(store.receipt(ComponentId::Clip), None);
        assert_eq!(store.locate(ComponentId::Clip), None);
        let sha256 = &manifest.component(ComponentId::Clip).unwrap().asset.sha256;
        let partial = temp.path().join(format!("downloads/{sha256}.partial"));
        assert!(partial.exists());

        cancel.store(false, Ordering::Relaxed);
        attempt(&store, ComponentId::Clip, &manifest, &mirror).unwrap();
        let dir = store.locate(ComponentId::Clip).unwrap();
        assert_eq!(fs::read(dir.join("model.safetensors")).unwrap(), weights);
    }

    #[test]
    fn a_receipt_for_other_qrate_versions_is_not_used() {
        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let store = store_for(temp.path(), APP);
        let manifest = pdfium(&mirror, &store, "1", &tar_of(&[("a", b"one", 0o644)]));
        attempt(&store, ComponentId::Pdfium, &manifest, &mirror).unwrap();

        let updated = store_for(temp.path(), "0.7.0-beta.1");
        assert!(updated.receipt(ComponentId::Pdfium).is_some());
        assert_eq!(updated.locate(ComponentId::Pdfium), None);
        let patched = store_for(temp.path(), "0.6.2");
        assert!(patched.locate(ComponentId::Pdfium).is_some());
    }

    #[test]
    fn removes_and_sweeps_what_no_receipt_names() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mirror = Mirror::new();
        let store = store_for(root, APP);
        let manifest = pdfium(&mirror, &store, "1", &tar_of(&[("a", b"one", 0o644)]));
        attempt(&store, ComponentId::Pdfium, &manifest, &mirror).unwrap();
        let dir = store.locate(ComponentId::Pdfium).unwrap();

        let before = generation();
        assert_eq!(store.remove(ComponentId::Pdfium).unwrap(), Removal::Removed);
        assert!(generation() > before);
        assert_eq!(store.locate(ComponentId::Pdfium), None);
        assert_eq!(store.receipt(ComponentId::Pdfium), None);
        assert!(!dir.exists());
        assert_eq!(store.remove(ComponentId::Pdfium).unwrap(), Removal::Removed);

        attempt(&store, ComponentId::Pdfium, &manifest, &mirror).unwrap();
        for leftover in [
            "pdfium/.staging-1",
            "pdfium/0",
            "ffmpeg/.trash-1",
            "downloads",
        ] {
            fs::create_dir_all(root.join(leftover)).unwrap();
        }
        let current = format!("manifest-{APP}.json");
        for file in [
            current.as_str(),
            "manifest-0.5.0.json",
            "manifest-0.6.0-beta.1.tmp",
            "downloads/finished",
            "downloads/old.partial",
            "downloads/fresh.partial",
        ] {
            fs::write(root.join(file), b"x").unwrap();
        }
        fs::File::options()
            .write(true)
            .open(root.join("downloads/old.partial"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() - KEEP_PARTIAL * 2)
            .unwrap();

        store.sweep();

        let names = |dir: &str| {
            let mut names: Vec<_> = fs::read_dir(root.join(dir))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(names("pdfium"), ["1"]);
        assert!(names("ffmpeg").is_empty());
        assert_eq!(names("downloads"), ["fresh.partial"]);
        assert!(root.join(&current).exists());
        assert!(!root.join("manifest-0.5.0.json").exists());
        assert!(!root.join("manifest-0.6.0-beta.1.tmp").exists());
        assert!(store.locate(ComponentId::Pdfium).is_some());
    }

    #[cfg(windows)]
    #[test]
    fn a_component_in_use_is_deleted_at_the_next_start() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let store = store_for(temp.path(), APP);
        let archive = tar_of(&[("pdfium.dll", b"dll", 0o644)]);
        let manifest = pdfium(&mirror, &store, "1", &archive);
        attempt(&store, ComponentId::Pdfium, &manifest, &mirror).unwrap();
        let dir = store.locate(ComponentId::Pdfium).unwrap();

        // Opened the way a loaded library is: nothing may move or delete it meanwhile.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(dir.join("pdfium.dll"))
            .unwrap();
        assert_eq!(
            store.remove(ComponentId::Pdfium).unwrap(),
            Removal::AfterRestart
        );
        assert_eq!(store.locate(ComponentId::Pdfium), None);
        assert!(dir.exists());

        drop(held);
        store.sweep();
        assert!(!dir.exists());
    }

    #[test]
    fn clip_sources_follow_the_setting() {
        assert_eq!(ClipSource::from_setting(None), ClipSource::HuggingFace);
        assert_eq!(
            ClipSource::from_setting(Some("bogus")),
            ClipSource::HuggingFace
        );
        for source in [ClipSource::HuggingFace, ClipSource::GitHub] {
            assert_eq!(ClipSource::from_setting(Some(source.setting())), source);
        }

        let temp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new();
        let store = store_for(temp.path(), APP);
        let asset = mirror.publish("component-clip-b33cedf-any-any.tar", &tar_of(&[]), 1, "tar");
        let manifest = verified(
            &store,
            vec![listing("clip", "b33cedf", ">=0.6.0-0", vec![asset])],
        );
        let first_urls = |id, manifest, clip| -> Vec<String> {
            origins(id, manifest, clip)
                .iter()
                .map(|origin| origin.fetches[0].url.clone())
                .collect()
        };
        let hugging_face = "https://huggingface.co/openai/clip-vit-base-patch32/resolve/";
        let preferred = first_urls(ComponentId::Clip, Some(&manifest), ClipSource::HuggingFace);
        assert!(preferred[0].starts_with(hugging_face));
        assert!(preferred[1].starts_with(RELEASE_DOWNLOADS));
        let reversed = first_urls(ComponentId::Clip, Some(&manifest), ClipSource::GitHub);
        assert_eq!(reversed, [preferred[1].clone(), preferred[0].clone()]);
        // Hugging Face needs no manifest, so a development build can still get the weights.
        let without = first_urls(ComponentId::Clip, None, ClipSource::GitHub);
        assert_eq!(without, [preferred[0].clone()]);
        assert!(first_urls(ComponentId::Pdfium, None, ClipSource::GitHub).is_empty());

        let missing = store
            .install(
                ComponentId::Pdfium,
                None,
                &mirror.source(),
                ClipSource::default(),
                &mut |_, _| {},
                &AtomicBool::new(false),
            )
            .unwrap_err();
        assert!(format!("{missing:#}").contains("no pdfium download"));
    }

    #[test]
    fn a_remembered_answer_is_found_again_only_when_the_generation_moves() {
        let slot = Mutex::new(None);
        let mut looked = 0;
        let mut look = |generation, answer: Option<&str>| {
            remember(&slot, generation, || {
                looked += 1;
                answer.map(PathBuf::from)
            })
        };
        // A miss is kept too, so a missing library is not probed on every card.
        assert_eq!(look(4, None), None);
        assert_eq!(look(4, Some("found")), None);
        assert_eq!(look(5, Some("found")), Some(PathBuf::from("found")));
        assert_eq!(look(5, None), Some(PathBuf::from("found")));
        assert_eq!(looked, 2);
    }

    #[test]
    fn adopts_weights_an_older_qrate_downloaded() {
        let temp = tempfile::tempdir().unwrap();
        let store = store_for(&temp.path().join("components"), APP);
        let legacy = temp.path().join("models/clip-vit-base-patch32");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("tokenizer.json"), b"words").unwrap();
        fs::write(legacy.join("model.safetensors"), b"weights").unwrap();
        let digest = |name: &str| updater::sha256_file(&legacy.join(name)).unwrap().0;
        let (words, weights) = (digest("tokenizer.json"), digest("model.safetensors"));
        let pins = [
            ("tokenizer.json", 5, words.as_str()),
            ("model.safetensors", 7, weights.as_str()),
        ];
        let before = generation();

        assert!(store.adopt(&legacy, &pins).unwrap());

        assert!(generation() > before);
        assert!(!legacy.exists());
        let dir = store.locate(ComponentId::Clip).unwrap();
        assert!(dir.ends_with(format!("clip/{CLIP_VERSION}")));
        assert_eq!(fs::read(dir.join("model.safetensors")).unwrap(), b"weights");
        let receipt = store.receipt(ComponentId::Clip).unwrap();
        assert_eq!((receipt.bytes, receipt.sha256), (12, weights.clone()));
        assert_eq!(receipt.app, VersionReq::STAR);
        // Once adopted there is nothing left to adopt.
        assert!(!store.adopt(&legacy, &pins).unwrap());
    }

    #[test]
    fn deletes_legacy_weights_that_are_not_the_pinned_ones() {
        let temp = tempfile::tempdir().unwrap();
        let store = store_for(&temp.path().join("components"), APP);
        let legacy = temp.path().join("models/clip-vit-base-patch32");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("tokenizer.json"), b"words").unwrap();
        fs::write(legacy.join("model.part"), b"half a download").unwrap();
        let zeros = "0".repeat(64);
        let words = updater::sha256_file(&legacy.join("tokenizer.json"))
            .unwrap()
            .0;
        let pins = [
            ("tokenizer.json", 5, words.as_str()),
            ("model.safetensors", 7, zeros.as_str()),
        ];

        assert!(!store.adopt(&legacy, &pins).unwrap());

        assert!(!legacy.exists());
        assert_eq!(store.receipt(ComponentId::Clip), None);
        assert!(!store.adopt(&legacy, &pins).unwrap());
    }
}

//! Trusted update metadata, installation provenance, and post-exit update jobs.

#[cfg(feature = "client")]
use std::time::Duration;
use std::{
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const AUTO_UPDATE_KEY: &str = "automatic_updates";
pub const ENVELOPE_SCHEMA: u32 = 1;
pub const MARKER_NAME: &str = "qrate-install.json";
pub const JOB_NAME: &str = "pending-update.json";
pub const RECEIPT_NAME: &str = "update-receipt.json";

const UPDATE_PUBLIC_KEY: [u8; 32] = [
    0x9c, 0xc1, 0x33, 0x97, 0x97, 0xa9, 0xc4, 0xe8, 0xd7, 0xb5, 0x3c, 0xb5, 0x5f, 0x6b, 0x4b, 0x04,
    0x7c, 0x9c, 0x7d, 0x1a, 0x69, 0x8d, 0x7d, 0x92, 0x03, 0xea, 0x04, 0x21, 0x37, 0x58, 0x67, 0xe4,
];
const UPDATE_KEY_ID: &str = "qrate-update-1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SignedEnvelope {
    pub schema: u32,
    pub key_id: String,
    pub payload_base64: String,
    pub signature_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateManifest {
    pub channel: ReleaseChannel,
    pub version: Version,
    pub published_at: String,
    pub release_notes_url: String,
    pub artifacts: Vec<UpdateArtifact>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseChannel {
    Beta,
    Stable,
}

impl ReleaseChannel {
    pub fn for_version(version: &Version) -> Self {
        if version.pre.is_empty() {
            Self::Stable
        } else {
            Self::Beta
        }
    }

    pub fn accepts(self, version: &Version) -> bool {
        self == Self::Beta || version.pre.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateArtifact {
    pub kind: InstallKind,
    pub os: String,
    pub arch: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallKind {
    WindowsNsis,
    WindowsPortable,
    WindowsMsi,
    MacosBundle,
    LinuxTar,
}

impl InstallKind {
    pub fn self_managed(self) -> bool {
        !matches!(self, Self::WindowsMsi)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstallMarker {
    pub schema: u32,
    pub kind: InstallKind,
    pub packaged_version: Version,
}

#[derive(Clone, Debug)]
pub struct Installation {
    pub root: PathBuf,
    pub executable: PathBuf,
    pub marker: InstallMarker,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateJob {
    pub schema: u32,
    pub envelope: SignedEnvelope,
    pub artifact_path: PathBuf,
    pub install_root: PathBuf,
    pub executable: PathBuf,
    pub expected_current_version: Version,
    pub target_version: Version,
}

#[derive(Clone, Debug)]
pub struct StagedUpdate {
    pub envelope: SignedEnvelope,
    pub path: PathBuf,
    pub version: Version,
    pub release_notes_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateReceipt {
    pub version: Version,
    pub status: ReceiptStatus,
    pub message: String,
    pub backup_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    AwaitingHealth,
    Healthy,
    Failed,
}

pub fn verify_envelope(envelope: &SignedEnvelope) -> Result<(UpdateManifest, Vec<u8>)> {
    let key =
        VerifyingKey::from_bytes(&UPDATE_PUBLIC_KEY).context("invalid embedded update key")?;
    verify_with(&key, envelope)
}

/// The trust root is a parameter so the signature rules can be exercised against a test key —
/// the shipped private key exists only in the release environment.
fn verify_with(key: &VerifyingKey, envelope: &SignedEnvelope) -> Result<(UpdateManifest, Vec<u8>)> {
    ensure!(
        envelope.schema == ENVELOPE_SCHEMA,
        "unsupported update envelope schema"
    );
    ensure!(
        envelope.key_id == UPDATE_KEY_ID,
        "unknown update signing key"
    );
    let payload = STANDARD
        .decode(&envelope.payload_base64)
        .context("invalid update payload encoding")?;
    let signature = Signature::from_slice(
        &STANDARD
            .decode(&envelope.signature_base64)
            .context("invalid update signature encoding")?,
    )
    .context("invalid update signature length")?;
    key.verify(&payload, &signature)
        .context("update manifest signature did not verify")?;
    let manifest = serde_json::from_slice(&payload).context("invalid signed update manifest")?;
    Ok((manifest, payload))
}

pub fn artifact_for<'a>(
    manifest: &'a UpdateManifest,
    installation: &Installation,
) -> Result<&'a UpdateArtifact> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    manifest
        .artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == installation.marker.kind
                && artifact.os == os
                && (artifact.arch == arch || artifact.arch == "universal")
        })
        .context("release has no artifact for this installation")
}

pub fn validate_update(
    envelope: &SignedEnvelope,
    installation: &Installation,
    current_version: &Version,
) -> Result<Option<(UpdateManifest, UpdateArtifact)>> {
    let (manifest, _) = verify_envelope(envelope)?;
    let Some(artifact) = select_update(&manifest, installation, current_version)? else {
        return Ok(None);
    };
    Ok(Some((manifest, artifact)))
}

#[cfg(feature = "client")]
pub fn fetch_and_stage(
    feed: &str,
    installation: &Installation,
    current: &Version,
    mut found: impl FnMut(Version),
    mut progress: impl FnMut(u64, u64),
) -> Result<Option<StagedUpdate>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("qrate-updater")
        .timeout(Duration::from_secs(30))
        .build()?;
    let envelope: SignedEnvelope = client.get(feed).send()?.error_for_status()?.json()?;
    let Some((manifest, artifact)) = validate_update(&envelope, installation, current)? else {
        return Ok(None);
    };
    found(manifest.version.clone());
    let dir = updates_dir()?.join(manifest.version.to_string());
    fs::create_dir_all(&dir)?;
    let filename = artifact
        .url
        .rsplit('/')
        .next()
        .context("artifact URL has no filename")?;
    let final_path = dir.join(filename);
    if final_path.exists() && verify_artifact(&final_path, &artifact).is_ok() {
        return Ok(Some(StagedUpdate {
            envelope,
            path: final_path,
            version: manifest.version,
            release_notes_url: manifest.release_notes_url,
        }));
    }

    let partial = final_path.with_extension("partial");
    let mut response = client.get(&artifact.url).send()?.error_for_status()?;
    if let Some(content_length) = response.content_length() {
        ensure!(
            content_length == artifact.size,
            "server content length differs from signed manifest"
        );
    }
    let mut output = fs::File::create(&partial)?;
    let mut received = 0_u64;
    let mut reported_percent = None;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        received += read as u64;
        ensure!(received <= artifact.size, "download exceeded signed size");
        output.write_all(&buffer[..read])?;
        let percent = received.saturating_mul(100) / artifact.size;
        if reported_percent != Some(percent) {
            reported_percent = Some(percent);
            progress(received, artifact.size);
        }
    }
    output.sync_all()?;
    verify_artifact(&partial, &artifact)?;
    fs::rename(&partial, &final_path)?;
    Ok(Some(StagedUpdate {
        envelope,
        path: final_path,
        version: manifest.version,
        release_notes_url: manifest.release_notes_url,
    }))
}

/// Channel, ordering, and artifact rules on an already-verified manifest.
pub fn select_update(
    manifest: &UpdateManifest,
    installation: &Installation,
    current_version: &Version,
) -> Result<Option<UpdateArtifact>> {
    let channel = ReleaseChannel::for_version(current_version);
    ensure!(
        channel.accepts(&manifest.version),
        "release is not in this build's channel"
    );
    if manifest.version <= *current_version {
        return Ok(None);
    }
    let artifact = artifact_for(manifest, installation)?.clone();
    ensure!(
        artifact
            .url
            .starts_with("https://github.com/devnull03/qrate/releases/download/"),
        "unexpected update download host"
    );
    ensure!(
        artifact.sha256.len() == 64 && artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid artifact digest"
    );
    ensure!(
        artifact.size > 0 && artifact.size <= 1_000_000_000,
        "invalid artifact size"
    );
    Ok(Some(artifact))
}

pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut size = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

pub fn verify_artifact(path: &Path, artifact: &UpdateArtifact) -> Result<()> {
    let (digest, size) = sha256_file(path)?;
    ensure!(size == artifact.size, "download size mismatch");
    ensure!(
        digest.eq_ignore_ascii_case(&artifact.sha256),
        "download checksum mismatch"
    );
    Ok(())
}

pub fn detect_installation() -> Result<Installation> {
    let executable = std::env::current_exe().context("locate qrate executable")?;
    let mut candidates = Vec::new();
    if let Some(parent) = executable.parent() {
        candidates.push((parent.to_path_buf(), parent.join(MARKER_NAME)));
        #[cfg(target_os = "macos")]
        if parent.ends_with("Contents/MacOS")
            && let Some(root) = parent.parent().and_then(Path::parent)
        {
            candidates.push((
                root.to_path_buf(),
                root.join("Contents/Resources").join(MARKER_NAME),
            ));
        }
    }
    for (root, marker_path) in candidates {
        let Ok(bytes) = fs::read(&marker_path) else {
            continue;
        };
        let marker: InstallMarker = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid install marker {}", marker_path.display()))?;
        ensure!(marker.schema == 1, "unsupported install marker schema");
        return Ok(Installation {
            root,
            executable,
            marker,
        });
    }
    bail!("this build is not a self-managed qrate installation")
}

pub fn updates_dir() -> Result<PathBuf> {
    let base = dirs_fallback().context("resolve application data directory")?;
    Ok(base.join("qrate").join("updates"))
}

fn dirs_fallback() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    return std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    return std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    return std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
}

pub fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("path has no parent")?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}

pub fn run_job(job_path: &Path) -> Result<()> {
    let key =
        VerifyingKey::from_bytes(&UPDATE_PUBLIC_KEY).context("invalid embedded update key")?;
    let job = fs::read(job_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<UpdateJob>(&bytes).ok());
    let result = run_job_with(job_path, &key, &updates_dir()?, spawn_installed);
    if result.is_err()
        && let Some(job) = job
    {
        let _ = spawn_installed(&job, &job.executable);
    }
    result
}

/// The trust key, the state directory, and the relaunch are parameters so an update can be applied
/// to a throwaway installation in a test — including the failure that has to roll one back.
fn run_job_with(
    job_path: &Path,
    key: &VerifyingKey,
    updates: &Path,
    launch: impl FnOnce(&UpdateJob, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let job: UpdateJob = serde_json::from_slice(&fs::read(job_path)?)?;
    let result = (|| {
        ensure!(job.schema == 1, "unsupported update job schema");
        let installation = installation_at(&job.install_root, &job.executable)?;
        ensure!(
            installation.marker.packaged_version == job.expected_current_version,
            "installed version changed after staging"
        );
        let (manifest, _) = verify_with(key, &job.envelope)?;
        ensure!(
            manifest.version == job.target_version,
            "update job version mismatch"
        );
        let artifact = artifact_for(&manifest, &installation)?;
        verify_artifact(&job.artifact_path, artifact)?;

        match installation.marker.kind {
            InstallKind::WindowsNsis => apply_nsis(&job),
            InstallKind::WindowsPortable => apply_zip(&job, updates, launch),
            InstallKind::MacosBundle => apply_macos(&job, updates, launch),
            InstallKind::LinuxTar => apply_tar(&job, updates, launch),
            InstallKind::WindowsMsi => bail!("MSI installations are administrator-managed"),
        }
    })();
    if let Err(error) = &result {
        let receipt = UpdateReceipt {
            version: job.target_version.clone(),
            status: ReceiptStatus::Failed,
            message: format!("{error:#}"),
            backup_path: None,
        };
        let _ = write_json_atomic(&updates.join(RECEIPT_NAME), &receipt);
    }
    result
}

fn installation_at(root: &Path, executable: &Path) -> Result<Installation> {
    let root = root.canonicalize()?;
    let executable = executable.canonicalize()?;
    ensure!(
        executable.starts_with(&root),
        "qrate executable is outside its marked installation"
    );
    // Probed, not chosen by host OS: the marker's location is a property of the package layout —
    // beside the executable for flat packages, under Resources for a macOS bundle.
    let bytes = [
        root.join(MARKER_NAME),
        root.join("Contents/Resources").join(MARKER_NAME),
    ]
    .iter()
    .find_map(|path| fs::read(path).ok())
    .context("installation has no qrate marker")?;
    let marker: InstallMarker = serde_json::from_slice(&bytes)?;
    ensure!(
        marker.schema == 1 && marker.kind.self_managed(),
        "installation is not self-managed"
    );
    Ok(Installation {
        root,
        executable,
        marker,
    })
}

fn apply_nsis(_job: &UpdateJob) -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    bail!("NSIS updates are only supported on Windows");
    #[cfg(target_os = "windows")]
    {
        let job = _job;
        let status = Command::new(&job.artifact_path)
            .args(["/S", "/UPDATE=1", "/RESTART=1"])
            .status()
            .context("run qrate update installer")?;
        ensure!(
            status.success(),
            "qrate update installer failed with {status}"
        );
        Ok(())
    }
}

fn apply_zip(
    job: &UpdateJob,
    updates: &Path,
    launch: impl FnOnce(&UpdateJob, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let temp = tempfile::Builder::new()
        .prefix("qrate-update-")
        .tempdir_in(
            job.install_root
                .parent()
                .context("install root has no parent")?,
        )?;
    let file = fs::File::open(&job.artifact_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry.enclosed_name().context("unsafe path in update zip")?;
        let output = temp.path().join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
        } else {
            fs::create_dir_all(output.parent().context("zip entry has no parent")?)?;
            std::io::copy(&mut entry, &mut fs::File::create(output)?)?;
        }
    }
    swap_and_launch(job, temp.keep(), updates, launch)
}

fn apply_tar(
    job: &UpdateJob,
    updates: &Path,
    launch: impl FnOnce(&UpdateJob, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let temp = tempfile::Builder::new()
        .prefix("qrate-update-")
        .tempdir_in(
            job.install_root
                .parent()
                .context("install root has no parent")?,
        )?;
    let decoder = flate2::read::GzDecoder::new(fs::File::open(&job.artifact_path)?);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(temp.path())?;
    let mut entries = fs::read_dir(temp.path())?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        entries.len() == 1 && entries[0].file_type()?.is_dir(),
        "Linux update must contain one application directory"
    );
    let staged = entries.pop().unwrap().path();
    swap_and_launch(job, staged, updates, launch)
}

fn apply_macos(
    _job: &UpdateJob,
    _updates: &Path,
    _launch: impl FnOnce(&UpdateJob, &Path) -> std::io::Result<()>,
) -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    bail!("DMG updates are only supported on macOS");
    #[cfg(target_os = "macos")]
    {
        let job = _job;
        let mount = tempfile::tempdir()?;
        let status = Command::new("hdiutil")
            .args(["attach", "-nobrowse", "-mountroot"])
            .arg(mount.path())
            .arg(&job.artifact_path)
            .status()?;
        ensure!(status.success(), "mount qrate update DMG");
        let source = fs::read_dir(mount.path())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
            .context("update DMG contains no app bundle")?;
        let staged = job.install_root.with_extension("qrate-new.app");
        if staged.exists() {
            fs::remove_dir_all(&staged)?;
        }
        let status = Command::new("ditto").arg(&source).arg(&staged).status()?;
        let _ = Command::new("hdiutil")
            .arg("detach")
            .arg(mount.path())
            .status();
        ensure!(status.success(), "stage qrate app bundle");
        swap_and_launch(job, staged, _updates, _launch)
    }
}

fn swap_and_launch(
    job: &UpdateJob,
    staged: PathBuf,
    updates: &Path,
    launch: impl FnOnce(&UpdateJob, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let backup = job.install_root.with_extension("qrate-old");
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    fs::rename(&job.install_root, &backup).context("move current installation to backup")?;
    if let Err(error) = fs::rename(&staged, &job.install_root) {
        let _ = fs::rename(&backup, &job.install_root);
        return Err(error).context("move staged update into place");
    }
    let relative_executable = job
        .executable
        .strip_prefix(&job.install_root)
        .context("executable is outside installation")?;
    let new_executable = job.install_root.join(relative_executable);
    let receipt = UpdateReceipt {
        version: job.target_version.clone(),
        status: ReceiptStatus::AwaitingHealth,
        message: "Update installed; waiting for qrate to start".into(),
        backup_path: Some(backup.clone()),
    };
    write_json_atomic(&updates.join(RECEIPT_NAME), &receipt)?;
    if let Err(error) = launch(job, &new_executable) {
        let _ = fs::remove_dir_all(&job.install_root);
        let _ = fs::rename(&backup, &job.install_root);
        return Err(error).context("launch updated qrate");
    }
    Ok(())
}

fn spawn_installed(job: &UpdateJob, executable: &Path) -> std::io::Result<()> {
    if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(&job.install_root)
            .spawn()
            .map(drop)
    } else {
        Command::new(executable).spawn().map(drop)
    }
}

pub fn mark_healthy(current_version: &Version) -> Result<Option<UpdateReceipt>> {
    mark_healthy_in(&updates_dir()?, current_version)
}

fn mark_healthy_in(updates: &Path, current_version: &Version) -> Result<Option<UpdateReceipt>> {
    let path = updates.join(RECEIPT_NAME);
    let Ok(bytes) = fs::read(&path) else {
        return Ok(None);
    };
    let mut receipt: UpdateReceipt = serde_json::from_slice(&bytes)?;
    if receipt.status == ReceiptStatus::AwaitingHealth && receipt.version == *current_version {
        if let Some(backup) = receipt.backup_path.take()
            && backup.exists()
        {
            fs::remove_dir_all(backup)?;
        }
        receipt.status = ReceiptStatus::Healthy;
        receipt.message = "Update installed successfully".into();
        write_json_atomic(&path, &receipt)?;
        // The job has been applied, so leaving it is both ~70 MB of installer nobody needs and a
        // job the helper would happily run a second time.
        let _ = fs::remove_file(updates.join(JOB_NAME));
        let _ = fs::remove_dir_all(updates.join(current_version.to_string()));
    }
    Ok(Some(receipt))
}

#[cfg(test)]
mod tests {
    use super::{
        ENVELOPE_SCHEMA, InstallKind, InstallMarker, Installation, ReleaseChannel, SignedEnvelope,
        UpdateArtifact, UpdateManifest, select_update, sha256_file, verify_with,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer as _, SigningKey};
    use semver::Version;
    use std::path::PathBuf;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn manifest(version_text: &str) -> UpdateManifest {
        UpdateManifest {
            channel: ReleaseChannel::Beta,
            version: version(version_text),
            published_at: "2026-08-26T00:00:00Z".into(),
            release_notes_url: "https://github.com/devnull03/qrate/releases/tag/v9.9.9".into(),
            artifacts: vec![UpdateArtifact {
                kind: InstallKind::LinuxTar,
                os: std::env::consts::OS.into(),
                arch: "universal".into(),
                url: format!(
                    "https://github.com/devnull03/qrate/releases/download/v{version_text}/qrate.tar.gz"
                ),
                size: 4_096,
                sha256: "a".repeat(64),
            }],
        }
    }

    fn installation() -> Installation {
        Installation {
            root: PathBuf::from("/opt/qrate"),
            executable: PathBuf::from("/opt/qrate/qrate"),
            marker: InstallMarker {
                schema: 1,
                kind: InstallKind::LinuxTar,
                packaged_version: version("0.4.0-alpha.1"),
            },
        }
    }

    fn sign(key: &SigningKey, manifest: &UpdateManifest) -> SignedEnvelope {
        let payload = serde_json::to_vec(manifest).unwrap();
        SignedEnvelope {
            schema: ENVELOPE_SCHEMA,
            key_id: super::UPDATE_KEY_ID.into(),
            signature_base64: STANDARD.encode(key.sign(&payload).to_bytes()),
            payload_base64: STANDARD.encode(payload),
        }
    }

    #[test]
    fn accepts_a_manifest_signed_by_the_trusted_key() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let envelope = sign(&key, &manifest("0.5.0-beta.1"));
        let (signed, _) = verify_with(&key.verifying_key(), &envelope).unwrap();
        assert_eq!(signed.version, version("0.5.0-beta.1"));
    }

    #[test]
    fn rejects_tampering_wrong_keys_and_unknown_schemas() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let good = sign(&key, &manifest("0.5.0-beta.1"));

        let tampered = SignedEnvelope {
            payload_base64: STANDARD.encode(serde_json::to_vec(&manifest("9.9.9")).unwrap()),
            ..good.clone()
        };
        assert!(verify_with(&key.verifying_key(), &tampered).is_err());

        let other = SigningKey::from_bytes(&[9; 32]);
        assert!(verify_with(&other.verifying_key(), &good).is_err());

        let future = SignedEnvelope {
            schema: ENVELOPE_SCHEMA + 1,
            ..good.clone()
        };
        assert!(verify_with(&key.verifying_key(), &future).is_err());

        let renamed = SignedEnvelope {
            key_id: "qrate-update-2".into(),
            ..good
        };
        assert!(verify_with(&key.verifying_key(), &renamed).is_err());
    }

    #[test]
    fn only_offers_a_newer_release_in_the_running_channel() {
        let install = installation();
        let current = version("0.4.0-alpha.1");

        assert!(
            select_update(&manifest("0.5.0-beta.1"), &install, &current)
                .unwrap()
                .is_some()
        );
        // Same and older releases are not updates, and neither is a prerelease for a stable build.
        assert!(
            select_update(&manifest("0.4.0-alpha.1"), &install, &current)
                .unwrap()
                .is_none()
        );
        assert!(
            select_update(&manifest("0.3.0"), &install, &current)
                .unwrap()
                .is_none()
        );
        assert!(select_update(&manifest("0.5.0-beta.1"), &install, &version("0.4.0")).is_err());
        // A stable release still reaches a prerelease build.
        assert!(
            select_update(&manifest("0.5.0"), &install, &current)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn rejects_artifacts_that_do_not_match_the_installation() {
        let install = installation();
        let current = version("0.4.0-alpha.1");

        let mut foreign = manifest("0.5.0-beta.1");
        foreign.artifacts[0].kind = InstallKind::WindowsNsis;
        assert!(select_update(&foreign, &install, &current).is_err());

        let mut offsite = manifest("0.5.0-beta.1");
        offsite.artifacts[0].url = "https://example.com/qrate.tar.gz".into();
        assert!(select_update(&offsite, &install, &current).is_err());

        let mut short_digest = manifest("0.5.0-beta.1");
        short_digest.artifacts[0].sha256 = "abc".into();
        assert!(select_update(&short_digest, &install, &current).is_err());

        let mut empty = manifest("0.5.0-beta.1");
        empty.artifacts[0].size = 0;
        assert!(select_update(&empty, &install, &current).is_err());
    }

    #[test]
    fn msi_installations_are_never_self_managed() {
        assert!(!InstallKind::WindowsMsi.self_managed());
        assert!(InstallKind::WindowsNsis.self_managed());
    }

    #[test]
    fn hashes_files() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"qrate").unwrap();
        let (hash, size) = sha256_file(file.path()).unwrap();
        assert_eq!(size, 5);
        assert_eq!(
            hash,
            "11ec30a948fa189f6f83df72eb26a0dff1cfd800d259f13326adbac46ac347fb"
        );
    }

    #[test]
    fn release_channel_follows_build_version() {
        assert_eq!(
            ReleaseChannel::for_version(&version("1.0.0-beta.1")),
            ReleaseChannel::Beta
        );
        assert_eq!(
            ReleaseChannel::for_version(&version("1.0.0")),
            ReleaseChannel::Stable
        );
        assert!(!ReleaseChannel::Stable.accepts(&version("1.1.0-beta.1")));
    }
}

/// Applying an update is the one thing here that destroys a working installation before it has a
/// new one, so these drive the real swap against throwaway trees rather than mocking it.
#[cfg(test)]
mod apply_tests {
    use super::{
        ENVELOPE_SCHEMA, InstallKind, InstallMarker, JOB_NAME, MARKER_NAME, RECEIPT_NAME,
        ReceiptStatus, ReleaseChannel, SignedEnvelope, UpdateArtifact, UpdateJob, UpdateManifest,
        UpdateReceipt, mark_healthy_in, run_job_with, sha256_file, write_json_atomic,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer as _, SigningKey};
    use semver::Version;
    use std::{
        fs,
        io::Write as _,
        path::{Path, PathBuf},
    };

    const FROM: &str = "0.4.0-alpha.1";
    const TO: &str = "0.5.0-beta.1";

    fn marker(version: &str) -> String {
        serde_json::to_string(&InstallMarker {
            schema: 1,
            kind: InstallKind::WindowsPortable,
            packaged_version: Version::parse(version).unwrap(),
        })
        .unwrap()
    }

    /// An installation of `version`: the marker the helper checks, the executable it relaunches,
    /// and one payload file that proves which version is actually on disk afterwards.
    fn install(root: &Path, version: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(MARKER_NAME), marker(version)).unwrap();
        fs::write(root.join("qrate.exe"), b"executable").unwrap();
        fs::write(root.join("payload.txt"), version).unwrap();
    }

    /// The portable package is a zip of the installation's *contents*, so build it that way.
    fn package(path: &Path, version: &str) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            (MARKER_NAME, marker(version)),
            ("qrate.exe", "executable".to_string()),
            ("payload.txt", version.to_string()),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn job(key: &SigningKey, root: &Path, artifact: &Path) -> UpdateJob {
        let (sha256, size) = sha256_file(artifact).unwrap();
        let manifest = UpdateManifest {
            channel: ReleaseChannel::Beta,
            version: Version::parse(TO).unwrap(),
            published_at: "2026-08-26T00:00:00Z".into(),
            release_notes_url: format!("https://github.com/devnull03/qrate/releases/tag/v{TO}"),
            artifacts: vec![UpdateArtifact {
                kind: InstallKind::WindowsPortable,
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                url: format!("https://github.com/devnull03/qrate/releases/download/v{TO}/q.zip"),
                size,
                sha256,
            }],
        };
        let payload = serde_json::to_vec(&manifest).unwrap();
        UpdateJob {
            schema: 1,
            envelope: SignedEnvelope {
                schema: ENVELOPE_SCHEMA,
                key_id: super::UPDATE_KEY_ID.into(),
                signature_base64: STANDARD.encode(key.sign(&payload).to_bytes()),
                payload_base64: STANDARD.encode(&payload),
            },
            artifact_path: artifact.to_path_buf(),
            install_root: root.to_path_buf(),
            executable: root.join("qrate.exe"),
            expected_current_version: Version::parse(FROM).unwrap(),
            target_version: Version::parse(TO).unwrap(),
        }
    }

    /// One staged update, ready to apply: the install tree, the signed job, and where the helper
    /// keeps its state.
    struct Staged {
        _home: tempfile::TempDir,
        key: SigningKey,
        root: PathBuf,
        updates: PathBuf,
        job_path: PathBuf,
    }

    fn stage() -> Staged {
        let home = tempfile::tempdir().unwrap();
        let key = SigningKey::from_bytes(&[7; 32]);
        let root = home.path().join("qrate");
        let updates = home.path().join("updates");
        install(&root, FROM);
        let artifact = home.path().join("qrate-next.zip");
        package(&artifact, TO);
        let job_path = updates.join(JOB_NAME);
        write_json_atomic(&job_path, &job(&key, &root, &artifact)).unwrap();
        Staged {
            _home: home,
            key,
            root,
            updates,
            job_path,
        }
    }

    fn receipt(updates: &Path) -> UpdateReceipt {
        serde_json::from_slice(&fs::read(updates.join(RECEIPT_NAME)).unwrap()).unwrap()
    }

    fn payload_version(root: &Path) -> String {
        fs::read_to_string(root.join("payload.txt")).unwrap()
    }

    #[test]
    fn applies_a_signed_update_and_keeps_the_old_install_until_it_starts() {
        let staged = stage();
        let mut launched = None;

        run_job_with(
            &staged.job_path,
            &staged.key.verifying_key(),
            &staged.updates,
            |_, executable| {
                launched = Some(executable.to_path_buf());
                Ok(())
            },
        )
        .unwrap();

        // The whole tree moved, not just the executable — a partial swap is the version skew this
        // design exists to prevent.
        assert_eq!(payload_version(&staged.root), TO);
        assert!(
            fs::read_to_string(staged.root.join(MARKER_NAME))
                .unwrap()
                .contains(TO)
        );
        assert_eq!(launched.unwrap(), staged.root.join("qrate.exe"));

        // The previous install is kept until the new one proves it starts.
        let backup = staged.root.with_extension("qrate-old");
        assert!(backup.exists());
        let pending = receipt(&staged.updates);
        assert_eq!(pending.status, ReceiptStatus::AwaitingHealth);
        assert_eq!(pending.backup_path.as_deref(), Some(backup.as_path()));

        let healthy = mark_healthy_in(&staged.updates, &Version::parse(TO).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(healthy.status, ReceiptStatus::Healthy);
        assert!(
            !backup.exists(),
            "a healthy start should reclaim the backup"
        );
        assert!(
            !staged.job_path.exists(),
            "an applied job must not be left where the helper would run it again"
        );
        assert!(
            !staged.updates.join(TO).exists(),
            "the downloaded installer is dead weight once it is installed"
        );
    }

    #[test]
    fn restores_the_previous_install_when_the_update_will_not_start() {
        let staged = stage();

        let error = run_job_with(
            &staged.job_path,
            &staged.key.verifying_key(),
            &staged.updates,
            |_, _| Err(std::io::Error::other("could not launch")),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("launch"));

        // Rolled all the way back: the old version runs again and nothing is left half-swapped.
        assert_eq!(payload_version(&staged.root), FROM);
        assert!(
            fs::read_to_string(staged.root.join(MARKER_NAME))
                .unwrap()
                .contains(FROM)
        );
        assert!(!staged.root.with_extension("qrate-old").exists());
        assert_eq!(receipt(&staged.updates).status, ReceiptStatus::Failed);
    }

    #[test]
    fn refuses_an_update_signed_by_the_wrong_key() {
        let staged = stage();

        let error = run_job_with(
            &staged.job_path,
            &SigningKey::from_bytes(&[9; 32]).verifying_key(),
            &staged.updates,
            |_, _| panic!("an unverified update must never reach the swap"),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("signature"));
        assert_eq!(payload_version(&staged.root), FROM);
        assert_eq!(receipt(&staged.updates).status, ReceiptStatus::Failed);
    }

    #[test]
    fn refuses_an_artifact_that_does_not_match_its_signed_digest() {
        let staged = stage();
        let job: UpdateJob = serde_json::from_slice(&fs::read(&staged.job_path).unwrap()).unwrap();
        package(&job.artifact_path, "9.9.9-tampered");

        let error = run_job_with(
            &staged.job_path,
            &staged.key.verifying_key(),
            &staged.updates,
            |_, _| panic!("a tampered artifact must never reach the swap"),
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("checksum") || message.contains("size"),
            "{message}"
        );
        assert_eq!(payload_version(&staged.root), FROM);
        assert_eq!(receipt(&staged.updates).status, ReceiptStatus::Failed);
    }

    #[test]
    fn refuses_an_installation_that_moved_on_since_staging() {
        let staged = stage();
        // Something else updated qrate between the download and the restart.
        fs::write(staged.root.join(MARKER_NAME), marker("0.4.5")).unwrap();

        let error = run_job_with(
            &staged.job_path,
            &staged.key.verifying_key(),
            &staged.updates,
            |_, _| panic!("a stale job must never reach the swap"),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("installed version changed"));
        assert_eq!(receipt(&staged.updates).status, ReceiptStatus::Failed);
    }
}

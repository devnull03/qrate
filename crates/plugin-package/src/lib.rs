//! Verifies, installs, updates, and removes qrate plugin release packages.

use std::collections::HashSet;
use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail, ensure};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const CATALOG_KEY_ID: &str = "qrate-plugin-catalog-1";
pub const CATALOG_URL: &str = "https://devnull03.github.io/qrate-plugin-registry/catalog.json";
pub const MAX_CATALOG_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 500 * 1024 * 1024;
pub const MAX_PACKAGE_FILES: usize = 10_000;
pub const SUPPORTED_API_VERSION: u64 = 1;
pub const ACCEPTED_LICENSES: &[&str] = &[
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "CC-BY-4.0",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "LGPL-3.0-only",
    "LGPL-3.0-or-later",
    "MIT",
    "Unlicense",
    "Zlib",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackageManifest {
    pub schema: u64,
    pub id: String,
    pub name: String,
    pub version: Version,
    pub api_version: u64,
    pub entry: String,
    pub description: String,
    pub homepage: String,
    pub license: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Catalog {
    pub schema: u64,
    pub generated_at: String,
    pub source_commit: String,
    pub plugins: Vec<CatalogPlugin>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CatalogPlugin {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    pub license: String,
    pub publisher: String,
    pub repository: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub support: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub featured: bool,
    pub current: CatalogRelease,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CatalogRelease {
    pub version: Version,
    pub api_version: u64,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub published_at: String,
    pub release_url: String,
    pub artifact_url: String,
    pub sha256: String,
    pub bytes: u64,
    pub status: ReleaseStatus,
    #[serde(default)]
    pub revocation_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseStatus {
    Active,
    Revoked,
}

#[derive(Debug, Deserialize)]
pub struct CatalogSignature {
    pub schema: u64,
    pub key_id: String,
    pub algorithm: String,
    pub sha256: String,
    pub signature_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstallReceipt {
    pub schema: u64,
    pub id: String,
    pub version: Version,
    pub source: InstallSource,
    pub repository: String,
    pub artifact_url: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct PackageInspection {
    pub manifest: PackageManifest,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct DirectRelease {
    pub repository: String,
    pub version: String,
    pub release_url: String,
    pub artifact_url: String,
    pub artifact_name: String,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallSource {
    OfficialCatalog,
    DirectGithub,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallTarget {
    Registry(String),
    Github(String),
}

pub fn parse_install_link(value: &str) -> Result<InstallTarget> {
    let url = reqwest::Url::parse(value).context("invalid qrate install link")?;
    ensure!(
        url.scheme() == "qrate"
            && url.host_str() == Some("plugin")
            && url.path() == "/install"
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "unsupported qrate install link"
    );
    let pairs: Vec<_> = url.query_pairs().collect();
    let one = |name: &str| -> Result<Option<String>> {
        let values: Vec<_> = pairs
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.to_string())
            .collect();
        ensure!(values.len() <= 1, "duplicate {name} parameter");
        Ok(values.into_iter().next())
    };
    let source = one("source")?.context("install link has no source")?;
    let allowed: &[&str] = match source.as_str() {
        "registry" => &["source", "id"],
        "github" => &["source", "repo"],
        "url" => &["source", "release"],
        _ => bail!("unsupported install source"),
    };
    ensure!(
        pairs.iter().all(|(key, _)| allowed.contains(&key.as_ref())),
        "install link has an unknown parameter"
    );
    match source.as_str() {
        "registry" => {
            let id = one("id")?.context("registry install link has no plugin ID")?;
            validate_id(&id)?;
            Ok(InstallTarget::Registry(id))
        }
        "github" => {
            let repo = one("repo")?.context("GitHub install link has no repository")?;
            let mut parts = repo.split('/');
            let owner = parts.next().unwrap_or_default();
            let repository = parts.next().unwrap_or_default();
            ensure!(
                !owner.is_empty() && !repository.is_empty() && parts.next().is_none(),
                "GitHub repository must be owner/name"
            );
            let source = format!("https://github.com/{owner}/{repository}");
            validate_github_url(&source)?;
            Ok(InstallTarget::Github(source))
        }
        "url" => {
            let release = one("release")?.context("GitHub install link has no release URL")?;
            validate_github_url(&release)?;
            Ok(InstallTarget::Github(release))
        }
        _ => unreachable!(),
    }
}

pub fn public_key(base64url: &str) -> Result<VerifyingKey> {
    let bytes: [u8; 32] = URL_SAFE_NO_PAD
        .decode(base64url)
        .context("invalid plugin catalog public key encoding")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid plugin catalog public key length"))?;
    VerifyingKey::from_bytes(&bytes).context("invalid plugin catalog public key")
}

pub fn verify_catalog(bytes: &[u8], signature_bytes: &[u8], key: &VerifyingKey) -> Result<Catalog> {
    ensure!(
        bytes.len() <= MAX_CATALOG_BYTES,
        "plugin catalog is too large"
    );
    let signature: CatalogSignature =
        serde_json::from_slice(signature_bytes).context("invalid plugin catalog signature")?;
    ensure!(
        signature.schema == 1,
        "unsupported plugin catalog signature schema"
    );
    ensure!(
        signature.key_id == CATALOG_KEY_ID,
        "unknown plugin catalog signing key"
    );
    ensure!(
        signature.algorithm == "Ed25519",
        "unsupported plugin catalog signature algorithm"
    );
    let digest = format!("{:x}", Sha256::digest(bytes));
    ensure!(
        digest.eq_ignore_ascii_case(&signature.sha256),
        "plugin catalog hash does not match"
    );
    let signature = Signature::from_slice(
        &STANDARD
            .decode(signature.signature_base64)
            .context("invalid plugin catalog signature encoding")?,
    )
    .context("invalid plugin catalog signature length")?;
    key.verify(bytes, &signature)
        .context("plugin catalog signature did not verify")?;

    let catalog: Catalog = serde_json::from_slice(bytes).context("invalid plugin catalog")?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

pub fn fetch_catalog(url: &str, key: &VerifyingKey) -> Result<Catalog> {
    let (bytes, signature) = fetch_catalog_files(url)?;
    verify_catalog(&bytes, &signature, key)
}

pub fn fetch_catalog_cached(url: &str, key: &VerifyingKey, cache_root: &Path) -> Result<Catalog> {
    let catalog_path = cache_root.join("catalog.json");
    let signature_path = cache_root.join("catalog.json.sig");
    match fetch_catalog_files(url).and_then(|(catalog, signature)| {
        let verified = verify_catalog(&catalog, &signature, key)?;
        fs::create_dir_all(cache_root)?;
        write_bytes_atomic(&catalog_path, &catalog)?;
        write_bytes_atomic(&signature_path, &signature)?;
        Ok(verified)
    }) {
        Ok(catalog) => Ok(catalog),
        Err(download_error) => {
            let catalog = fs::read(&catalog_path)
                .context("catalog refresh failed and no cached catalog is available")?;
            let signature = fs::read(&signature_path)
                .context("catalog refresh failed and no cached signature is available")?;
            verify_catalog(&catalog, &signature, key).with_context(|| {
                format!("catalog refresh failed ({download_error:#}) and cached catalog is invalid")
            })
        }
    }
}

fn fetch_catalog_files(url: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let bytes = bounded_response(client.get(url).send()?, MAX_CATALOG_BYTES as u64)
        .context("could not download plugin catalog")?;
    let signature = bounded_response(client.get(format!("{url}.sig")).send()?, 64 * 1024)
        .context("could not download plugin catalog signature")?;
    Ok((bytes, signature))
}

fn bounded_response(response: reqwest::blocking::Response, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        response.status().is_success(),
        "server returned HTTP {}",
        response.status()
    );
    if let Some(length) = response.content_length() {
        ensure!(length <= limit, "download is too large");
    }
    let mut bytes = Vec::new();
    response.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "download is too large");
    Ok(bytes)
}

pub fn download_package(url: &str, output: &Path) -> Result<(String, u64)> {
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?
        .get(url)
        .send()?;
    let bytes = bounded_response(response, MAX_PACKAGE_BYTES)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    fs::write(output, &bytes)?;
    Ok((sha256, bytes.len() as u64))
}

pub fn resolve_github_release(source: &str) -> Result<DirectRelease> {
    let url = validate_github_url(source)?;
    let parts: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
    let owner = parts[0];
    let repository = parts[1].trim_end_matches(".git");

    let endpoint = if parts.get(2..4) == Some(&["releases", "tag"]) && parts.len() == 5 {
        format!(
            "https://api.github.com/repos/{owner}/{repository}/releases/tags/{}",
            parts[4]
        )
    } else {
        ensure!(parts.len() == 2, "unsupported GitHub release URL");
        format!("https://api.github.com/repos/{owner}/{repository}/releases/latest")
    };
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
        size: u64,
    }
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
        assets: Vec<Asset>,
    }
    let release: Release = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("qrate-plugin-installer")
        .build()?
        .get(endpoint)
        .send()?
        .error_for_status()
        .context("GitHub could not resolve that release")?
        .json()?;
    let mut packages = release
        .assets
        .into_iter()
        .filter(|asset| asset.name.to_ascii_lowercase().ends_with(".zip"));
    let asset = packages
        .next()
        .context("GitHub release has no plugin ZIP asset")?;
    ensure!(
        packages.next().is_none(),
        "GitHub release has more than one ZIP asset"
    );
    ensure!(
        asset.size <= MAX_PACKAGE_BYTES,
        "plugin package is too large"
    );
    Ok(DirectRelease {
        repository: format!("https://github.com/{owner}/{repository}"),
        version: release.tag_name,
        release_url: release.html_url,
        artifact_url: asset.browser_download_url,
        artifact_name: asset.name,
        bytes: asset.size,
    })
}

fn validate_github_url(source: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(source).context("invalid GitHub URL")?;
    ensure!(
        url.scheme() == "https" && url.host_str() == Some("github.com"),
        "only public HTTPS GitHub repositories are supported"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "GitHub URL must not contain credentials"
    );
    let parts: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
    ensure!(
        parts.len() >= 2,
        "GitHub URL must name an owner and repository"
    );
    let owner = parts[0];
    let repository = parts[1].trim_end_matches(".git");
    ensure!(
        owner
            .bytes()
            .chain(repository.bytes())
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
        "GitHub owner or repository is invalid"
    );
    ensure!(
        parts.len() == 2 || (parts.get(2..4) == Some(&["releases", "tag"]) && parts.len() == 5),
        "unsupported GitHub release URL"
    );
    Ok(url)
}

pub fn inspect_archive(archive: &Path) -> Result<PackageInspection> {
    let (sha256, bytes) = sha256_file(archive)?;
    ensure!(bytes <= MAX_PACKAGE_BYTES, "plugin package is too large");
    let staging = tempfile::tempdir()?;
    extract_package(archive, staging.path())?;
    let manifest: PackageManifest = serde_json::from_slice(
        &fs::read(staging.path().join("qrate-plugin.json"))
            .context("plugin package has no qrate-plugin.json")?,
    )
    .context("invalid qrate-plugin.json")?;
    validate_manifest(&manifest, staging.path())?;
    Ok(PackageInspection {
        manifest,
        sha256,
        bytes,
    })
}

pub fn install_archive(
    archive: &Path,
    plugins_root: &Path,
    receipts_root: &Path,
    source: InstallSource,
    expected: Option<(&CatalogPlugin, &CatalogRelease)>,
) -> Result<InstallReceipt> {
    install_archive_inner(archive, plugins_root, receipts_root, source, expected, None)
}

pub fn install_direct_archive(
    archive: &Path,
    plugins_root: &Path,
    receipts_root: &Path,
    release: &DirectRelease,
) -> Result<InstallReceipt> {
    install_archive_inner(
        archive,
        plugins_root,
        receipts_root,
        InstallSource::DirectGithub,
        None,
        Some(release),
    )
}

fn install_archive_inner(
    archive: &Path,
    plugins_root: &Path,
    receipts_root: &Path,
    source: InstallSource,
    expected: Option<(&CatalogPlugin, &CatalogRelease)>,
    direct: Option<&DirectRelease>,
) -> Result<InstallReceipt> {
    let (sha256, bytes) = sha256_file(archive)?;
    ensure!(bytes <= MAX_PACKAGE_BYTES, "plugin package is too large");
    if let Some((_, release)) = expected {
        ensure!(
            release.status == ReleaseStatus::Active,
            "plugin release is revoked"
        );
        ensure!(
            bytes == release.bytes,
            "plugin package size does not match catalog"
        );
        ensure!(
            sha256.eq_ignore_ascii_case(&release.sha256),
            "plugin package hash does not match catalog"
        );
    }

    fs::create_dir_all(plugins_root)?;
    fs::create_dir_all(receipts_root)?;
    let staging = tempfile::Builder::new()
        .prefix("qrate-plugin-")
        .tempdir_in(plugins_root)?;
    extract_package(archive, staging.path())?;
    let manifest: PackageManifest = serde_json::from_slice(
        &fs::read(staging.path().join("qrate-plugin.json"))
            .context("plugin package has no qrate-plugin.json")?,
    )
    .context("invalid qrate-plugin.json")?;
    validate_manifest(&manifest, staging.path())?;

    if let Some((plugin, release)) = expected {
        ensure!(
            manifest.id == plugin.id,
            "package ID does not match catalog"
        );
        ensure!(
            manifest.name == plugin.name,
            "package name does not match catalog"
        );
        ensure!(
            manifest.version == release.version,
            "package version does not match catalog"
        );
        ensure!(
            manifest.api_version == release.api_version,
            "package API version does not match catalog"
        );
        ensure!(
            manifest.license == plugin.license,
            "package license does not match catalog"
        );
        ensure!(
            manifest.permissions == release.permissions,
            "package permissions do not match catalog"
        );
    }

    let receipt_path = receipt_path(receipts_root, &manifest.id)?;
    let target = plugins_root.join(&manifest.id);
    ensure!(
        !target.exists() || receipt_path.exists(),
        "an unmanaged plugin already uses this package ID"
    );
    if let Some(installed) = read_receipt(receipts_root, &manifest.id)? {
        ensure!(
            installed.source == source,
            "installed plugin comes from a different source"
        );
        if let Some((plugin, _)) = expected {
            ensure!(
                installed.repository == plugin.repository,
                "installed plugin belongs to a different repository"
            );
        }
        if let Some(release) = direct {
            ensure!(
                installed.repository == release.repository,
                "installed plugin belongs to a different repository"
            );
        }
    }
    let rollback = plugins_root.join(format!(".{}.rollback", manifest.id));
    if rollback.exists() {
        fs::remove_dir_all(&rollback)?;
    }
    if target.exists() {
        fs::rename(&target, &rollback).context("could not retain the installed plugin")?;
    }

    let staged = staging.keep();
    if let Err(error) = fs::rename(&staged, &target) {
        if rollback.exists() {
            let _ = fs::rename(&rollback, &target);
        }
        return Err(error).context("could not activate the plugin package");
    }

    let receipt = InstallReceipt {
        schema: 1,
        id: manifest.id,
        version: manifest.version,
        source,
        repository: expected.map_or_else(
            || {
                direct.map_or_else(
                    || manifest.homepage.clone(),
                    |release| release.repository.clone(),
                )
            },
            |(plugin, _)| plugin.repository.clone(),
        ),
        artifact_url: expected.map_or_else(
            || direct.map_or_else(String::new, |release| release.artifact_url.clone()),
            |(_, release)| release.artifact_url.clone(),
        ),
        sha256,
        bytes,
    };
    if let Err(error) = write_json_atomic(&receipt_path, &receipt) {
        let _ = fs::remove_dir_all(&target);
        if rollback.exists() {
            let _ = fs::rename(&rollback, &target);
        }
        return Err(error).context("could not record the plugin installation");
    }
    if rollback.exists() {
        fs::remove_dir_all(rollback)?;
    }
    Ok(receipt)
}

pub fn remove_managed(id: &str, plugins_root: &Path, receipts_root: &Path) -> Result<()> {
    let receipt = receipt_path(receipts_root, id)?;
    ensure!(receipt.exists(), "qrate does not manage this plugin");
    let target = plugins_root.join(id);
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::remove_file(receipt)?;
    Ok(())
}

pub fn receipts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugin-installs")
}

pub fn read_receipt(receipts_root: &Path, id: &str) -> Result<Option<InstallReceipt>> {
    let path = receipt_path(receipts_root, id)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn receipt_path(receipts_root: &Path, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    Ok(receipts_root.join(format!("{id}.json")))
}

fn validate_catalog(catalog: &Catalog) -> Result<()> {
    ensure!(catalog.schema == 1, "unsupported plugin catalog schema");
    let mut ids = HashSet::new();
    for plugin in &catalog.plugins {
        validate_id(&plugin.id)?;
        ensure!(ids.insert(&plugin.id), "duplicate plugin ID {}", plugin.id);
        ensure!(!plugin.name.trim().is_empty(), "plugin name is empty");
        ensure!(
            ACCEPTED_LICENSES.contains(&plugin.license.as_str()),
            "plugin {} uses an unsupported license",
            plugin.id
        );
        ensure!(
            plugin.current.api_version > 0 && plugin.current.api_version <= SUPPORTED_API_VERSION,
            "plugin {} needs an unsupported API version",
            plugin.id
        );
        ensure!(
            plugin.current.bytes <= MAX_PACKAGE_BYTES,
            "plugin {} package is too large",
            plugin.id
        );
        validate_sha256(&plugin.current.sha256)?;
        validate_https_url(&plugin.repository)?;
        validate_https_url(&plugin.current.release_url)?;
        validate_https_url(&plugin.current.artifact_url)?;
        ensure!(
            plugin.current.status == ReleaseStatus::Revoked
                || plugin.current.revocation_reason.is_none(),
            "active plugin {} has a revocation reason",
            plugin.id
        );
        ensure!(
            plugin
                .current
                .permissions
                .iter()
                .collect::<HashSet<_>>()
                .len()
                == plugin.current.permissions.len(),
            "duplicate permission in {}",
            plugin.id
        );
    }
    Ok(())
}

fn validate_manifest(manifest: &PackageManifest, root: &Path) -> Result<()> {
    ensure!(manifest.schema == 1, "unsupported package manifest schema");
    validate_id(&manifest.id)?;
    ensure!(!manifest.name.trim().is_empty(), "package name is empty");
    ensure!(
        manifest.api_version > 0 && manifest.api_version <= SUPPORTED_API_VERSION,
        "package needs an unsupported API version"
    );
    ensure!(
        ACCEPTED_LICENSES.contains(&manifest.license.as_str()),
        "package uses an unsupported license"
    );
    validate_https_url(&manifest.homepage)?;
    let mut entry = Path::new(&manifest.entry).components();
    ensure!(
        matches!(entry.next(), Some(Component::Normal(_))) && entry.next().is_none(),
        "package entry must be a root file"
    );
    ensure!(
        root.join(&manifest.entry).is_file(),
        "package entry does not exist"
    );
    ensure!(
        manifest
            .permissions
            .iter()
            .all(|permission| permission == "net"),
        "package requests an unknown permission"
    );
    ensure!(
        manifest.permissions.iter().collect::<HashSet<_>>().len() == manifest.permissions.len(),
        "package repeats a permission"
    );
    ensure!(
        fs::read_dir(root)?.flatten().any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_uppercase()
                    .starts_with("LICENSE")
        }),
        "plugin package has no root license file"
    );
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    let segments: Vec<_> = id.split('.').collect();
    ensure!(
        segments.len() >= 2
            && id.len() <= 128
            && segments.iter().all(|segment| {
                !segment.is_empty()
                    && segment
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && segment
                        .as_bytes()
                        .last()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'_')
                    })
            }),
        "invalid plugin ID"
    );
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid SHA-256"
    );
    Ok(())
}

fn validate_https_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).context("invalid HTTPS URL")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "URL must be public HTTPS without credentials"
    );
    Ok(())
}

fn extract_package(archive: &Path, output: &Path) -> Result<()> {
    let file = fs::File::open(archive)?;
    let mut archive = zip::ZipArchive::new(file).context("invalid plugin ZIP")?;
    ensure!(
        archive.len() <= MAX_PACKAGE_FILES,
        "plugin package has too many files"
    );
    let mut names = HashSet::new();
    let mut expanded = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry.enclosed_name().context("unsafe path in plugin ZIP")?;
        ensure!(!relative.as_os_str().is_empty(), "empty path in plugin ZIP");
        ensure!(
            names.insert(relative.clone()),
            "duplicate path in plugin ZIP"
        );
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("plugin ZIP contains a symbolic link");
        }
        expanded = expanded
            .checked_add(entry.size())
            .context("plugin package expansion overflow")?;
        ensure!(
            expanded <= MAX_EXPANDED_BYTES,
            "plugin package expands too large"
        );

        let path = output.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(path)?;
        } else {
            fs::create_dir_all(path.parent().context("plugin ZIP entry has no parent")?)?;
            let mut file = fs::File::create(path)?;
            std::io::copy(&mut entry, &mut file)?;
            file.flush()?;
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let bytes = std::io::copy(&mut file, &mut hasher)?;
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().context("receipt has no parent")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.write_all(b"\n")?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("cache file has no parent")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.write_all(bytes)?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde_json::json;
    use sha2::{Digest as _, Sha256};
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{
        CATALOG_KEY_ID, InstallSource, InstallTarget, install_archive, parse_install_link,
        read_receipt, remove_managed, verify_catalog,
    };

    fn package(path: &std::path::Path, id: &str, extra: Option<(&str, &[u8])>) {
        let file = fs::File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, body) in [
            (
                "qrate-plugin.json",
                serde_json::to_vec(&json!({
                    "schema": 1,
                    "id": id,
                    "name": "Example",
                    "version": "1.0.0",
                    "api_version": 1,
                    "entry": "init.lua",
                    "description": "Example",
                    "homepage": "https://example.org/plugin",
                    "license": "MIT",
                    "permissions": []
                }))
                .unwrap(),
            ),
            ("init.lua", b"return {}".to_vec()),
            ("LICENSE", b"MIT".to_vec()),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(&body).unwrap();
        }
        if let Some((name, body)) = extra {
            zip.start_file(name, options).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn signed_catalog_verifies_and_tampering_fails() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let bytes = br#"{"schema":1,"generated_at":"now","source_commit":"abc","plugins":[]}"#;
        let signature = json!({
            "schema": 1,
            "key_id": CATALOG_KEY_ID,
            "algorithm": "Ed25519",
            "sha256": format!("{:x}", Sha256::digest(bytes)),
            "signature_base64": STANDARD.encode(key.sign(bytes).to_bytes())
        });
        verify_catalog(
            bytes,
            &serde_json::to_vec(&signature).unwrap(),
            &key.verifying_key(),
        )
        .unwrap();
        assert!(
            verify_catalog(
                b"{}",
                &serde_json::to_vec(&signature).unwrap(),
                &key.verifying_key()
            )
            .is_err()
        );
    }

    #[test]
    fn install_links_accept_only_reviewable_sources() {
        assert_eq!(
            parse_install_link("qrate://plugin/install?source=registry&id=org.example.plugin")
                .unwrap(),
            InstallTarget::Registry("org.example.plugin".into())
        );
        assert_eq!(
            parse_install_link("qrate://plugin/install?source=github&repo=owner/plugin").unwrap(),
            InstallTarget::Github("https://github.com/owner/plugin".into())
        );
        assert!(
            parse_install_link("qrate://plugin/install?source=url&release=file:///tmp/plugin.zip")
                .is_err()
        );
        assert!(
            parse_install_link(
                "qrate://plugin/install?source=registry&id=org.example.plugin&extra=yes"
            )
            .is_err()
        );
        for id in ["plugin", "org.-plugin", "org.plugin-", "org..plugin"] {
            assert!(
                parse_install_link(&format!("qrate://plugin/install?source=registry&id={id}"))
                    .is_err()
            );
        }
    }

    #[test]
    fn installs_and_removes_a_managed_package() {
        let root = tempdir().unwrap();
        let archive = root.path().join("plugin.zip");
        package(&archive, "org.example.plugin", None);
        let plugins = root.path().join("plugins");
        let receipts = root.path().join("receipts");
        let receipt = install_archive(
            &archive,
            &plugins,
            &receipts,
            InstallSource::DirectGithub,
            None,
        )
        .unwrap();
        assert!(plugins.join("org.example.plugin/init.lua").is_file());
        assert_eq!(
            read_receipt(&receipts, "org.example.plugin")
                .unwrap()
                .unwrap()
                .version,
            receipt.version
        );
        remove_managed("org.example.plugin", &plugins, &receipts).unwrap();
        assert!(!plugins.join("org.example.plugin").exists());
    }

    #[test]
    fn rejects_path_traversal() {
        let root = tempdir().unwrap();
        let archive = root.path().join("plugin.zip");
        package(&archive, "org.example.plugin", Some(("../outside", b"bad")));
        assert!(
            install_archive(
                &archive,
                &root.path().join("plugins"),
                &root.path().join("receipts"),
                InstallSource::DirectGithub,
                None
            )
            .is_err()
        );
    }
}

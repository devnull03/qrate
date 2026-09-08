//! Verifies, installs, updates, and removes qrate plugin release packages.

use std::collections::HashSet;
use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallSource {
    OfficialCatalog,
    DirectGithub,
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
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let bytes = bounded_response(client.get(url).send()?, MAX_CATALOG_BYTES as u64)
        .context("could not download plugin catalog")?;
    let signature = bounded_response(client.get(format!("{url}.sig")).send()?, 64 * 1024)
        .context("could not download plugin catalog signature")?;
    verify_catalog(&bytes, &signature, key)
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

pub fn install_archive(
    archive: &Path,
    plugins_root: &Path,
    receipts_root: &Path,
    source: InstallSource,
    expected: Option<(&CatalogPlugin, &CatalogRelease)>,
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
            || manifest.homepage.clone(),
            |(plugin, _)| plugin.repository.clone(),
        ),
        artifact_url: expected
            .map_or_else(String::new, |(_, release)| release.artifact_url.clone()),
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
        validate_sha256(&plugin.current.sha256)?;
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
        manifest.api_version > 0,
        "package API version must be positive"
    );
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
    ensure!(
        !id.is_empty()
            && id.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || b"._-".contains(&byte))
            && id.contains('.')
            && !id.starts_with(['.', '-', '_'])
            && !id.ends_with(['.', '-', '_'])
            && !id.contains(".."),
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
        CATALOG_KEY_ID, InstallSource, install_archive, read_receipt, remove_managed,
        verify_catalog,
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

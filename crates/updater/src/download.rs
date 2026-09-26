//! Where downloads come from, and fetching them so they can be trusted. Only the app has this;
//! the post-exit helper never touches the network.

use std::{
    fs,
    io::{Read, Seek as _, SeekFrom, Write as _},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use reqwest::{StatusCode, Url, blocking::Client, header::RANGE};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::{MANIFEST_NAME, RELEASE_DOWNLOADS, ReleaseChannel, SignedEnvelope};

pub const DOWNLOAD_SOURCE_KEY: &str = "download_source";
pub const DOWNLOAD_SOURCE_ENV: &str = "QRATE_DOWNLOAD_SOURCE";

const GITHUB: &str = "https://github.com/";
const LATEST_STABLE_FEED: &str =
    "https://github.com/devnull03/qrate/releases/latest/download/update-manifest.json";
const RELEASES_API: &str = "https://api.github.com/repos/devnull03/qrate/releases?per_page=30";
const MAX_FEED_BYTES: u64 = 1024 * 1024;

/// A mirror or local folder standing in for GitHub and the site. It is untrusted: everything
/// fetched through it is either signed or checked against a signed size and SHA-256, so it can
/// withhold a download but never change one.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Source {
    base: Option<Url>,
}

impl Source {
    /// Empty means the defaults. `https://` anywhere, `http://` only on this computer, or a
    /// `file:///` folder.
    pub fn parse(value: Option<&str>) -> Result<Self> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Self::default());
        };
        let mut url = Url::parse(value).context("the download source is not a URL")?;
        match url.scheme() {
            "https" => {}
            "http" => {
                let host = url.host_str().unwrap_or_default();
                ensure!(
                    host.eq_ignore_ascii_case("localhost")
                        || host
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .parse::<IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback()),
                    "an http:// download source must be on this computer; use https:// otherwise"
                );
            }
            "file" => ensure!(
                url.to_file_path().is_ok(),
                "the download source is not a local folder"
            ),
            scheme => bail!("the download source cannot be a {scheme}:// URL"),
        }
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "the download source must not contain credentials"
        );
        ensure!(
            url.query().is_none() && url.fragment().is_none(),
            "the download source must be a plain folder URL"
        );
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Ok(Self { base: Some(url) })
    }

    /// The environment variable wins over the setting, and a value that does not parse means
    /// the defaults rather than no downloads at all.
    pub fn choose(env: Option<&str>, setting: Option<&str>) -> Self {
        let value = env.filter(|value| !value.trim().is_empty()).or(setting);
        Self::parse(value).unwrap_or_else(|error| {
            log::warn!(
                "ignoring the download source {:?} and using the defaults: {error:#}",
                value.unwrap_or_default()
            );
            Self::default()
        })
    }

    pub fn is_default(&self) -> bool {
        self.base.is_none()
    }

    fn mirrored(&self, path: &str) -> Option<String> {
        let base = self.base.as_ref()?;
        Some(format!("{base}{}", path.trim_start_matches('/')))
    }

    /// `path` on the source, or on `default_origin` when there is none.
    pub fn site(&self, default_origin: &str, path: &str) -> String {
        self.mirrored(path)
            .unwrap_or_else(|| format!("{default_origin}{path}"))
    }

    /// A `https://github.com/...` URL moved under the source's `github/` folder. Anything else,
    /// or no source, comes back unchanged.
    pub fn github(&self, url: &str) -> String {
        url.strip_prefix(GITHUB)
            .and_then(|path| self.mirrored(&format!("github/{path}")))
            .unwrap_or_else(|| url.to_owned())
    }
}

/// Runs `fetch` on the source's copy, then on the original once if the source does not have it.
/// A partial mirror, such as a folder holding only what one person is testing, still works.
pub fn try_mirror<T>(
    mirrored: &str,
    original: &str,
    mut fetch: impl FnMut(&str) -> Result<T>,
) -> Result<T> {
    match fetch(mirrored) {
        Err(error) if mirrored != original && is_not_found(&error) => {
            log::warn!("the download source has no {mirrored}; downloading {original} instead");
            fetch(original)
        }
        result => result,
    }
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(|error| error.status() == Some(StatusCode::NOT_FOUND))
            || cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    })
}

pub(crate) fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent("qrate-updater")
        .timeout(Duration::from_secs(30))
        .build()?)
}

/// The body of `url` from byte `from`, how many bytes it says are left, and whether it really
/// starts at `from`. A server that ignores `Range` sends the whole file, and the caller restarts.
fn open(client: &Client, url: &str, from: u64) -> Result<(Box<dyn Read>, Option<u64>, bool)> {
    if let Some(path) = url
        .starts_with("file:")
        .then(|| Url::parse(url).ok()?.to_file_path().ok())
    {
        let path = path.with_context(|| format!("{url} is not a local file"))?;
        let mut file = fs::File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let length = file.metadata()?.len();
        let from = from.min(length);
        file.seek(SeekFrom::Start(from))?;
        return Ok((Box::new(file), Some(length - from), from > 0));
    }
    let mut request = client.get(url);
    if from > 0 {
        request = request.header(RANGE, format!("bytes={from}-"));
    }
    let response = request
        .send()
        .with_context(|| format!("could not reach {url}"))?
        .error_for_status()?;
    let resumed = response.status() == StatusCode::PARTIAL_CONTENT;
    let length = response.content_length();
    Ok((Box::new(response), length, resumed))
}

fn read_bytes(client: &Client, url: &str, limit: u64) -> Result<Vec<u8>> {
    let (body, _, _) = open(client, url, 0)?;
    let mut bytes = Vec::new();
    body.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "{url} is too large");
    Ok(bytes)
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
}

/// The newest release in `channel` that carries a signed update manifest. A prerelease build
/// follows every release, a stable one only stable releases.
fn newest_tag(releases: &[GithubRelease], channel: ReleaseChannel) -> Option<&str> {
    releases
        .iter()
        .filter(|release| {
            !release.draft
                && release
                    .assets
                    .iter()
                    .any(|asset| asset.name == MANIFEST_NAME)
        })
        .filter_map(|release| {
            let tag = release.tag_name.as_str();
            let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
            channel.accepts(&version).then_some((version, tag))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, tag)| tag)
}

/// Stable builds read GitHub's latest release, which never is a prerelease, without spending the
/// API's 60 anonymous requests an hour. Prerelease builds have no such URL and ask the API.
fn github_feed(
    channel: ReleaseChannel,
    releases: impl FnOnce() -> Result<Vec<GithubRelease>>,
) -> Result<String> {
    Ok(match channel {
        ReleaseChannel::Stable => LATEST_STABLE_FEED.to_owned(),
        ReleaseChannel::Beta => {
            let releases = releases()?;
            let tag = newest_tag(&releases, channel)
                .context("no published qrate release carries a signed update manifest")?;
            format!("{RELEASE_DOWNLOADS}{tag}/{MANIFEST_NAME}")
        }
    })
}

/// The signed manifest of the newest release in `channel`. A download source answers first with
/// its `updates/<channel>.json`, since a folder cannot work out which release is newest.
pub(crate) fn fetch_feed(
    client: &Client,
    source: &Source,
    channel: ReleaseChannel,
) -> Result<SignedEnvelope> {
    let read = |url: &str| -> Result<SignedEnvelope> {
        serde_json::from_slice(&read_bytes(client, url, MAX_FEED_BYTES)?)
            .with_context(|| format!("{url} is not a signed update manifest"))
    };
    let from_github = || {
        read(&github_feed(channel, || {
            Ok(client
                .get(RELEASES_API)
                .send()?
                .error_for_status()?
                .json()?)
        })?)
    };
    let Some(mirrored) = source.mirrored(&format!("updates/{}.json", channel.name())) else {
        return from_github();
    };
    match read(&mirrored) {
        Err(error) if is_not_found(&error) => {
            log::warn!("the download source has no {mirrored}; reading the feed from GitHub");
            from_github()
        }
        result => result,
    }
}

fn partial_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_owned();
    name.push(".partial");
    dest.with_file_name(name)
}

/// Downloads `url` to `dest` through `<dest>.partial`, hashing as it arrives so the file is read
/// once. A `.partial` left by a cancelled or dropped download is resumed, and one that fails its
/// size or digest is deleted so the next attempt starts clean.
pub fn download_verified(
    url: &str,
    size: u64,
    sha256: &str,
    dest: &Path,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<()> {
    let partial = partial_path(dest);
    let discard = |message: &str| {
        let _ = fs::remove_file(&partial);
        anyhow!("{message}")
    };
    let mut received = fs::metadata(&partial).map_or(0, |meta| meta.len());
    if received > size {
        fs::remove_file(&partial)?;
        received = 0;
    }
    let mut hasher = Sha256::new();
    if received > 0 {
        let mut kept = fs::File::open(&partial)?;
        std::io::copy(&mut kept, &mut hasher)?;
    }

    if received < size {
        let (mut body, length, resumed) = open(&client()?, url, received)?;
        if !resumed && received > 0 {
            hasher = Sha256::new();
            received = 0;
        }
        if let Some(length) = length
            && received + length != size
        {
            return Err(discard(
                "server content length differs from signed manifest",
            ));
        }
        let mut output = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(received > 0)
            .truncate(received == 0)
            .open(&partial)?;
        let mut reported_percent = None;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                output.sync_all()?;
                bail!("download cancelled");
            }
            let read = body.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            received += read as u64;
            if received > size {
                drop(output);
                return Err(discard("download exceeded signed size"));
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read])?;
            let percent = received.saturating_mul(100) / size;
            if reported_percent != Some(percent) {
                reported_percent = Some(percent);
                progress(received, size);
            }
        }
        output.sync_all()?;
    }
    if received != size {
        return Err(discard("download size mismatch"));
    }
    if !format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(sha256) {
        return Err(discard("download checksum mismatch"));
    }
    fs::rename(&partial, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{BufRead as _, BufReader, Write as _},
        net::TcpListener,
        path::Path,
        sync::atomic::{AtomicBool, Ordering},
    };

    use anyhow::bail;
    use reqwest::Url;
    use sha2::{Digest as _, Sha256};

    use super::{
        GithubAsset, GithubRelease, LATEST_STABLE_FEED, Source, client, download_verified,
        fetch_feed, github_feed, newest_tag, partial_path, try_mirror,
    };
    use crate::ReleaseChannel;

    fn source(value: &str) -> Source {
        Source::parse(Some(value)).unwrap()
    }

    fn file_url(path: &Path) -> String {
        Url::from_directory_path(path).unwrap().to_string()
    }

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn release(tag: &str, signed: bool) -> GithubRelease {
        GithubRelease {
            tag_name: tag.into(),
            draft: false,
            assets: signed
                .then(|| GithubAsset {
                    name: "update-manifest.json".into(),
                })
                .into_iter()
                .collect(),
        }
    }

    /// An HTTP server for one test: `/missing` is a 404, and `/ranged` and `/whole` serve `body`,
    /// the first honouring `Range` the way GitHub's CDN does and the second ignoring it.
    fn serve(body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                let mut from = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(range) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = range.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                let (status, content) = if request.contains("/missing") {
                    ("404 Not Found", &b""[..])
                } else if request.contains("/ranged") && from > 0 {
                    ("206 Partial Content", &body[from..])
                } else {
                    ("200 OK", body)
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    content.len()
                );
                let _ = stream.write_all(content);
            }
        });
        format!("http://{address}")
    }

    #[test]
    fn parses_only_sources_that_cannot_be_intercepted() {
        assert!(Source::parse(None).unwrap().is_default());
        assert!(Source::parse(Some("  ")).unwrap().is_default());
        assert!(!source("https://mirror.example.org/qrate").is_default());
        for loopback in [
            "http://127.0.0.1:8000",
            "http://localhost:8000/",
            "http://[::1]:8000",
        ] {
            assert!(Source::parse(Some(loopback)).is_ok(), "{loopback}");
        }
        let temp = tempfile::tempdir().unwrap();
        assert!(Source::parse(Some(&file_url(temp.path()))).is_ok());

        for refused in [
            "http://mirror.example.org",
            "http://10.0.0.5:8000",
            "ftp://mirror.example.org",
            "https://user:secret@mirror.example.org",
            "https://mirror.example.org/?token=1",
            "mirror.example.org",
        ] {
            assert!(Source::parse(Some(refused)).is_err(), "{refused}");
        }
    }

    #[test]
    fn the_environment_wins_and_a_bad_value_means_the_defaults() {
        let setting = Some("https://setting.example.org");
        assert_eq!(
            Source::choose(Some("https://env.example.org"), setting),
            source("https://env.example.org")
        );
        assert_eq!(
            Source::choose(Some(""), setting),
            source("https://setting.example.org")
        );
        assert!(Source::choose(None, Some("http://example.org")).is_default());
    }

    #[test]
    fn rewrites_site_paths_and_github_assets_under_the_source() {
        let asset = "https://github.com/devnull03/qrate/releases/download/v1.0.0/a.tar.gz";
        let default = Source::default();
        assert_eq!(
            default.site("https://qrate.dvnl.work", "/plugins/catalog.json"),
            "https://qrate.dvnl.work/plugins/catalog.json"
        );
        assert_eq!(default.github(asset), asset);

        // With or without a trailing slash, the base is a folder the paths go under.
        for base in [
            "https://mirror.example.org/qrate",
            "https://mirror.example.org/qrate/",
        ] {
            let mirror = source(base);
            assert_eq!(
                mirror.site("https://qrate.dvnl.work", "/plugins/catalog.json"),
                "https://mirror.example.org/qrate/plugins/catalog.json"
            );
            assert_eq!(
                mirror.github(asset),
                "https://mirror.example.org/qrate/github/devnull03/qrate/releases/download/v1.0.0/a.tar.gz"
            );
            assert_eq!(
                mirror.github("https://example.com/a.tar.gz"),
                "https://example.com/a.tar.gz"
            );
        }
    }

    #[test]
    fn falls_back_to_the_original_only_when_the_mirror_lacks_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let mirrored = format!("{}missing.json", file_url(temp.path()));
        let mut tried = Vec::new();
        let result = try_mirror(&mirrored, "https://github.com/x", |url| {
            tried.push(url.to_owned());
            if url.starts_with("file:") {
                Ok(fs::read(Url::parse(url).unwrap().to_file_path().unwrap())?)
            } else {
                Ok(b"original".to_vec())
            }
        });
        assert_eq!(result.unwrap(), b"original");
        assert_eq!(tried.len(), 2);

        // A mirror that is there but broken is reported, not papered over.
        let mut calls = 0;
        let result: anyhow::Result<()> = try_mirror("https://m/x", "https://github.com/x", |_| {
            calls += 1;
            bail!("connection refused")
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);

        // No source: the one URL is tried once.
        let mut calls = 0;
        let result: anyhow::Result<()> =
            try_mirror("https://github.com/x", "https://github.com/x", |_| {
                calls += 1;
                Err(std::io::Error::from(std::io::ErrorKind::NotFound).into())
            });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_mirror_404_over_http_falls_back() {
        let server = serve(b"payload");
        let mut tried = Vec::new();
        let body = try_mirror(
            &format!("{server}/missing"),
            &format!("{server}/whole"),
            |url| {
                tried.push(url.to_owned());
                Ok(client()?
                    .get(url)
                    .send()?
                    .error_for_status()?
                    .bytes()?
                    .to_vec())
            },
        )
        .unwrap();
        assert_eq!(body, b"payload");
        assert_eq!(tried.len(), 2);
    }

    #[test]
    fn follows_the_channel_the_site_used_to_choose() {
        let releases = vec![
            release("v0.4.0", true),
            release("v0.5.0-beta.2", true),
            release("v0.6.0-beta.1", false),
            GithubRelease {
                draft: true,
                ..release("v0.7.0", true)
            },
            release("nightly", true),
            release("v0.4.1", true),
        ];
        // A prerelease build follows every signed release; a stable build only stable ones.
        assert_eq!(
            newest_tag(&releases, ReleaseChannel::Beta),
            Some("v0.5.0-beta.2")
        );
        assert_eq!(
            newest_tag(&releases, ReleaseChannel::Stable),
            Some("v0.4.1")
        );
        assert_eq!(newest_tag(&[], ReleaseChannel::Beta), None);

        assert_eq!(
            github_feed(ReleaseChannel::Beta, || Ok(releases)).unwrap(),
            "https://github.com/devnull03/qrate/releases/download/v0.5.0-beta.2/update-manifest.json"
        );
        // Stable builds never spend an API request.
        assert_eq!(
            github_feed(ReleaseChannel::Stable, || panic!("no API call for stable")).unwrap(),
            LATEST_STABLE_FEED
        );
        assert!(github_feed(ReleaseChannel::Beta, || Ok(Vec::new())).is_err());
    }

    #[test]
    fn a_download_source_serves_the_channel_feed() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("updates")).unwrap();
        fs::write(
            temp.path().join("updates/beta.json"),
            r#"{"schema":1,"key_id":"qrate-update-1","payload_base64":"e30=","signature_base64":""}"#,
        )
        .unwrap();
        let envelope = fetch_feed(
            &client().unwrap(),
            &source(&file_url(temp.path())),
            ReleaseChannel::Beta,
        )
        .unwrap();
        assert_eq!(envelope.payload_base64, "e30=");
    }

    #[test]
    fn downloads_and_verifies_from_a_local_folder() {
        let temp = tempfile::tempdir().unwrap();
        let body = b"component bytes".repeat(10_000);
        let size = body.len() as u64;
        fs::write(temp.path().join("asset.tar.gz"), &body).unwrap();
        let url = format!("{}asset.tar.gz", file_url(temp.path()));
        let dest = temp.path().join("asset-copy.tar.gz");
        let never = AtomicBool::new(false);

        let mut last = (0, 0);
        download_verified(
            &url,
            size,
            &digest(&body),
            &dest,
            |r, t| last = (r, t),
            &never,
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        assert_eq!(last, (size, size));
        assert!(!partial_path(&dest).exists());

        // A digest that does not match leaves nothing behind to resume or install.
        let wrong = temp.path().join("wrong.tar.gz");
        let error =
            download_verified(&url, size, &"0".repeat(64), &wrong, |_, _| {}, &never).unwrap_err();
        assert!(format!("{error:#}").contains("checksum"));
        assert!(!wrong.exists() && !partial_path(&wrong).exists());

        // Nor does a file larger than it was signed as.
        let short = temp.path().join("short.tar.gz");
        assert!(
            download_verified(&url, 10, &digest(&body[..10]), &short, |_, _| {}, &never).is_err()
        );
        assert!(!short.exists() && !partial_path(&short).exists());
    }

    #[test]
    fn cancel_keeps_a_partial_that_the_next_attempt_resumes() {
        let temp = tempfile::tempdir().unwrap();
        let body = vec![7_u8; 1024 * 1024];
        let size = body.len() as u64;
        fs::write(temp.path().join("asset"), &body).unwrap();
        let url = format!("{}asset", file_url(temp.path()));
        let dest = temp.path().join("dest");
        let cancel = AtomicBool::new(false);

        let error = download_verified(
            &url,
            size,
            &digest(&body),
            &dest,
            |received, _| {
                if received >= 256 * 1024 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("cancelled"));
        assert!(!dest.exists());
        let kept = fs::metadata(partial_path(&dest)).unwrap().len();
        assert!(kept > 0 && kept < size);

        cancel.store(false, Ordering::Relaxed);
        let mut first = None;
        download_verified(
            &url,
            size,
            &digest(&body),
            &dest,
            |received, _| {
                first.get_or_insert(received);
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        assert!(
            first.unwrap() > kept,
            "the second attempt starts from the kept bytes"
        );
    }

    #[test]
    fn resumes_over_http_with_range_and_restarts_when_the_server_ignores_it() {
        const BODY: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let size = BODY.len() as u64;
        let server = serve(BODY);
        let temp = tempfile::tempdir().unwrap();
        let never = AtomicBool::new(false);

        for path in ["ranged", "whole"] {
            let dest = temp.path().join(path);
            fs::write(partial_path(&dest), &BODY[..10]).unwrap();
            let url = format!("{server}/{path}");
            download_verified(&url, size, &digest(BODY), &dest, |_, _| {}, &never).unwrap();
            assert_eq!(fs::read(&dest).unwrap(), BODY, "{path}");
        }

        // A kept partial that is not a prefix of the real file fails the digest and is dropped,
        // so the attempt after it starts clean.
        let dest = temp.path().join("corrupt");
        fs::write(partial_path(&dest), b"XXXXXXXXXX").unwrap();
        let url = format!("{server}/ranged");
        assert!(download_verified(&url, size, &digest(BODY), &dest, |_, _| {}, &never).is_err());
        assert!(!partial_path(&dest).exists());
        download_verified(&url, size, &digest(BODY), &dest, |_, _| {}, &never).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), BODY);
    }
}

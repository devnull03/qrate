//! Where downloads come from. Shared by both targets, so settings can show and check the choice
//! even where nothing is ever downloaded.

use std::net::IpAddr;

use anyhow::{Context as _, Result, bail, ensure};
use url::Url;

pub const DOWNLOAD_SOURCE_KEY: &str = "download_source";
pub const DOWNLOAD_SOURCE_ENV: &str = "QRATE_DOWNLOAD_SOURCE";

const GITHUB: &str = "https://github.com/";

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
            #[cfg(not(target_family = "wasm"))]
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

    pub(crate) fn mirrored(&self, path: &str) -> Option<String> {
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

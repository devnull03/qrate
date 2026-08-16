//! One-shot GitHub release check at startup: is the latest tagged release newer than the version
//! baked into this build? If so, the [`AvailableUpdate`] global carries the platform-matching
//! download link for `status_items::UpdateNotice` to show. Never downloads or installs anything
//! itself — `.github/workflows/release.yml` already builds the installer this links to.

use gpui::{App, AppContext as _, Global};
use serde::Deserialize;
use settings::AppSettings;

const API_URL: &str = "https://api.github.com/repos/devnull03/qrate/releases/latest";

/// Matches the asset names `release.yml` packages under.
#[cfg(target_os = "windows")]
const ASSET_SUFFIX: &str = "-setup.exe";
#[cfg(target_os = "macos")]
const ASSET_SUFFIX: &str = "-universal.dmg";
#[cfg(target_os = "linux")]
const ASSET_SUFFIX: &str = "-linux.tar.gz";

/// So a dismissed release doesn't come back on the next launch.
const DISMISSED_KEY: &str = "update_dismissed_version";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub download_url: String,
}

impl Global for AvailableUpdate {}

fn fetch() -> Option<Release> {
    reqwest::blocking::Client::builder()
        .user_agent("qrate-update-check")
        .build()
        .ok()?
        .get(API_URL)
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()
}

/// Runs once at startup, off the UI thread. Sets [`AvailableUpdate`] when the latest release is
/// newer and not already dismissed; leaves it unset on any network failure, missing platform
/// asset, or when already current — offline is the normal case for this audience.
pub fn check(cx: &mut App) {
    cx.spawn(async move |cx| {
        let Some(release) = cx.background_spawn(async { fetch() }).await else {
            return;
        };
        let latest = release.tag_name.trim_start_matches('v').to_string();
        if latest == env!("CARGO_PKG_VERSION") {
            return;
        }
        let Some(asset) = release
            .assets
            .iter()
            .find(|a| a.name.ends_with(ASSET_SUFFIX))
        else {
            return;
        };
        let download_url = asset.browser_download_url.clone();
        cx.update(|cx| {
            let dismissed = AppSettings::get(cx)
                .values
                .get(DISMISSED_KEY)
                .map(|v| v.text().to_string());
            if dismissed.as_deref() == Some(latest.as_str()) {
                return;
            }
            cx.set_global(AvailableUpdate {
                version: latest,
                download_url,
            });
        });
    })
    .detach();
}

/// Dismiss the current notice; remembered so it doesn't reappear for this same release.
pub fn dismiss(cx: &mut App) {
    if let Some(update) = cx.try_global::<AvailableUpdate>() {
        AppSettings::set_text(DISMISSED_KEY, update.version.clone().into(), cx);
        cx.remove_global::<AvailableUpdate>();
    }
}

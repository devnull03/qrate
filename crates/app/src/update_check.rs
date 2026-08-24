//! One-shot GitHub release check at startup: is the latest tagged release newer than the version
//! baked into this build? If so, the [`AvailableUpdate`] global carries the platform-matching
//! download link for `title_items::UpdateNotice` to show. Never downloads or installs anything
//! itself — the link goes to the site's download page for the platform asset.

use gpui::{App, AppContext as _, Global, Task};
use serde::Deserialize;
use settings::AppSettings;

const API_URL: &str = "https://api.github.com/repos/devnull03/qrate/releases/latest";

/// Downloads go through the site rather than straight at the GitHub asset, so the reader lands on
/// a page that can say what an unsigned build will do before their browser starts saving one.
const DOWNLOAD_PAGE: &str = "https://qrate.dvnl.work/thanks";

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
}

#[derive(Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub download_page: String,
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

/// What the latest tagged release means for this build.
pub enum UpdateStatus {
    UpToDate,
    Available {
        version: String,
        download_page: String,
    },
}

/// The one HTTP-call site: fetches the latest release off the UI thread and classifies it against
/// `CARGO_PKG_VERSION`. `check` (startup, gated by the dismissed-version setting) and the About
/// window (on-demand, ungated) both go through this rather than each hitting the API themselves.
pub fn check_now(cx: &App) -> Task<Option<UpdateStatus>> {
    cx.background_spawn(async {
        let release = fetch()?;
        let latest = release.tag_name.trim_start_matches('v').to_string();
        if latest == env!("CARGO_PKG_VERSION") {
            return Some(UpdateStatus::UpToDate);
        }
        let asset = release
            .assets
            .iter()
            .find(|a| a.name.ends_with(ASSET_SUFFIX))?;
        Some(UpdateStatus::Available {
            version: latest,
            download_page: format!("{DOWNLOAD_PAGE}?a={}", asset.name),
        })
    })
}

/// Runs once at startup, off the UI thread. Sets [`AvailableUpdate`] when the latest release is
/// newer and not already dismissed; leaves it unset on any network failure, missing platform
/// asset, or when already current — offline is the normal case for this audience.
pub fn check(cx: &mut App) {
    let task = check_now(cx);
    cx.spawn(async move |cx| {
        let Some(UpdateStatus::Available {
            version,
            download_page,
        }) = task.await
        else {
            return;
        };
        cx.update(|cx| {
            let dismissed = AppSettings::get(cx)
                .values
                .get(DISMISSED_KEY)
                .map(|v| v.text().to_string());
            if dismissed.as_deref() == Some(version.as_str()) {
                return;
            }
            cx.set_global(AvailableUpdate {
                version,
                download_page,
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

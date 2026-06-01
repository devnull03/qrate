pub mod title_bar;
pub mod status_bar;


use gpui::*;
pub use gpui_component::TitleBar;

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = this_app)]
#[serde(deny_unknown_fields)]
pub struct OpenBrowser {
    pub url: String,
}

/// Global flag set by the workspace while a check or ingest run is in progress.
/// The title bar reads this to show a status indicator; main registers a close
/// handler that blocks the window from being closed while locked.
#[derive(Clone, Default)]
pub struct WindowLock {
    pub locked: bool,
}

impl Global for WindowLock {}

impl WindowLock {
    pub fn set(locked: bool, cx: &mut App) {
        // set_global (not global_mut) pushes NotifyGlobalObservers so observe_global fires.
        cx.set_global(Self { locked });
    }

    pub fn is_locked(cx: &App) -> bool {
        cx.try_global::<Self>().map(|l| l.locked).unwrap_or(false)
    }
}

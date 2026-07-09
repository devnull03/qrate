use std::collections::HashMap;

use gpui::{AnyWindowHandle, App, Global};

/// Generic focus-existing-or-open-new registry, keyed by a window-kind string
/// (e.g. "settings", "project-creation"), so each kind only ever has one live
/// window: a second request to open it focuses the existing instance instead
/// of spawning a duplicate.
#[derive(Clone, Default)]
pub struct WindowRegistry {
    handles: HashMap<&'static str, AnyWindowHandle>,
}

impl Global for WindowRegistry {}

impl WindowRegistry {
    /// If a live window is registered under `kind`, focus it and return true.
    /// If the registered handle is stale (window already closed), clears it
    /// and returns false so the caller can open a fresh window.
    pub fn focus_or_clear(kind: &'static str, cx: &mut App) -> bool {
        let Some(handle) = cx.global::<Self>().handles.get(kind).copied() else {
            return false;
        };

        let focused = handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok();
        if !focused {
            cx.global_mut::<Self>().handles.remove(kind);
        }
        focused
    }

    pub fn register(kind: &'static str, handle: AnyWindowHandle, cx: &mut App) {
        cx.global_mut::<Self>().handles.insert(kind, handle);
    }
}

//! Layer-1 keyboard shortcuts (GPUI actions + keymap). To add a shortcut:
//!   1. declare the action in the `actions!` group below,
//!   2. add a `KeyBinding::new(keys, Action, context)` line in [`key_bindings`],
//!   3. handle it — globally via `cx.on_action` (see [`register_global_handlers`]),
//!      or on a view via `.on_action(cx.listener(..))` when the handler needs a `Window`
//!      (e.g. the dock toggles, handled on the `App` root in `main.rs`).

use gpui::*;

actions!(
    qrate,
    [
        // Workspace commands.
        NewWindow,
        NewProject,
        // Dock/panel toggles (handled on the App root — they need a `Window`).
        ToggleLeftDock,
        ToggleBottomDock,
        ToggleRightDock,
    ]
);

/// All keybindings, registered once at startup via `cx.bind_keys`.
/// `None` context = global; scope later with `Some("SomeContext")` if a key needs to mean
/// different things in different focus regions.
pub fn key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("ctrl-shift-n", NewWindow, None),
        KeyBinding::new("ctrl-n", NewProject, None),
        KeyBinding::new("ctrl-b", ToggleLeftDock, None),
        KeyBinding::new("ctrl-`", ToggleBottomDock, None),
        KeyBinding::new("ctrl-alt-b", ToggleRightDock, None),
    ]
}

/// Handlers that don't need a `Window`. Window-needing actions (the dock toggles) are handled
/// on the `App` view in `main.rs` instead, since global handlers only get `&mut App`.
pub fn register_global_handlers(cx: &mut App) {
    // ponytail: stubs. NewProject has no project concept yet; NewWindow is blocked on the
    // bar registries being globals (single-window) — de-globalize them per-window first.
    cx.on_action(|_: &NewWindow, _cx| eprintln!("NewWindow: TODO (bar registries are global)"));
    cx.on_action(|_: &NewProject, _cx| eprintln!("NewProject: TODO"));
}

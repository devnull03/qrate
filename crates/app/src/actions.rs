//! Layer-1 keyboard shortcuts (GPUI actions + keymap). To add a shortcut:
//!   1. declare the action in the `actions!` group below,
//!   2. add a `KeyBinding::new(keys, Action, context)` line in [`key_bindings`],
//!   3. handle it — globally via `cx.on_action` (see [`register_global_handlers`]),
//!      or on a view via `.on_action(cx.listener(..))` when the handler needs a `Window`
//!      (e.g. the dock toggles, handled on the `App` root in `main.rs`).

use gpui::*;

use crate::app_menus::OpenSettings;

actions!(
    qrate,
    [
        // Workspace commands.
        NewWindow,
        NewProject,
        Save,
        // Dock/panel toggles (handled on the App root — they need a `Window`).
        ToggleLeftDock,
        ToggleBottomDock,
        ToggleRightDock,
    ]
);

/// All keybindings, registered once at startup via `cx.bind_keys`.
/// `None` context = global; scope later with `Some("SomeContext")` if a key needs to mean
/// different things in different focus regions.
///
/// `secondary-` is gpui's platform modifier — Cmd on macOS, Ctrl on Windows and Linux — so one
/// line covers all three, and the menus print the right glyph for whoever is reading them.
pub fn key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("secondary-shift-n", NewWindow, None),
        KeyBinding::new("secondary-n", NewProject, None),
        // Save the open project's data to its `.qrate` file. Global: saving shouldn't depend on
        // where focus sits (a focused cell editor's `Input` context doesn't bind Ctrl+S).
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-b", ToggleLeftDock, None),
        KeyBinding::new("secondary-`", ToggleBottomDock, None),
        KeyBinding::new("secondary-alt-b", ToggleRightDock, None),
        // Settings. Declared in `app_menus` (it's a menu action first); the handler is already
        // registered globally in `main.rs`, so this only adds the key.
        KeyBinding::new("secondary-,", OpenSettings, None),
        // The grid's own actions are declared in `crate::table` (app→table is one-way). Ctrl+F is
        // scoped to the panel so it works wherever focus sits inside it; everything below acts on a
        // grid selection and is scoped to the grid, which keeps the cell editor's own Ctrl+Z/Ctrl+C
        // and its Enter-to-commit intact while it has focus.
        KeyBinding::new("secondary-f", table::Search, Some("TablePanel")),
        KeyBinding::new("secondary-h", table::Replace, Some("TablePanel")),
        KeyBinding::new("enter", table::EditCell, Some(table::GRID_CONTEXT)),
        KeyBinding::new("secondary-z", table::Undo, Some(table::GRID_CONTEXT)),
        KeyBinding::new("secondary-y", table::Redo, Some(table::GRID_CONTEXT)),
        KeyBinding::new("secondary-shift-z", table::Redo, Some(table::GRID_CONTEXT)),
        // The Details panel edits the same grid, so it undoes the same history. Scoped to the
        // panel, not global: inside its field editor the deeper `Input` context wins, which keeps
        // Ctrl+Z as text-undo mid-edit.
        KeyBinding::new(
            "secondary-z",
            table::Undo,
            Some(workspace::DETAILS_META.name),
        ),
        KeyBinding::new(
            "secondary-y",
            table::Redo,
            Some(workspace::DETAILS_META.name),
        ),
        KeyBinding::new(
            "secondary-shift-z",
            table::Redo,
            Some(workspace::DETAILS_META.name),
        ),
        KeyBinding::new("secondary-x", table::Cut, Some(table::GRID_CONTEXT)),
        KeyBinding::new("secondary-c", table::Copy, Some(table::GRID_CONTEXT)),
        KeyBinding::new("secondary-v", table::Paste, Some(table::GRID_CONTEXT)),
        // Blank the selection, the spreadsheet convention — and both keys, since Sheets and Excel
        // accept either. Scoped to the grid like the clipboard keys above, which is what keeps
        // Backspace deleting *text* while the cell editor or the find bar holds focus.
        KeyBinding::new("backspace", table::Clear, Some(table::GRID_CONTEXT)),
        KeyBinding::new("delete", table::Clear, Some(table::GRID_CONTEXT)),
        // Escape drops the selection — what the Details panel used to spend a button on. It is the
        // *last* thing Escape can mean, so it is scoped to the three places a selection is visible
        // rather than bound globally, and anything that answers Escape with "close me" has to
        // outrank it from a deeper context.
        //
        // Deeper context, not a deeper key handler: gpui resolves every matching binding before it
        // runs one `on_key_down` listener, so a listener loses to a binding no matter where it
        // sits. That is what left the gallery's viewer unable to close — see `CloseViewerLayer`.
        KeyBinding::new("escape", table::Deselect, Some(table::GRID_CONTEXT)),
        KeyBinding::new(
            "escape",
            table::Deselect,
            Some(workspace::DETAILS_META.name),
        ),
        KeyBinding::new("escape", table::Deselect, Some(workspace::VIEWS_CONTEXT)),
        // Beats all three above by depth wherever the overlay is mounted.
        KeyBinding::new(
            "escape",
            workspace::CloseViewerLayer,
            Some(workspace::VIEWER_CONTEXT),
        ),
    ]
    .into_iter()
    // Ctrl+1, Ctrl+2, … pick a view directly, in `ViewMode::ALL` order — so a new view gets its
    // key from the same list that gives it a switcher tab and a menu item, with nothing to add
    // here. Global, and handled on the `App` root: the View menu has to work with focus in any
    // panel, and switching views moves the docks around, which needs a `Window`.
    .chain(
        workspace::ViewMode::ALL
            .into_iter()
            .enumerate()
            .map(|(ix, mode)| {
                KeyBinding::new(
                    &format!("secondary-{}", ix + 1),
                    workspace::ShowView { mode },
                    None,
                )
            }),
    )
    .collect()
}

/// Handlers that don't need a `Window`. Window-needing actions (the dock toggles) are handled
/// on the `App` view in `main.rs` instead, since global handlers only get `&mut App`.
///
/// `NewProject` is registered in `main.rs` alongside `OpenSettings` (it needs to call into the
/// `project-wizard` crate). Only `NewWindow` is still a stub here.
pub fn register_global_handlers(cx: &mut App) {
    // ponytail: NewWindow is blocked on the bar registries being globals (single-window) —
    // de-globalize them per-window first.
    cx.on_action(|_: &NewWindow, _cx| log::warn!("NewWindow: TODO (bar registries are global)"));
    cx.on_action(|_: &Save, cx| table::save_now(cx));
    // One handler for all three bindings above: which view was showing the selection doesn't
    // change what dropping it means.
    cx.on_action(|_: &table::Deselect, cx| table::clear_selection(cx));
    // Edit ▸ Undo/Redo dispatches wherever focus happens to sit, which is not always the grid —
    // globally is the only place that catches it from any panel.
    cx.on_action(|_: &table::Undo, cx| table::history_step(false, cx));
    cx.on_action(|_: &table::Redo, cx| table::history_step(true, cx));
}

#[cfg(test)]
mod tests {
    // Never `use super::*`: the parent has `use gpui::*`, and chaining that glob into a test module
    // lets gpui's `test` macro shadow the `#[test]` its own expansion emits (see CLAUDE.md).
    use gpui::{KeyContext, Keymap, Keystroke};

    use crate::actions::key_bindings;

    /// What Escape resolves to for a focus chain, outermost context first. gpui sorts the matching
    /// bindings by how deep their context sits, so the first one is what actually runs — which
    /// makes this the whole Escape priority ranking, readable without a window.
    fn escape_action(contexts: &[&str]) -> Option<&'static str> {
        let keymap = Keymap::new(key_bindings());
        let stack: Vec<KeyContext> = contexts
            .iter()
            .map(|context| KeyContext::parse(context).expect("test context parses"))
            .collect();
        let escape = Keystroke::parse("escape").expect("escape parses");
        let (bindings, _) = keymap.bindings_for_input(&[escape], &stack);
        bindings.first().map(|binding| binding.action().name())
    }

    /// The bug this ranking was written for: a photo opened from a gallery card mounts inside
    /// `ViewsPanel`, so both contexts are live at once. Escape has to close the overlay; dropping
    /// the row selection out from under it left the picture on screen with an empty Details panel.
    #[test]
    fn the_viewer_outranks_deselect_wherever_it_is_mounted() {
        assert_eq!(
            escape_action(&["ViewsPanel", "Viewer"]),
            Some("qrate::CloseViewerLayer"),
            "a gallery-scoped viewer closes before the selection is dropped"
        );
        assert_eq!(
            escape_action(&["Viewer"]),
            Some("qrate::CloseViewerLayer"),
            "and a workspace-scoped one, which has no ViewsPanel above it"
        );
    }

    /// Deselect is the last thing Escape can mean, so it still has to be the answer wherever
    /// nothing deeper is open — otherwise fixing the ranking above would just break the key.
    #[test]
    fn deselect_still_answers_escape_in_each_place_a_selection_shows() {
        for context in ["DataTable", "DetailsPanel", "ViewsPanel"] {
            assert_eq!(
                escape_action(&[context]),
                Some("qrate::Deselect"),
                "{context} drops the selection when no overlay is open"
            );
        }
    }
}

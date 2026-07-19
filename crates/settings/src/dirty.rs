//! Global "there is unsaved work" tracker.
//!
//! qrate persists continuously through debounced writers, so today nothing in the app has to ask
//! whether state is dirty. This exists for the work that will: hand-rolled undo/redo needs a change
//! cursor, and an explicit autosave needs to know what still owes a write. Marking is therefore
//! wired up now, at every site that mutates persisted state, so those features inherit correct
//! marks instead of having to retrofit them.
//!
//! Two things are tracked: a set of dirty *domains* (what changed) and a monotonic *revision*
//! (how many changes have happened). Undo/redo wants the revision; autosave wants the domains.

use std::collections::BTreeSet;

use gpui::{App, Global};

/// Cell edits — the project's actual data.
pub const PROJECT_DATA: &str = "project_data";
/// Column order and widths.
pub const COLUMN_LAYOUT: &str = "column_layout";
/// Per-column preferences.
pub const COLUMN_SETTINGS: &str = "column_settings";

#[derive(Default)]
pub struct Dirty {
    domains: BTreeSet<&'static str>,
    revision: u64,
}

impl Global for Dirty {}

impl Dirty {
    /// Whether anything at all is unsaved.
    pub fn any(cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|d| !d.domains.is_empty())
    }

    /// Whether one domain is unsaved.
    pub fn has(domain: &str, cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|d| d.domains.contains(domain))
    }

    /// Monotonic change counter, never reset. A cursor for undo/redo: stash it, compare later to
    /// know whether anything moved.
    pub fn revision(cx: &App) -> u64 {
        cx.try_global::<Self>().map_or(0, |d| d.revision)
    }

    /// The currently-dirty domains, sorted.
    pub fn domains(cx: &App) -> Vec<&'static str> {
        cx.try_global::<Self>()
            .map(|d| d.domains.iter().copied().collect())
            .unwrap_or_default()
    }
}

/// Record that `domain` changed. Mutating the global notifies observers, so any future indicator
/// repaints for free. No-op before [`init`] runs.
pub fn mark(domain: &'static str, cx: &mut App) {
    if !cx.has_global::<Dirty>() {
        return;
    }
    let d = cx.global_mut::<Dirty>();
    d.domains.insert(domain);
    d.revision = d.revision.saturating_add(1);
}

/// Record that `domain` reached disk.
pub fn clear(domain: &str, cx: &mut App) {
    if !cx.has_global::<Dirty>() {
        return;
    }
    cx.global_mut::<Dirty>().domains.remove(domain);
}

/// Record that everything reached disk — the quit-time flush.
pub fn clear_all(cx: &mut App) {
    if !cx.has_global::<Dirty>() {
        return;
    }
    cx.global_mut::<Dirty>().domains.clear();
}

/// Install the tracker. Call once at startup, before any `mark`.
pub fn init(cx: &mut App) {
    cx.set_global(Dirty::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_accumulate_by_domain_and_bump_the_revision() {
        let mut d = Dirty::default();
        d.domains.insert(PROJECT_DATA);
        d.revision += 1;
        d.domains.insert(PROJECT_DATA);
        d.revision += 1;
        // Same domain twice is still one dirty domain, but two revisions.
        assert_eq!(d.domains.len(), 1);
        assert_eq!(d.revision, 2);

        d.domains.insert(COLUMN_LAYOUT);
        assert_eq!(d.domains.len(), 2);
        d.domains.remove(COLUMN_LAYOUT);
        assert!(d.domains.contains(PROJECT_DATA));
        assert!(!d.domains.contains(COLUMN_LAYOUT));
    }
}

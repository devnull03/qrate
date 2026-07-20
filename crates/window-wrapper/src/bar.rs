//! Shared left/right item model for the app's bars (title bar + status bar).
//! Each bar stores its components in a [`BarItems`] and exposes it as a GPUI global
//! via [`BarRegistry`]; any crate can then drop a view onto either side of either bar.

use gpui::{AnyView, Global};

/// Left/right component lists for one bar.
#[derive(Default)]
pub struct BarItems {
    pub left: Vec<AnyView>,
    pub right: Vec<AnyView>,
}

impl BarItems {
    pub fn add_left(&mut self, view: impl Into<AnyView>) {
        self.left.push(view.into());
    }
    pub fn add_right(&mut self, view: impl Into<AnyView>) {
        self.right.push(view.into());
    }
}

/// A bar's global registry. The data lives in [`BarItems`]; implementors exist only to
/// give each bar a distinct type (GPUI globals are keyed by type, so title and status
/// bars can't share one registry struct). Push items via `items_mut().add_left(view)`.
pub trait BarRegistry: Global {
    fn items(&self) -> &BarItems;
    fn items_mut(&mut self) -> &mut BarItems;
}

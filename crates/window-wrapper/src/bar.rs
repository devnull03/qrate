//! Shared left/right item model for the app's bars (title bar + status bar).
//! Each bar stores its components in a [`BarItems`] and exposes it as a GPUI global
//! via [`BarRegistry`]; any crate can then drop a view onto either side of either bar.

use gpui::{AnyView, App, Global};

/// One registered component, plus whether it currently has anything to show.
///
/// The occupancy question exists because the status bar draws dividers *between* neighbours: an
/// item that renders to nothing — a plugin bar with no plugins, a panel-button group whose panels
/// all moved to the other side — would otherwise strand a divider with empty space beside it. An
/// element can't be asked whether it came out empty, so the item that knows says so up front.
pub struct BarItem {
    pub view: AnyView,
    occupied: Box<dyn Fn(&App) -> bool>,
}

impl BarItem {
    pub fn occupied(&self, cx: &App) -> bool {
        (self.occupied)(cx)
    }
}

/// Left/centre/right component lists for one bar.
#[derive(Default)]
pub struct BarItems {
    pub left: Vec<BarItem>,
    /// Only the status bar draws this one; the title bar's centre is the project name.
    pub centre: Vec<BarItem>,
    pub right: Vec<BarItem>,
}

impl BarItems {
    pub fn add_left(&mut self, view: impl Into<AnyView>) {
        self.add_left_if(view, |_| true);
    }
    pub fn add_right(&mut self, view: impl Into<AnyView>) {
        self.add_right_if(view, |_| true);
    }

    /// Register a component that is sometimes empty; `occupied` says when it isn't.
    pub fn add_left_if(
        &mut self,
        view: impl Into<AnyView>,
        occupied: impl Fn(&App) -> bool + 'static,
    ) {
        self.left.push(BarItem {
            view: view.into(),
            occupied: Box::new(occupied),
        });
    }
    pub fn add_centre_if(
        &mut self,
        view: impl Into<AnyView>,
        occupied: impl Fn(&App) -> bool + 'static,
    ) {
        self.centre.push(BarItem {
            view: view.into(),
            occupied: Box::new(occupied),
        });
    }
    pub fn add_right_if(
        &mut self,
        view: impl Into<AnyView>,
        occupied: impl Fn(&App) -> bool + 'static,
    ) {
        self.right.push(BarItem {
            view: view.into(),
            occupied: Box::new(occupied),
        });
    }
}

/// A bar's global registry. The data lives in [`BarItems`]; implementors exist only to
/// give each bar a distinct type (GPUI globals are keyed by type, so title and status
/// bars can't share one registry struct). Push items via `items_mut().add_left(view)`.
pub trait BarRegistry: Global {
    fn items(&self) -> &BarItems;
    fn items_mut(&mut self) -> &mut BarItems;
}

//! How a crate that builds on the workspace gets a place in it without the workspace knowing it
//! exists — the same inversion `LauncherHooks` and the bar registries already use.
//!
//! An extension is a set of plain `fn`s, registered once at startup. The workspace calls
//! [`attach`](WorkspaceExtension::attach) when a main window's workspace is built, draws each
//! extension's overlays over the centre and over the window body, and tells it when the centre
//! switches views or changes size. Onboarding is the first; anything else that needs to float
//! over the grid, or react to the view, goes through the same list instead of growing its own
//! wiring into `Workspace` and `ViewsPanel`.

use gpui::{AnyElement, App, Global, Pixels, Size, WeakEntity};
use gpui_component::dock::DockArea;

use crate::ViewMode;

#[derive(Clone, Copy)]
pub struct WorkspaceExtension {
    /// A main window's workspace was built; `dock_area` is how the extension reaches its panels.
    pub attach: fn(WeakEntity<DockArea>, &mut App),
    /// Drawn over the centre's view, filling it — an absolutely positioned layer.
    pub centre_overlay: fn(&App) -> Option<AnyElement>,
    /// Drawn over the whole window body, above the docks — an absolutely positioned layer.
    pub window_overlay: fn(&App) -> Option<AnyElement>,
    /// The centre switched views.
    pub view_changed: fn(ViewMode, &mut App),
    /// The centre's body was measured at a new size.
    pub centre_resized: fn(Size<Pixels>, &mut App),
}

#[derive(Default)]
struct Extensions(Vec<WorkspaceExtension>);
impl Global for Extensions {}

/// Add an extension. Call before the main window opens; a workspace built earlier never attaches
/// to it.
pub fn register(extension: WorkspaceExtension, cx: &mut App) {
    cx.default_global::<Extensions>().0.push(extension);
}

fn all(cx: &App) -> Vec<WorkspaceExtension> {
    cx.try_global::<Extensions>()
        .map(|extensions| extensions.0.clone())
        .unwrap_or_default()
}

/// Bumped by [`repaint`]; the views that draw overlays observe it.
#[derive(Default)]
pub(crate) struct OverlaysChanged(u64);
impl Global for OverlaysChanged {}

/// An extension's overlay changed: repaint the centre and the window body that draw it. Cheaper
/// than a whole-window refresh, and the workspace needn't know the extension's own entities.
pub fn repaint(cx: &mut App) {
    cx.default_global::<OverlaysChanged>().0 += 1;
}

pub(crate) fn attach(dock_area: WeakEntity<DockArea>, cx: &mut App) {
    for extension in all(cx) {
        (extension.attach)(dock_area.clone(), cx);
    }
}

pub(crate) fn centre_overlays(cx: &App) -> Vec<AnyElement> {
    all(cx)
        .iter()
        .filter_map(|extension| (extension.centre_overlay)(cx))
        .collect()
}

pub(crate) fn window_overlays(cx: &App) -> Vec<AnyElement> {
    all(cx)
        .iter()
        .filter_map(|extension| (extension.window_overlay)(cx))
        .collect()
}

pub(crate) fn view_changed(mode: ViewMode, cx: &mut App) {
    for extension in all(cx) {
        (extension.view_changed)(mode, cx);
    }
}

pub(crate) fn centre_resized(size: Size<Pixels>, cx: &mut App) {
    for extension in all(cx) {
        (extension.centre_resized)(size, cx);
    }
}

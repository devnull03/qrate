//! The main workspace view: a `gpui_component::dock::DockArea` hosting the app's panels
//! (center table, left details, right agent, bottom problems) with layout persistence.

mod dock_button;
mod panel_registry;
mod panels;
mod viewer;
mod views;

pub use viewer::{Scope as ViewerScope, open_viewer};

pub use dock_button::DockToggleButton;
pub use panel_registry::{BarSide, PANELS, PanelMeta, PanelRegistry, bar_side};
pub use panels::DETAILS_META;
pub use views::{ShowView, VIEW_MODE_KEY, ViewMode, show_view};

use std::sync::Arc;

use gpui::*;
use gpui_component::dock::{
    DockArea, DockAreaState, DockEvent, DockItem, DockPlacement, PanelView, register_panel,
};
use settings::AppSettings;

use diagnostics::ProblemsPanel;

use crate::panel_registry::PROBLEMS_META;
use crate::panels::{AGENT_META, AgentPanel, DetailsPanel};
use crate::views::ViewsPanel;

/// Settings key under which the serialized [`DockAreaState`] is persisted —
/// both in the open project's `.qrate` `__settings` (preferred) and in the
/// global app settings (fallback when no project is open).
const DOCK_LAYOUT_KEY: &str = "main_dock_layout";
/// Layout schema version. Bumped to 3 when the centre became `ViewsPanel` (the view host) instead
/// of `TablePanel`, so a layout saved under the old name is discarded rather than restoring a
/// centre with no view switcher.
const DOCK_LAYOUT_VERSION: usize = 3;

/// gpui_component keeps a fixed ~29px title strip for a *closed* bottom dock (dock.rs) so it
/// stays clickable — for us it just "sticks out". We drive the bottom dock from our own bar,
/// so we crop that strip off the bottom instead.
/// ponytail: magic 29 mirrors the library's hardcoded strip height; drop this whole hack if
/// gpui_component ever exposes a "hide when closed" option for bottom docks.
const BOTTOM_DOCK_STRIP_PX: f32 = 29.;

/// How many pixels the strip crop above is currently eating off the bottom of the *side* docks,
/// for their panels to pad themselves back out by. `px(0.)` whenever the bottom dock is open.
///
/// Why they need it: the crop stretches the whole dock area past the bottom edge and clips, but
/// the library only parks the closed bottom dock's strip inside the *center* column — the left
/// and right docks are `h_full` of the entire area (`DockArea::render`), so the crop takes their
/// last 29px of real content with it. The center is unaffected (the strip occupied that space
/// anyway), which is why only the side panels compensate.
///
/// ponytail: a global rather than plumbing, because it's a workspace-level hack the panels
/// shouldn't have in their signatures; dies with `BOTTOM_DOCK_STRIP_PX`.
#[derive(Copy, Clone, Default, PartialEq)]
pub struct BottomDockCrop(pub Pixels);

impl Global for BottomDockCrop {}

pub struct Workspace {
    dock_area: Entity<DockArea>,
    /// Persists the layout to settings whenever the dock emits `LayoutChanged`.
    _layout_sub: Subscription,
    /// Re-renders to mount/unmount the image viewer overlay when it opens or closes.
    _viewer_sub: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Register panel constructors so a persisted layout can be reconstructed by name.
        // The centre is the view host, not the grid — the grid is one view it mounts.
        register_panel(cx, "ViewsPanel", |weak, _state, _info, window, cx| {
            Box::new(cx.new(|cx| ViewsPanel::new(weak, window, cx)))
        });
        register_panel(cx, "DetailsPanel", |_weak, _state, _info, window, cx| {
            Box::new(cx.new(|cx| DetailsPanel::new(window, cx)))
        });
        register_panel(cx, "AgentPanel", |_weak, _state, _info, window, cx| {
            Box::new(cx.new(|cx| AgentPanel::new(window, cx)))
        });
        register_panel(cx, "ProblemsPanel", |_weak, _state, _info, window, cx| {
            Box::new(cx.new(|cx| ProblemsPanel::new(window, cx)))
        });

        let dock_area =
            cx.new(|cx| DockArea::new("qrate-main", Some(DOCK_LAYOUT_VERSION), window, cx));

        // Restore if saved, else build default; building first then loading would orphan a throwaway table.
        if !Self::restore_layout(&dock_area, window, cx) {
            let weak = dock_area.downgrade();
            let centre = cx.new(|cx| ViewsPanel::new(weak.clone(), window, cx));
            // Which dock each panel starts in is the panel's own declaration, so they're built
            // as a flat list and grouped by it rather than one `set_*_dock` call apiece.
            let panels: [(&PanelMeta, Arc<dyn PanelView>); 3] = [
                (
                    &DETAILS_META,
                    Arc::new(cx.new(|cx| DetailsPanel::new(window, cx))),
                ),
                (
                    &PROBLEMS_META,
                    Arc::new(cx.new(|cx| ProblemsPanel::new(window, cx))),
                ),
                (
                    &AGENT_META,
                    Arc::new(cx.new(|cx| AgentPanel::new(window, cx))),
                ),
            ];

            dock_area.update(cx, |area, cx| {
                // `tab`, not `panel`: `DockItem::panel` embeds the centre bare, with no title bar
                // at all — and the title bar is where the view switcher lives. Restoring a saved
                // layout always produces tabs anyway, so this only fixes the first launch, which
                // would otherwise come up with no way to leave the grid.
                area.set_center(DockItem::tab(centre, &weak, window, cx), window, cx);
                for placement in [
                    DockPlacement::Left,
                    DockPlacement::Right,
                    DockPlacement::Bottom,
                ] {
                    let items: Vec<Arc<dyn PanelView>> = panels
                        .iter()
                        .filter(|(meta, _)| meta.default_placement == placement)
                        .map(|(_, view)| view.clone())
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    let item = DockItem::tabs(items, &weak, window, cx);
                    match placement {
                        DockPlacement::Left => {
                            area.set_left_dock(item, Some(px(300.)), true, window, cx)
                        }
                        DockPlacement::Right => {
                            area.set_right_dock(item, Some(px(340.)), true, window, cx)
                        }
                        _ => area.set_bottom_dock(item, Some(px(200.)), true, window, cx),
                    }
                }
            });
        }

        // We drive dock open/close from our own title/status-bar buttons, so hide the built-in
        // toggle arrows. Done in both paths — `load` doesn't carry this runtime-only flag.
        dock_area.update(cx, |area, cx| area.set_toggle_button_visible(false, cx));

        // Whichever path ran above built the panels; this is what learns where they landed.
        PanelRegistry::sync(&dock_area, cx);

        // Persist on `LayoutChanged` (inner changes only); `toggle_dock` and the app-quit flush cover the rest.
        let _layout_sub = cx.subscribe(&dock_area, |_this, area, event: &DockEvent, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                Self::persist_layout(&area, cx);
                // Re-render the workspace so the bottom-strip crop tracks the dock's open state.
                cx.notify();
            }
        });

        let _viewer_sub = cx.observe_global::<viewer::ActiveViewer>(|_this, cx| cx.notify());

        Self {
            dock_area,
            _layout_sub,
            _viewer_sub,
        }
    }

    /// Weak handle to the dock area, so bar buttons can toggle panels open/closed.
    pub fn dock_area(&self) -> WeakEntity<DockArea> {
        self.dock_area.downgrade()
    }

    /// Open/close one dock. Driven by both the bar buttons and the keyboard shortcuts.
    pub fn toggle_dock(
        &mut self,
        placement: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock_area.update(cx, |area, cx| {
            area.toggle_dock(placement, window, cx);
        });
        // `Dock::set_open` only notifies, never emits `LayoutChanged`, so persist the toggle here.
        Self::persist_layout(&self.dock_area, cx);
        cx.notify();
    }

    /// Serializes the dock state into the open project's `.qrate` (debounced,
    /// off the UI thread) or the global app settings when no project is open.
    ///
    /// Also the app-quit path (via `dock_area()`): dock edge-resizes never fire
    /// any event, so the final sizes are only guaranteed to be captured at quit.
    /// ponytail: mid-session edge-resizes persist only at quit; hook the dock's
    /// resize if gpui_component ever emits an event for it.
    pub fn persist_layout(dock_area: &Entity<DockArea>, cx: &mut App) {
        let state = dock_area.read(cx).dump(cx);
        let Ok(json) = serde_json::to_string(&state) else {
            return;
        };
        if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
            settings::project::queue_write(&project.file, DOCK_LAYOUT_KEY, &json, cx);
        } else {
            AppSettings::set_text(DOCK_LAYOUT_KEY, json.into(), cx);
        }
    }

    /// Re-reads the layout for whatever project is current and applies it over the
    /// dock's existing state. Needed because the main window is only ever created
    /// once per app session (see `WindowRegistry`); opening a *different* project
    /// while it's already open focuses it instead of re-running `Workspace::new`,
    /// so nothing else re-triggers `restore_layout` for the newly opened project.
    pub fn reload_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if Self::restore_layout(&self.dock_area, window, cx) {
            // `load` rebuilt every panel from its saved name, so the old entries are orphans.
            PanelRegistry::sync(&self.dock_area, cx);
        }
    }

    /// Applies a previously saved dock arrangement, returning `true` if one was loaded. `false`
    /// (no saved layout / corrupt / stale version / load error) tells the caller to fall back to
    /// the default static layout.
    fn restore_layout(
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Prefer the open project's own saved layout; fall back to the global one so a
        // brand-new project still inherits a familiar arrangement.
        let project_layout = cx
            .try_global::<settings::project::CurrentProject>()
            .and_then(|p| settings::project::read_setting(&p.file, DOCK_LAYOUT_KEY).ok())
            .flatten();
        let Some(raw) = project_layout.or_else(|| {
            AppSettings::get(cx)
                .values
                .get(DOCK_LAYOUT_KEY)
                .map(|v| v.text().to_string())
        }) else {
            return false;
        };

        let state: DockAreaState = match serde_json::from_str(&raw) {
            Ok(state) => state,
            Err(err) => {
                log::warn!("ignoring corrupt dock layout: {err}");
                return false;
            }
        };

        // Discard layouts saved under a different schema version.
        if state.version != Some(DOCK_LAYOUT_VERSION) {
            return false;
        }

        dock_area.update(cx, |area, cx| match area.load(state, window, cx) {
            Ok(()) => true,
            Err(err) => {
                log::error!("failed to restore dock layout: {err}");
                false
            }
        })
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Bottom dock closed: overhang 29px and clip to hide the library's residual strip; open: sit flush.
        let bottom_open = self
            .dock_area
            .read(cx)
            .is_dock_open(DockPlacement::Bottom, cx);
        let overshoot = if bottom_open {
            px(0.)
        } else {
            px(-BOTTOM_DOCK_STRIP_PX)
        };

        // Publish what the crop costs the side docks, so their panels can pad it back. Guarded:
        // `set_global` wakes every `observe_global`, and this runs on every workspace render.
        let crop = BottomDockCrop(-overshoot);
        if cx.try_global::<BottomDockCrop>() != Some(&crop) {
            cx.set_global(crop);
        }

        // A sibling overlay, not a dialog: it covers the dock but leaves the title bar's controls
        // reachable. Only the workspace-scoped viewer mounts here — a centre-scoped one is the
        // gallery's, and `ViewsPanel` mounts that inside the centre panel instead.
        let viewer = viewer::viewer_in(viewer::Scope::Workspace, cx);

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom(overshoot)
                    .child(self.dock_area.clone()),
            )
            .children(viewer)
    }
}

//! The main workspace view: a `gpui_component::dock::DockArea` hosting the app's panels
//! (center table, left details, right agent, bottom problems) with layout persistence.

mod dock_button;
mod panels;
// `panels/log_viewer.rs` is intentionally NOT declared here — it is a set-aside,
// reusable line-coloring viewer kept for when `ProblemsPanel` grows real content.

pub use dock_button::DockToggleButton;

use std::sync::Arc;

use gpui::*;
use gpui_component::dock::{
    DockArea, DockAreaState, DockEvent, DockItem, DockPlacement, register_panel,
};
use settings::AppSettings;
use table::TablePanel;

use crate::panels::{AgentPanel, DetailsPanel, ProblemsPanel};

/// Settings key under which the serialized [`DockAreaState`] is persisted —
/// both in the open project's `.qrate` `__settings` (preferred) and in the
/// global app settings (fallback when no project is open).
const DOCK_LAYOUT_KEY: &str = "main_dock_layout";
/// Layout schema version. Bumped to 2 for the center/left/right/bottom panel set so any
/// layout saved under the previous (grid + metadata/vocabulary tabs) shape is discarded.
const DOCK_LAYOUT_VERSION: usize = 2;

/// gpui_component keeps a fixed ~29px title strip for a *closed* bottom dock (dock.rs) so it
/// stays clickable — for us it just "sticks out". We drive the bottom dock from our own bar,
/// so we crop that strip off the bottom instead.
/// ponytail: magic 29 mirrors the library's hardcoded strip height; drop this whole hack if
/// gpui_component ever exposes a "hide when closed" option for bottom docks.
const BOTTOM_DOCK_STRIP_PX: f32 = 29.;

pub struct Workspace {
    dock_area: Entity<DockArea>,
    /// Persists the layout to settings whenever the dock emits `LayoutChanged`.
    _layout_sub: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Register panel constructors so a persisted layout can be reconstructed by name.
        register_panel(cx, "TablePanel", |_weak, _state, _info, window, cx| {
            Box::new(cx.new(|cx| TablePanel::new(window, cx)))
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

        // Restore the saved arrangement if there is one; only build the default layout when
        // there isn't. Building the default first and then `load`ing over it would construct a
        // throwaway *second* table, leaving cross-crate readers (Details panel, status bar)
        // bound to the orphaned first one — which never sees selection events.
        if !Self::restore_layout(&dock_area, window, cx) {
            let weak = dock_area.downgrade();
            // Default static layout: center table, left details, right agent, bottom problems.
            let table = cx.new(|cx| TablePanel::new(window, cx));
            let details = cx.new(|cx| DetailsPanel::new(window, cx));
            let agent = cx.new(|cx| AgentPanel::new(window, cx));
            let problems = cx.new(|cx| ProblemsPanel::new(window, cx));

            dock_area.update(cx, |area, cx| {
                // The center table gets no tab/title-bar chrome at all (no "⋯" menu, no rounded
                // tab corners) — `DockItem::panel` embeds it directly instead of wrapping it in
                // a `TabPanel`, which would otherwise be forced regardless of `zoomable`.
                area.set_center(DockItem::panel(Arc::new(table.clone())), window, cx);
                area.set_left_dock(
                    DockItem::tab(details, &weak, window, cx),
                    Some(px(300.)),
                    true,
                    window,
                    cx,
                );
                area.set_right_dock(
                    DockItem::tab(agent, &weak, window, cx),
                    Some(px(340.)),
                    true,
                    window,
                    cx,
                );
                area.set_bottom_dock(
                    DockItem::tab(problems, &weak, window, cx),
                    Some(px(200.)),
                    true,
                    window,
                    cx,
                );
            });
        }

        // We drive dock open/close from our own title/status-bar buttons, so hide the built-in
        // toggle arrows. Done in both paths — `load` doesn't carry this runtime-only flag.
        dock_area.update(cx, |area, cx| area.set_toggle_button_visible(false, cx));

        // Persist on every layout change. Set up last so the default/restore building above
        // doesn't trigger a redundant write. With a project open the layout is saved into its
        // `.qrate` file (per-project panel state); otherwise into the global app settings.
        // NOTE: gpui_component only emits `LayoutChanged` for *inner* panel changes (tab
        // drags, split resizes) — a dock open/close or edge resize just `cx.notify()`s, so
        // `toggle_dock` below and the app-quit flush persist those explicitly.
        let _layout_sub = cx.subscribe(&dock_area, |_this, area, event: &DockEvent, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                Self::persist_layout(&area, cx);
                // Re-render the workspace so the bottom-strip crop tracks the dock's open state.
                cx.notify();
            }
        });

        Self {
            dock_area,
            _layout_sub,
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
        // `Dock::set_open` never emits `LayoutChanged` (it only notifies), so the
        // subscription in `new` can't see toggles — persist here or open/closed
        // state is lost on reopen.
        Self::persist_layout(&self.dock_area, cx);
        cx.notify();
    }

    /// Serializes the dock state into the open project's `.qrate` (debounced,
    /// off the UI thread) or the global app settings when no project is open.
    fn persist_layout(dock_area: &Entity<DockArea>, cx: &mut App) {
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

    /// Best-effort layout persist for the app-quit path: dock edge-resizes never
    /// fire any event (see `persist_layout`), so the final sizes are only
    /// guaranteed to be captured here.
    /// ponytail: mid-session edge-resizes persist only at quit; hook the dock's
    /// resize if gpui_component ever emits an event for it.
    pub fn persist_layout_on_quit(&self, cx: &mut App) {
        Self::persist_layout(&self.dock_area, cx);
    }

    /// Re-reads the layout for whatever project is current and applies it over the
    /// dock's existing state. Needed because the main window is only ever created
    /// once per app session (see `WindowRegistry`); opening a *different* project
    /// while it's already open focuses it instead of re-running `Workspace::new`,
    /// so nothing else re-triggers `restore_layout` for the newly opened project.
    pub fn reload_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Self::restore_layout(&self.dock_area, window, cx);
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
                eprintln!("ignoring corrupt dock layout: {err}");
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
                eprintln!("failed to restore dock layout: {err}");
                false
            }
        })
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // When the bottom dock is closed, stretch the dock area 29px past the bottom edge and
        // clip it, so the library's residual "closed bottom" strip is cropped out of view.
        // When it's open, sit flush (bottom: 0) so no real content is lost.
        let bottom_open = self
            .dock_area
            .read(cx)
            .is_dock_open(DockPlacement::Bottom, cx);
        let overshoot = if bottom_open {
            px(0.)
        } else {
            px(-BOTTOM_DOCK_STRIP_PX)
        };

        div().size_full().relative().overflow_hidden().child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom(overshoot)
                .child(self.dock_area.clone()),
        )
    }
}

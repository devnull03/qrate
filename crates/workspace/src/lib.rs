//! The main workspace view: a `gpui_component::dock::DockArea` hosting the app's panels
//! (center table, left details, right agent, bottom problems) with layout persistence.

mod dock_button;
mod panels;
// `panels/log_viewer.rs` is intentionally NOT declared here — it is a set-aside,
// reusable line-coloring viewer kept for when `ProblemsPanel` grows real content.

pub use dock_button::DockToggleButton;

use gpui::*;
use gpui_component::dock::{
    DockArea, DockAreaState, DockEvent, DockItem, DockPlacement, register_panel,
};
use settings::AppSettings;

use crate::panels::{AgentPanel, DetailsPanel, ProblemsPanel, TablePanel};

/// Settings key under which the serialized [`DockAreaState`] is persisted.
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
        let weak = dock_area.downgrade();

        // Default static layout: center table, left details, right agent, bottom problems.
        let table = cx.new(|cx| TablePanel::new(window, cx));
        let details = cx.new(|cx| DetailsPanel::new(window, cx));
        let agent = cx.new(|cx| AgentPanel::new(window, cx));
        let problems = cx.new(|cx| ProblemsPanel::new(window, cx));

        dock_area.update(cx, |area, cx| {
            // We drive open/close from our own title/status-bar buttons, so hide the dock's
            // built-in toggle arrows (they otherwise flank the center table panel).
            area.set_toggle_button_visible(false, cx);
            area.set_center(DockItem::tab(table, &weak, window, cx), window, cx);
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

        // Restore a previously saved layout over the default, if one exists and matches.
        Self::restore_layout(&dock_area, window, cx);

        // Persist on every layout change. Set up last so the default/restore building above
        // doesn't trigger a redundant write.
        let _layout_sub = cx.subscribe(&dock_area, |_this, area, event: &DockEvent, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                let state = area.read(cx).dump(cx);
                if let Ok(json) = serde_json::to_string(&state) {
                    AppSettings::set_text(DOCK_LAYOUT_KEY, json.into(), cx);
                }
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
    }

    fn restore_layout(dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(raw) = AppSettings::get(cx)
            .values
            .get(DOCK_LAYOUT_KEY)
            .map(|v| v.text().to_string())
        else {
            return;
        };

        let state: DockAreaState = match serde_json::from_str(&raw) {
            Ok(state) => state,
            Err(err) => {
                eprintln!("ignoring corrupt dock layout: {err}");
                return;
            }
        };

        // Discard layouts saved under a different schema version.
        if state.version != Some(DOCK_LAYOUT_VERSION) {
            return;
        }

        dock_area.update(cx, |area, cx| {
            if let Err(err) = area.load(state, window, cx) {
                eprintln!("failed to restore dock layout: {err}");
            }
        });
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

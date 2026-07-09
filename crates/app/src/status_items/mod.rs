mod cell_location;
mod fake_data;

use cell_location::CellLocation;
use fake_data::FakeDataButton;
use gpui::*;
use gpui_component::{
    IconName,
    dock::{DockArea, DockPlacement},
};
use window_wrapper::{BarRegistry, status_bar::StatusBarRegistry};
use workspace::DockToggleButton;

/// Populate the status bar. Left: left-panel toggle + a problems (errors/warnings) counter
/// that opens the bottom panel. Right: the agent-panel toggle.
pub fn build_status_bar_registry(cx: &mut App, dock: WeakEntity<DockArea>) -> StatusBarRegistry {
    let mut registry = StatusBarRegistry::new();

    let left_panel = cx.new(|_| {
        DockToggleButton::new(
            "status-left-panel",
            dock.clone(),
            DockPlacement::Left,
            IconName::PanelLeft,
        )
    });
    registry.add_left(left_panel);

    // ponytail: diagnostics don't exist yet — count is a placeholder "0". Wire the real
    // error/warning totals when ProblemsPanel grows content.
    let problems = cx.new(|_| {
        DockToggleButton::new(
            "status-problems",
            dock.clone(),
            DockPlacement::Bottom,
            IconName::TriangleAlert,
        )
        .label("0")
    });
    registry.add_left(problems);

    // Debug tool: append 2000 fake rows to the center table. Interactive → left group.
    let fake_data = cx.new(|_| FakeDataButton::new());
    registry.add_left(fake_data);

    // Text readout of the table's selected cell. Added to the right *before* the agent star so it
    // lands leftmost in the right group (text items go right-leftmost, star stays rightmost).
    let cell_location = cx.new(|cx| CellLocation::new(cx));
    registry.add_right(cell_location);

    let agent = cx.new(|_| {
        DockToggleButton::new(
            "status-agent",
            dock.clone(),
            DockPlacement::Right,
            IconName::Star,
        )
    });
    registry.add_right(agent);

    registry
}

mod cell_location;
pub mod markup;
mod plugin_bar;

use cell_location::CellLocation;
use gpui::*;
use gpui_component::{
    IconName,
    dock::{DockArea, DockPlacement},
};
use plugin_api::{Bar, Side};
pub use plugin_bar::PluginBar;
use window_wrapper::{BarRegistry, status_bar::StatusBarRegistry};
use workspace::DockToggleButton;

/// Populate the status bar. Left: left-panel toggle + a problems (errors/warnings) counter
/// that opens the bottom panel. Right: the agent-panel toggle.
pub fn build_status_bar_registry(cx: &mut App, dock: WeakEntity<DockArea>) -> StatusBarRegistry {
    let mut registry = StatusBarRegistry::default();

    let left_panel = cx.new(|_| {
        DockToggleButton::new(
            "status-left-panel",
            dock.clone(),
            DockPlacement::Left,
            IconName::Info,
        )
    });
    registry.items_mut().add_left(left_panel);

    let problems = cx.new(|cx| {
        DockToggleButton::new(
            "status-problems",
            dock.clone(),
            DockPlacement::Bottom,
            IconName::TriangleAlert,
        )
        .problem_count(cx)
    });
    registry.items_mut().add_left(problems);

    let plugins = cx.new(|cx| PluginBar::new(Bar::Status, Side::Left, cx));
    registry.items_mut().add_left(plugins);

    // Plugin text, then the cell readout, then the agent star: text items go right-leftmost and
    // the star stays rightmost.
    let plugins = cx.new(|cx| PluginBar::new(Bar::Status, Side::Right, cx));
    registry.items_mut().add_right(plugins);

    // Text readout of the table's selected cell. Added to the right *before* the agent star so it
    // lands leftmost in the right group (text items go right-leftmost, star stays rightmost).
    let cell_location = cx.new(CellLocation::new);
    registry.items_mut().add_right(cell_location);

    let agent = cx.new(|_| {
        DockToggleButton::new(
            "status-agent",
            dock.clone(),
            DockPlacement::Right,
            IconName::Star,
        )
    });
    registry.items_mut().add_right(agent);

    registry
}

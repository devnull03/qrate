mod cell_location;
pub mod markup;
mod panel_buttons;
mod plugin_bar;
mod update_notice;

use cell_location::CellLocation;
use gpui::*;
use gpui_component::dock::DockArea;
use panel_buttons::PanelButtons;
use plugin_api::{Bar, BarContributions, Side};
pub use plugin_bar::PluginBar;
use update_notice::UpdateNotice;
use window_wrapper::{BarRegistry, status_bar::StatusBarRegistry};
use workspace::{BarSide, DockToggleButton, PANELS};

/// Populate the status bar. The panel buttons come from what each panel declared about itself
/// (`workspace::PANELS`), and which side they land on follows where the panel is docked.
pub fn build_status_bar_registry(cx: &mut App, dock: WeakEntity<DockArea>) -> StatusBarRegistry {
    let mut registry = StatusBarRegistry::default();

    let buttons: Vec<_> = PANELS
        .iter()
        .map(|meta| {
            (
                *meta,
                cx.new(|cx| DockToggleButton::for_panel(dock.clone(), meta, cx)),
            )
        })
        .collect();

    let left_panels = cx.new(|cx| PanelButtons::new(BarSide::Left, buttons.clone(), cx));
    registry
        .items_mut()
        .add_left_if(left_panels, |cx| PanelButtons::occupied(BarSide::Left, cx));

    // Bottom-docked panels sit under the middle of the window, so their buttons do too.
    let centre_panels = cx.new(|cx| PanelButtons::new(BarSide::Centre, buttons.clone(), cx));
    registry.items_mut().add_centre_if(centre_panels, |cx| {
        PanelButtons::occupied(BarSide::Centre, cx)
    });

    let plugins = cx.new(|cx| PluginBar::new(Bar::Status, Side::Left, cx));
    registry.items_mut().add_left_if(plugins, |cx| {
        !BarContributions::at(Bar::Status, Side::Left, cx).is_empty()
    });

    // Plugin text, then the cell readout, then the panel buttons: text items go right-leftmost
    // and the buttons stay rightmost.
    let plugins = cx.new(|cx| PluginBar::new(Bar::Status, Side::Right, cx));
    registry.items_mut().add_right_if(plugins, |cx| {
        !BarContributions::at(Bar::Status, Side::Right, cx).is_empty()
    });

    // Dismissible "an update is available" text, shown only once `update_check::check` finds one.
    let update_notice = cx.new(UpdateNotice::new);
    registry
        .items_mut()
        .add_right_if(update_notice, UpdateNotice::occupied);

    // Text readout of the table's selected cell.
    let cell_location = cx.new(CellLocation::new);
    registry.items_mut().add_right(cell_location);

    let right_panels = cx.new(|cx| PanelButtons::new(BarSide::Right, buttons, cx));
    registry.items_mut().add_right_if(right_panels, |cx| {
        PanelButtons::occupied(BarSide::Right, cx)
    });

    registry
}

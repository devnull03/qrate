mod update_notice;

use gpui::*;
use gpui_component::{
    dock::{DockArea, DockPlacement},
    menu::AppMenuBar,
};
use plugin_api::{Bar, Side};

use crate::actions::{ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::status_items::PluginBar;
use update_notice::UpdateNotice;
use window_wrapper::{BarRegistry, title_bar::TitleBarRegistry};
use workspace::DockToggleButton;

/// Populate the title bar registry. The app menus are the one always-present item, so they
/// are registered on the left by default. Right: generic open/close buttons for each dock.
pub fn build_title_bar_registry(cx: &mut App, dock: WeakEntity<DockArea>) -> TitleBarRegistry {
    let mut registry = TitleBarRegistry::default();

    // The library's menu bar, not a row of independent dropdowns: it holds the "a menu is open"
    // state that makes hovering a sibling switch to it, and arrow keys walk between them.
    registry.items_mut().add_left(AppMenuBar::new(cx));

    let plugins = cx.new(|cx| PluginBar::new(Bar::Title, Side::Left, cx));
    registry.items_mut().add_left(plugins);

    // Before the dock buttons, so plugin text sits inboard of them.
    let plugins = cx.new(|cx| PluginBar::new(Bar::Title, Side::Right, cx));
    registry.items_mut().add_right(plugins);

    // Dismissible "an update is available" text, shown only once `update_check::check` finds one.
    // Text before buttons, on the same reasoning as the plugin bar above.
    let update_notice = cx.new(UpdateNotice::new);
    registry
        .items_mut()
        .add_right_if(update_notice, UpdateNotice::occupied);

    // The label and action each button hovers with: the action is what makes the tooltip print
    // Ctrl or ⌘ to match whoever is reading it, rather than a string that is wrong on one platform.
    for (id, placement, label, action) in [
        (
            "title-panel-left",
            DockPlacement::Left,
            "Toggle Left Dock",
            Box::new(ToggleLeftDock) as Box<dyn Action>,
        ),
        (
            "title-panel-bottom",
            DockPlacement::Bottom,
            "Toggle Bottom Dock",
            Box::new(ToggleBottomDock),
        ),
        (
            "title-panel-right",
            DockPlacement::Right,
            "Toggle Right Dock",
            Box::new(ToggleRightDock),
        ),
    ] {
        let btn =
            cx.new(|_| DockToggleButton::new(id, dock.clone(), placement).hint(label, action));
        registry.items_mut().add_right(btn);
    }

    registry
}

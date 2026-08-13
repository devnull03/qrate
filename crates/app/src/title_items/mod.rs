use gpui::*;
use gpui_component::{
    dock::{DockArea, DockPlacement},
    menu::AppMenuBar,
};
use plugin_api::{Bar, Side};

use crate::status_items::PluginBar;
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

    for (id, placement) in [
        ("title-panel-left", DockPlacement::Left),
        ("title-panel-bottom", DockPlacement::Bottom),
        ("title-panel-right", DockPlacement::Right),
    ] {
        let btn = cx.new(|_| DockToggleButton::new(id, dock.clone(), placement));
        registry.items_mut().add_right(btn);
    }

    registry
}

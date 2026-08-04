use gpui::*;
use gpui_component::{
    IconName,
    dock::{DockArea, DockPlacement},
};
use plugin_api::{Bar, Side};

use crate::status_items::PluginBar;
use window_wrapper::{
    BarRegistry,
    title_bar::{TitleBarRegistry, TitleMenus},
};
use workspace::DockToggleButton;

/// Populate the title bar registry. The app menus are the one always-present item, so they
/// are registered on the left by default. Right: generic open/close buttons for each dock.
pub fn build_title_bar_registry(cx: &mut App, dock: WeakEntity<DockArea>) -> TitleBarRegistry {
    let mut registry = TitleBarRegistry::default();

    let menus = cx.new(|_| TitleMenus);
    registry.items_mut().add_left(menus);

    let plugins = cx.new(|cx| PluginBar::new(Bar::Title, Side::Left, cx));
    registry.items_mut().add_left(plugins);

    // Before the dock buttons, so plugin text sits inboard of them.
    let plugins = cx.new(|cx| PluginBar::new(Bar::Title, Side::Right, cx));
    registry.items_mut().add_right(plugins);

    for (id, placement, icon) in [
        ("title-panel-left", DockPlacement::Left, IconName::PanelLeft),
        (
            "title-panel-bottom",
            DockPlacement::Bottom,
            IconName::PanelBottom,
        ),
        (
            "title-panel-right",
            DockPlacement::Right,
            IconName::PanelRight,
        ),
    ] {
        let btn = cx.new(|_| DockToggleButton::new(id, dock.clone(), placement, icon));
        registry.items_mut().add_right(btn);
    }

    registry
}

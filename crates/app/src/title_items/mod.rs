use gpui::*;
use gpui_component::{
    IconName,
    dock::{DockArea, DockPlacement},
};
use window_wrapper::{
    BarRegistry,
    title_bar::{TitleBarRegistry, TitleMenus},
};
use workspace::DockToggleButton;

/// Populate the title bar registry. The app menus are the one always-present item, so they
/// are registered on the left by default. Right: generic open/close buttons for each dock.
pub fn build_title_bar_registry(cx: &mut App, dock: WeakEntity<DockArea>) -> TitleBarRegistry {
    let mut registry = TitleBarRegistry::new();

    let menus = cx.new(|_| TitleMenus);
    registry.items_mut().add_left(menus);

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

pub mod launcher_bar;
mod update_notice;

use std::rc::Rc;

use gpui::*;
use gpui_component::{
    ActiveTheme,
    dock::{DockArea, DockPlacement},
    h_flex,
    menu::{AppMenuBar, PopupMenu, PopupMenuItem},
};
use plugin_api::{Bar, Side};

use crate::actions::{ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::status_items::PluginBar;
use update_notice::UpdateNotice;
use window_wrapper::{BarRegistry, title_bar::TitleBarRegistry};
use workspace::DockToggleButton;

struct FeedbackButton;

impl Render for FeedbackButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("feedback")
            .px(px(6.))
            .py(px(2.))
            .rounded_md()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().secondary_hover))
            .occlude()
            .child("Feedback")
            .on_click(|_, _, cx| cx.open_url(&crate::logging::feedback_url(cx, None)))
    }
}

/// Populate the title bar registry. The app menus are the one always-present item, so they
/// are registered on the left by default. Right: generic open/close buttons for each dock.
pub fn build_title_bar_registry(cx: &mut App, dock: WeakEntity<DockArea>) -> TitleBarRegistry {
    let mut registry = TitleBarRegistry::default();
    let menu_dock = dock.clone();
    registry.overflow_menu = Some(Rc::new(move |menu, window, cx, focus| {
        let menus = gpui_component::GlobalState::global(cx).app_menus().to_vec();
        let menu = menus.into_iter().fold(menu, |menu, app_menu| {
            let focus = focus.clone();
            let submenu = PopupMenu::build(window, cx, move |menu, window, cx| {
                append_menu_items(menu, app_menu.items, window, cx, focus)
            });
            menu.item(PopupMenuItem::submenu(app_menu.name, submenu).disabled(app_menu.disabled))
        });
        let menu = PluginBar::overflow_menu(Bar::Title, menu, window, cx);
        let dock = menu_dock.clone();
        let dock_focus = focus.clone();
        let menu = menu
            .separator()
            .item(PopupMenuItem::new("Feedback").on_click(|_, _, cx| {
                cx.open_url(&crate::logging::feedback_url(cx, None));
            }))
            .submenu("Panels", window, cx, move |menu, _, cx| {
                let menu = match &dock_focus {
                    Some(focus) => menu.action_context(focus.clone()),
                    None => menu,
                };
                [
                    (
                        "Left dock",
                        DockPlacement::Left,
                        Box::new(ToggleLeftDock) as Box<dyn Action>,
                    ),
                    (
                        "Bottom dock",
                        DockPlacement::Bottom,
                        Box::new(ToggleBottomDock),
                    ),
                    (
                        "Right dock",
                        DockPlacement::Right,
                        Box::new(ToggleRightDock),
                    ),
                ]
                .into_iter()
                .fold(menu, |menu, (label, placement, action)| {
                    let open = dock
                        .upgrade()
                        .is_some_and(|dock| dock.read(cx).is_dock_open(placement));
                    menu.menu_with_check(label, open, action)
                })
            });
        update_notice::overflow_menu(menu, cx)
    }));

    // The library's menu bar, not a row of independent dropdowns: it holds the "a menu is open"
    // state that makes hovering a sibling switch to it, and arrow keys walk between them.
    registry.items_mut().add_left(AppMenuBar::new(cx));

    let plugins = cx.new(|cx| PluginBar::new(Bar::Title, Side::Left, cx));
    registry.items_mut().add_left(plugins);

    // Before the dock buttons, so plugin text sits inboard of them.
    let plugins = cx.new(|cx| PluginBar::new(Bar::Title, Side::Right, cx));
    registry.items_mut().add_right(plugins);

    registry.items_mut().add_right(cx.new(|_| FeedbackButton));

    // Download progress and the explicit restart action from the signed updater.
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

/// Convert the same owned menus AppMenuBar reads, preserving action, check, and disabled state.
fn append_menu_items(
    mut menu: PopupMenu,
    items: Vec<OwnedMenuItem>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    focus: Option<FocusHandle>,
) -> PopupMenu {
    if let Some(focus) = &focus {
        menu = menu.action_context(focus.clone());
    }
    for item in items {
        menu = match item {
            OwnedMenuItem::Action {
                name,
                action,
                checked,
                disabled,
                ..
            } => menu.menu_with_check_and_disabled(name, checked, action.boxed_clone(), disabled),
            OwnedMenuItem::Separator => menu.separator(),
            OwnedMenuItem::Submenu(submenu) => {
                let focus = focus.clone();
                let child = PopupMenu::build(window, cx, move |menu, window, cx| {
                    append_menu_items(menu, submenu.items, window, cx, focus)
                });
                menu.item(PopupMenuItem::submenu(submenu.name, child).disabled(submenu.disabled))
            }
            OwnedMenuItem::SystemMenu(_) => menu,
        };
    }
    menu.scrollable(true)
}

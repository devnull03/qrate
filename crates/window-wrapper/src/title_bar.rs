use gpui::*;
use gpui_component::{
    Sizable, TitleBar,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu},
};

use crate::bar::{BarItems, BarRegistry};

/// Global registry for title bar items. The app menus are registered as a default
/// left item at startup; other crates can `add_left`/`add_right` custom components.
#[derive(Default)]
pub struct TitleBarRegistry(BarItems);

impl Global for TitleBarRegistry {}

impl TitleBarRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BarRegistry for TitleBarRegistry {
    fn items(&self) -> &BarItems {
        &self.0
    }
    fn items_mut(&mut self) -> &mut BarItems {
        &mut self.0
    }
}

/// The app-menu dropdown row, rendered from `cx.get_menus()`. Registered as the default
/// left item of the title bar. A view (not a bare element) so it can live in the registry
/// as an `AnyView` and re-read the menus on each render.
pub struct TitleMenus;

impl TitleMenus {
    fn convert_menu(menu_spec: OwnedMenu) -> impl IntoElement {
        let button_id: SharedString = format!("menu-btn-{}", menu_spec.name).into();
        Button::new(button_id)
            .small()
            .ghost()
            .compact()
            .label(menu_spec.name.clone())
            .dropdown_menu(move |mut menu, window, cx| {
                for item in menu_spec.items.clone() {
                    match item {
                        OwnedMenuItem::Action { name, action, .. } => {
                            menu = menu.menu(name.clone(), action.boxed_clone());
                        }
                        OwnedMenuItem::Submenu(submenu) => {
                            menu = *Self::convert_submenu(submenu, menu, window, cx);
                        }
                        OwnedMenuItem::Separator => {
                            menu = menu.separator();
                        }
                        _ => {}
                    }
                }
                menu
            })
    }

    fn convert_submenu(
        submenu_spec: OwnedMenu,
        parent_menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<'_, PopupMenu>,
    ) -> Box<PopupMenu> {
        let items = submenu_spec.items.clone();
        Box::new(parent_menu.submenu(
            submenu_spec.name.clone(),
            window,
            cx,
            move |mut submenu, window, cx| {
                for item in items.clone() {
                    match item {
                        OwnedMenuItem::Action { name, action, .. } => {
                            submenu = submenu.menu(name.clone(), action.boxed_clone());
                        }
                        OwnedMenuItem::Submenu(sub_submenu) => {
                            submenu = *Self::convert_submenu(sub_submenu, submenu, window, cx);
                        }
                        OwnedMenuItem::Separator => {
                            submenu = submenu.separator();
                        }
                        _ => {}
                    }
                }
                submenu
            },
        ))
    }
}

impl Render for TitleMenus {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menus = cx.get_menus().unwrap_or_default();
        let mut row = gpui_component::h_flex().gap_1().justify_start();
        for item in menus {
            row = row.child(Self::convert_menu(item)).cursor_pointer();
        }
        row
    }
}

#[derive(IntoElement, Default)]
pub struct AppTitleBar;

impl AppTitleBar {
    pub fn new() -> Self {
        Self
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (left, right) = cx
            .try_global::<TitleBarRegistry>()
            .map(|r| (r.items().left.clone(), r.items().right.clone()))
            .unwrap_or_default();

        TitleBar::new()
            .child(
                gpui_component::h_flex()
                    .gap_1()
                    .justify_start()
                    .children(left),
            )
            .child(
                gpui_component::h_flex()
                    .flex_1()
                    .justify_end()
                    .pr_4()
                    .gap_1()
                    .items_center()
                    .children(right),
            )
    }
}

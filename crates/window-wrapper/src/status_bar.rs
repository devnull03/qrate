use gpui::*;
use gpui_component::ActiveTheme as _;

use crate::bar::{BarItems, BarRegistry};

/// A global registry for status bar items.
#[derive(Default)]
pub struct StatusBarRegistry(BarItems);

impl Global for StatusBarRegistry {}

impl BarRegistry for StatusBarRegistry {
    fn items(&self) -> &BarItems {
        &self.0
    }
    fn items_mut(&mut self) -> &mut BarItems {
        &mut self.0
    }
}

pub struct StatusBar;

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (left, right) = cx
            .try_global::<StatusBarRegistry>()
            .map(|r| (r.items().left.clone(), r.items().right.clone()))
            .unwrap_or_default();

        gpui_component::h_flex()
            .id("status-bar")
            .bg(cx.theme().title_bar)
            .w_full()
            .px_3()
            .py_1()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                gpui_component::h_flex()
                    .gap_3()
                    .items_center()
                    .children(left),
            )
            .child(
                gpui_component::h_flex()
                    .gap_3()
                    .items_center()
                    .children(right),
            )
    }
}

use gpui::*;
use gpui_component::{h_flex, separator::Separator, status_bar::StatusBar as StatusBarElement};

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
        // One group per end, each its own children so the divider run is per-group. Registered
        // items are already one view per logical group (the panel buttons, the plugin bar, the
        // cell readout), so a divider between neighbours lands between groups and never between
        // two panel icons.
        let group = |items: &Vec<crate::bar::BarItem>, cx: &App| {
            let mut children: Vec<AnyElement> = Vec::new();
            for item in items.iter().filter(|item| item.occupied(cx)) {
                if !children.is_empty() {
                    children.push(Separator::vertical().h_3().into_any_element());
                }
                children.push(item.view.clone().into_any_element());
            }
            // The library's own regions are `gap_2`; wrapping keeps this bar's roomier spacing.
            h_flex().gap_3().items_center().children(children)
        };

        let (left, right) = cx
            .try_global::<StatusBarRegistry>()
            .map(|r| (group(&r.items().left, cx), group(&r.items().right, cx)))
            .unwrap_or_else(|| (h_flex(), h_flex()));

        StatusBarElement::new()
            .px_3()
            .text_color(crate::bar::bar_foreground(cx))
            .left(left)
            .right(right)
    }
}

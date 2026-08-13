use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme as _, h_flex, separator::Separator, status_bar::StatusBar as StatusBarElement,
};

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

        let items = cx.try_global::<StatusBarRegistry>().map(|r| r.items());
        let occupied = |side: fn(&crate::bar::BarItems) -> &Vec<crate::bar::BarItem>| {
            items.is_some_and(|items| side(items).iter().any(|item| item.occupied(cx)))
        };
        let (left, centre, right) = match items {
            Some(items) => (
                group(&items.left, cx),
                group(&items.centre, cx),
                group(&items.right, cx),
            ),
            None => (h_flex(), h_flex(), h_flex()),
        };

        StatusBarElement::new()
            .px_3()
            .text_color(cx.theme().foreground)
            .left(left)
            // The middle *region*, but not middle-aligned: the library centres its child between
            // the two ends, which floats these items in the empty middle of a wide window. A
            // `w_full` row fills that region instead, so its own contents stay left-aligned just
            // past the divider and the bar reads as one run of items from the left edge.
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    // Spelled out rather than `Separator::vertical`, whose base element is
                    // zero-width — it paints its rule as an absolutely-positioned child, which
                    // lands under the neighbour instead of between the two groups here.
                    .when(
                        occupied(|items| &items.left) && occupied(|items| &items.centre),
                        |row| row.child(div().w(px(1.)).h_3().bg(cx.theme().border)),
                    )
                    .child(centre),
            )
            .right(right)
    }
}

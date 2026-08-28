use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme as _, h_flex, separator::Separator, status_bar::StatusBar as StatusBarElement,
};

use crate::bar::{BarItems, BarRegistry};

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
        let group = |items: &Vec<crate::bar::BarItem>, cx: &App| {
            let mut children: Vec<AnyElement> = Vec::new();
            for item in items.iter().filter(|item| item.occupied(cx)) {
                if !children.is_empty() {
                    children.push(Separator::vertical().h_3().into_any_element());
                }
                children.push(item.view.clone().into_any_element());
            }
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
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    // `Separator::vertical` has a zero-width base element, so its absolutely
                    // positioned rule lands under the neighbouring group.
                    .when(
                        occupied(|items| &items.left) && occupied(|items| &items.centre),
                        |row| row.child(div().w(px(1.)).h_3().bg(cx.theme().border)),
                    )
                    .child(centre),
            )
            .right(right)
    }
}

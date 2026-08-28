use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{ActiveTheme, TitleBar};

use crate::bar::{BarItems, BarRegistry};

#[derive(Default)]
pub struct TitleBarRegistry(BarItems);

impl Global for TitleBarRegistry {}

impl BarRegistry for TitleBarRegistry {
    fn items(&self) -> &BarItems {
        &self.0
    }
    fn items_mut(&mut self) -> &mut BarItems {
        &mut self.0
    }
}

#[derive(IntoElement, Default)]
pub struct AppTitleBar {
    title: SharedString,
    dirty: bool,
}

impl AppTitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            dirty: false,
        }
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let views = |items: &Vec<crate::bar::BarItem>| {
            items
                .iter()
                .map(|item| item.view.clone())
                .collect::<Vec<_>>()
        };
        let (left, right) = cx
            .try_global::<TitleBarRegistry>()
            .map(|r| (views(&r.items().left), views(&r.items().right)))
            .unwrap_or_default();

        TitleBar::new()
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(
                gpui_component::h_flex()
                    .flex_1()
                    .gap_1()
                    .justify_start()
                    .children(left),
            )
            .child(
                gpui_component::h_flex()
                    .justify_center()
                    .items_center()
                    .gap_1p5()
                    .when(self.dirty, |this| {
                        this.child(div().size(px(6.)).rounded_full().bg(cx.theme().foreground))
                    })
                    .when(!self.title.is_empty(), |this| {
                        this.child(self.title.clone())
                    }),
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

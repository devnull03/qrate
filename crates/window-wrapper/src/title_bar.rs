use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{ActiveTheme, TitleBar};

use crate::bar::{BarItems, BarRegistry};

/// Global registry for title bar items. The app menus are registered as a default
/// left item at startup; other crates can `add_left`/`add_right` custom components.
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
    /// Centered title text (the open project's name); empty renders nothing.
    title: SharedString,
    /// Whether there is unsaved work — draws a dot before the title.
    dirty: bool,
}

impl AppTitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            dirty: false,
        }
    }

    /// Show the unsaved-changes dot (fed from `settings::dirty::Dirty::any`).
    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // No dividers up here, so the occupancy each item reports is of no interest — just views.
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

        // left menus | centered project name | right dock toggles. Left and right groups both
        // flex_1 so the natural-width center label sits in the true middle of the bar.
        // Text sizing and colour match the status bar, so the two bars frame the window alike.
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
                    // Unsaved-changes dot, left of the project name (editor convention).
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

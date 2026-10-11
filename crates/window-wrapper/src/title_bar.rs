use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{ActiveTheme, TitleBar};

use crate::bar::{BarItems, BarRegistry};
use crate::responsive_header::{HeaderMenu, ResponsiveHeader};

#[derive(Default)]
pub struct TitleBarRegistry {
    items: BarItems,
    pub overflow_menu: Option<HeaderMenu>,
}

impl Global for TitleBarRegistry {}

impl BarRegistry for TitleBarRegistry {
    fn items(&self) -> &BarItems {
        &self.items
    }
    fn items_mut(&mut self) -> &mut BarItems {
        &mut self.items
    }
}

#[derive(IntoElement, Default)]
pub struct AppTitleBar {
    title: SharedString,
    author: SharedString,
    dirty: bool,
}

impl AppTitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            author: SharedString::default(),
            dirty: false,
        }
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    pub fn author(mut self, author: impl Into<SharedString>) -> Self {
        self.author = author.into();
        self
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let views = |items: &Vec<crate::bar::BarItem>| {
            items
                .iter()
                .filter(|item| item.occupied(cx))
                .map(|item| item.view.clone())
                .collect::<Vec<_>>()
        };
        let (left, right) = cx
            .try_global::<TitleBarRegistry>()
            .map(|r| (views(&r.items().left), views(&r.items().right)))
            .unwrap_or_default();

        let compact_title = self.title.clone();
        let compact_author = self.author.clone();
        let measure_left = left.clone();
        let measure_right = right.clone();
        let dirty = self.dirty;

        TitleBar::new()
            // macOS reserves this space for its traffic lights; other platforms can start
            // the menu close to the window edge.
            .when(!cfg!(target_os = "macos"), |bar| bar.pl(px(8.)))
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(
                ResponsiveHeader::new(
                    "app-responsive-header",
                    gpui_component::TITLE_BAR_HEIGHT,
                    move |window, cx| {
                        gpui_component::h_flex()
                            .size_full()
                            .min_w_0()
                            .gap_2()
                            .child(
                                gpui_component::h_flex()
                                    .flex_none()
                                    .gap_1()
                                    .children(left.clone()),
                            )
                            .child(
                                gpui_component::h_flex()
                                    .flex_1()
                                    .min_w(window.rem_size() * 3.)
                                    .justify_center()
                                    .gap_1p5()
                                    .overflow_hidden()
                                    .when(dirty, |title| {
                                        title.child(
                                            div()
                                                .flex_none()
                                                .size(px(6.))
                                                .rounded_full()
                                                .bg(cx.theme().foreground),
                                        )
                                    })
                                    .child(div().min_w_0().truncate().child(self.title.clone()))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if self.author.is_empty() {
                                                "· No author set".to_string()
                                            } else {
                                                format!("· Editing as {}", self.author)
                                            }),
                                    ),
                            )
                            .child(
                                gpui_component::h_flex()
                                    .flex_none()
                                    .pr_4()
                                    .gap_1()
                                    .children(right.clone()),
                            )
                            .into_any_element()
                    },
                    move |_, cx| {
                        gpui_component::h_flex()
                            .min_w_0()
                            .gap_1p5()
                            .justify_center()
                            .when(dirty, |title| {
                                title.child(
                                    div()
                                        .flex_none()
                                        .size(px(6.))
                                        .rounded_full()
                                        .bg(cx.theme().foreground),
                                )
                            })
                            .child(div().min_w_0().truncate().child(compact_title.clone()))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if compact_author.is_empty() {
                                        "· No author set".to_string()
                                    } else {
                                        format!("· Editing as {compact_author}")
                                    }),
                            )
                            .into_any_element()
                    },
                    move |menu, window, cx, focus| {
                        let builder = cx
                            .try_global::<TitleBarRegistry>()
                            .and_then(|registry| registry.overflow_menu.clone());
                        match builder {
                            Some(builder) => builder(menu, window, cx, focus),
                            None => menu,
                        }
                    },
                )
                .measure_controls(move |window, _| {
                    gpui_component::h_flex()
                        .gap_2()
                        .child(
                            gpui_component::h_flex()
                                .flex_none()
                                .gap_1()
                                .children(measure_left.clone()),
                        )
                        .child(div().flex_none().w(window.rem_size() * 3.))
                        .child(
                            gpui_component::h_flex()
                                .flex_none()
                                .pr_4()
                                .gap_1()
                                .children(measure_right.clone()),
                        )
                        .into_any_element()
                })
                .menu_left(),
            )
    }
}

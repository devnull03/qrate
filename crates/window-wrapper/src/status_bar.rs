use gpui::*;
use gpui_component::ActiveTheme as _;

/// A global registry for status bar items.
pub struct StatusBarRegistry {
    left_items: Vec<AnyView>,
    right_items: Vec<AnyView>,
}

impl Global for StatusBarRegistry {}

impl Default for StatusBarRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBarRegistry {
    pub fn new() -> Self {
        Self {
            left_items: Vec::new(),
            right_items: Vec::new(),
        }
    }

    pub fn add_left(&mut self, view: impl Into<AnyView>) {
        self.left_items.push(view.into());
    }

    pub fn add_right(&mut self, view: impl Into<AnyView>) {
        self.right_items.push(view.into());
    }
}

pub struct StatusBar;

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut left_items = Vec::new();
        let mut right_items = Vec::new();

        if let Some(registry) = cx.try_global::<StatusBarRegistry>() {
            left_items = registry.left_items.clone();
            right_items = registry.right_items.clone();
        }

        let mut left_flex = gpui_component::h_flex().gap_3().items_center();
        for item in left_items {
            left_flex = left_flex.child(item);
        }

        let mut right_flex = gpui_component::h_flex().gap_3().items_center();
        for item in right_items {
            right_flex = right_flex.child(item);
        }

        gpui_component::h_flex()
            .id("status-bar")
            .bg(cx.theme().title_bar)
            .w_full()
            .px_3()
            .py_1()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().title_bar_border)
            .child(left_flex)
            .child(right_flex)
    }
}

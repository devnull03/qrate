//! A reusable bar button that toggles one dock of the workspace's `DockArea` open/closed.
//! Registered into the title/status bar registries so the panels can be opened from the bars.
//!
//! Rendered as a plain `div` rather than `gpui_component::Button` on purpose: the library
//! Button hardcodes `cursor_default` for every non-link variant with no override, so a div is
//! the only way to get a pointer cursor on these toggles. It also lets us set the hover
//! highlight explicitly instead of relying on the ghost variant's subtle default.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    dock::{DockArea, DockPlacement},
    h_flex,
};

pub struct DockToggleButton {
    id: SharedString,
    dock: WeakEntity<DockArea>,
    placement: DockPlacement,
    icon: IconName,
    /// Optional text (e.g. the problems counter's count). Placeholder until diagnostics exist.
    label: Option<SharedString>,
}

impl DockToggleButton {
    pub fn new(
        id: impl Into<SharedString>,
        dock: WeakEntity<DockArea>,
        placement: DockPlacement,
        icon: IconName,
    ) -> Self {
        Self {
            id: id.into(),
            dock,
            placement,
            icon,
            label: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl Render for DockToggleButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock.clone();
        let placement = self.placement;
        let hover_bg = cx.theme().secondary_hover;

        h_flex()
            .id(self.id.clone())
            .gap_1()
            .px(px(4.))
            .py(px(2.))
            .rounded_md()
            .cursor_pointer()
            .hover(move |this| this.bg(hover_bg))
            // The title bar is a window-drag region; claim the mouse-down (as the library
            // Button does) so a click here toggles the dock instead of dragging the window.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(Icon::new(self.icon.clone()).small())
            .when_some(self.label.clone(), |this, label| this.child(label))
            .on_click(move |_, window, cx| {
                dock.update(cx, |area, cx| {
                    area.toggle_dock(placement, window, cx);
                })
                .ok();
            })
    }
}

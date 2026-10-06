//! Width-aware headers. Measure the existing header's minimum layout width, then move its
//! controls into a popup when the parent cannot provide that space. Only the visible tree is
//! prepainted, so hidden controls cannot intercept clicks or become tab stops.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    popover::Popover,
};

type Content = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

pub struct ResponsiveHeader {
    id: ElementId,
    height: Pixels,
    expanded: Content,
    title: Content,
    overflow: Option<Content>,
    menu_left: bool,
}

impl ResponsiveHeader {
    pub fn new(
        id: impl Into<ElementId>,
        height: Pixels,
        expanded: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        title: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            height,
            expanded: Rc::new(expanded),
            title: Rc::new(title),
            overflow: None,
            menu_left: false,
        }
    }

    /// By default the popup contains the original header, including any custom controls.
    pub fn overflow_content(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.overflow = Some(Rc::new(content));
        self
    }

    pub fn menu_left(mut self) -> Self {
        self.menu_left = true;
        self
    }
}

impl IntoElement for ResponsiveHeader {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct HeaderLayout {
    expanded: AnyElement,
    minimum_width: Pixels,
}

impl Element for ResponsiveHeader {
    type RequestLayoutState = HeaderLayout;
    type PrepaintState = AnyElement;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, HeaderLayout) {
        // AvailableSpace::Definite sets the space available to a root, not its width. The
        // stretch container gives the original header that width after intrinsic measurement.
        let mut expanded = div()
            .size_full()
            .flex()
            .flex_col()
            .items_stretch()
            .child((self.expanded)(window, cx))
            .into_any_element();
        let minimum_width = expanded
            .layout_as_root(
                size(
                    AvailableSpace::MinContent,
                    AvailableSpace::Definite(self.height),
                ),
                window,
                cx,
            )
            .width;
        let mut frame = div().w_full().min_w_0().h(self.height).into_any_element();
        let layout = frame.request_layout(window, cx);
        (
            layout,
            HeaderLayout {
                expanded,
                minimum_width,
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut HeaderLayout,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let mut visible = if layout.minimum_width > bounds.size.width {
            let content = self
                .overflow
                .clone()
                .unwrap_or_else(|| self.expanded.clone());
            let width = layout.minimum_width;
            let menu = Popover::new("header-overflow")
                .anchor(if self.menu_left {
                    Anchor::TopLeft
                } else {
                    Anchor::TopRight
                })
                .trigger(
                    Button::new("header-overflow-trigger")
                        .icon(IconName::Menu)
                        .ghost()
                        .xsmall()
                        .when(self.menu_left, |button| {
                            button.with_size(px(28.)).size(px(30.))
                        })
                        .tooltip("Header controls"),
                )
                .content(move |_, window, cx| {
                    // Very long plugin controls remain reachable even on small displays.
                    div()
                        .id("header-overflow-content")
                        .min_w_0()
                        .w(width.min((window.viewport_size().width - px(32.)).max(px(20.))))
                        .overflow_x_scroll()
                        .child(div().w(width).child(content(window, cx)))
                });
            let title = div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child((self.title)(window, cx));
            // Occlude the drag region behind the whole popup host. Occluding its button
            // instead blocks the host's mouse-down listener as well as the titlebar.
            let menu = div().flex_none().occlude().child(menu);
            let row = h_flex()
                .size_full()
                .min_w_0()
                .gap_1()
                .px_2()
                .when(self.menu_left, |row| row.pl_0());
            if self.menu_left {
                row.child(menu).child(title)
            } else {
                row.child(title).child(menu)
            }
            .into_any_element()
        } else {
            // The measured tree is reused; controls are never rendered twice in one header.
            std::mem::replace(&mut layout.expanded, div().into_any_element())
        };
        visible.prepaint_as_root(
            bounds.origin,
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        visible
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut HeaderLayout,
        visible: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        visible.paint(window, cx);
    }
}

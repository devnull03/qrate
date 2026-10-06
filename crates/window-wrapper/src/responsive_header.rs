//! Width-aware headers. Measure the existing header's intrinsic width, then move its
//! controls into a dropdown when the parent cannot provide that space. Only the visible tree is
//! prepainted, so hidden controls cannot intercept clicks or become tab stops.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu},
};

type Content = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
pub type HeaderMenu =
    Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>, Option<FocusHandle>) -> PopupMenu>;

#[derive(Default)]
struct HeaderState {
    action_context: Option<FocusHandle>,
    menu_open: bool,
}

pub struct ResponsiveHeader {
    id: ElementId,
    height: Pixels,
    expanded: Content,
    measurement: Option<Content>,
    title: Content,
    menu: HeaderMenu,
    menu_left: bool,
}

impl ResponsiveHeader {
    pub fn new(
        id: impl Into<ElementId>,
        height: Pixels,
        expanded: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        title: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>, Option<FocusHandle>) -> PopupMenu
        + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            height,
            expanded: Rc::new(expanded),
            measurement: None,
            title: Rc::new(title),
            menu: Rc::new(menu),
            menu_left: false,
        }
    }

    /// Measure controls separately when the title should truncate before controls collapse.
    pub fn measure_controls(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.measurement = Some(Rc::new(content));
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
    state: Entity<HeaderState>,
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
        let state = window.use_keyed_state("header-focus", cx, |_, _| HeaderState::default());
        if !state.read(cx).menu_open {
            let focus = window.focused(cx);
            state.update(cx, |state, _| state.action_context = focus);
        }
        // AvailableSpace::Definite sets the space available to a root, not its width. The
        // stretch container gives the original header that width after intrinsic measurement.
        let mut expanded = div()
            .size_full()
            .flex()
            .flex_col()
            .items_stretch()
            .child((self.expanded)(window, cx))
            .into_any_element();
        let mut measurement = self.measurement.as_ref().map(|content| {
            div()
                .id("header-measurement")
                .child(content(window, cx))
                .into_any_element()
        });
        let minimum_width = measurement
            .as_mut()
            .unwrap_or(&mut expanded)
            .layout_as_root(
                size(
                    AvailableSpace::MaxContent,
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
                state,
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
            let builder = self.menu.clone();
            let focus_state = layout.state.clone();
            let open_state = layout.state.clone();
            let menu = Button::new("header-overflow-trigger")
                .icon(IconName::Menu)
                .ghost()
                .xsmall()
                .when(self.menu_left, |button| {
                    // A compact titlebar button: the custom size yields a 15px icon.
                    button.with_size(px(20.)).size(px(24.))
                })
                .tooltip("Header controls")
                .dropdown_menu_with_anchor(
                    if self.menu_left {
                        Anchor::TopLeft
                    } else {
                        Anchor::TopRight
                    },
                    move |menu, window, cx| {
                        let focus = focus_state.read(cx).action_context.clone();
                        let menu = match &focus {
                            Some(focus) => menu.action_context(focus.clone()),
                            None => menu,
                        };
                        builder(menu, window, cx, focus)
                    },
                )
                .on_open_change(move |open, _, cx| {
                    open_state.update(cx, |state, _| state.menu_open = *open);
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
            // A resize can remove an open dropdown without its close callback being called.
            layout.state.update(cx, |state, _| state.menu_open = false);
            // Reuse the expanded tree after measurement.
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

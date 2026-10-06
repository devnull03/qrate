//! Measured, ordered header layouts. Panel owners decide which controls remain visible at
//! each stage and supply matching dropdown entries for the controls that disappear.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu},
};

use crate::panel_headers::PanelHeaderRegistry;

type Content = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
pub type HeaderMenu =
    Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>, Option<FocusHandle>) -> PopupMenu>;

/// One candidate, ordered from most visible controls to least visible controls.
#[derive(Clone)]
pub struct HeaderStage {
    content: Content,
    measurement: Option<Content>,
    menu: Option<HeaderMenu>,
    truncate: bool,
}

impl HeaderStage {
    pub fn new(content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        Self {
            content: Rc::new(content),
            measurement: None,
            menu: None,
            truncate: false,
        }
    }

    pub fn with_menu(mut self, menu: HeaderMenu) -> Self {
        self.menu = Some(menu);
        self
    }

    pub fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }

    pub fn measure_controls(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.measurement = Some(Rc::new(content));
        self
    }
}

#[derive(Default)]
struct HeaderState {
    action_context: Option<FocusHandle>,
    menu_open: bool,
    stage: Option<usize>,
}

pub struct ResponsiveHeader {
    id: ElementId,
    height: Pixels,
    stages: Vec<HeaderStage>,
    menu_left: bool,
}

impl ResponsiveHeader {
    /// Default policy for simple headers: normal controls, then a title and dropdown.
    pub fn new(
        id: impl Into<ElementId>,
        height: Pixels,
        expanded: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        title: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>, Option<FocusHandle>) -> PopupMenu
        + 'static,
    ) -> Self {
        Self::from_stages(
            id,
            height,
            vec![
                HeaderStage::new(expanded),
                HeaderStage::new(title).truncate().with_menu(Rc::new(menu)),
            ],
        )
    }

    pub fn from_stages(id: impl Into<ElementId>, height: Pixels, stages: Vec<HeaderStage>) -> Self {
        assert!(
            !stages.is_empty(),
            "a responsive header needs at least one stage"
        );
        Self {
            id: id.into(),
            height,
            stages,
            menu_left: false,
        }
    }

    /// Let a registered panel replace the default stages without changing the dock skin.
    pub fn panel_stages(
        mut self,
        name: &str,
        view: AnyView,
        common_menu: HeaderMenu,
        cx: &mut App,
    ) -> Self {
        let defaults = self.stages.clone();
        let stages = PanelHeaderRegistry::stages(name, view, self.stages, common_menu, cx);
        self.stages = if stages.is_empty() { defaults } else { stages };
        self
    }

    pub fn measure_controls(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.stages[0] = self.stages[0].clone().measure_controls(content);
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
    widths: Vec<Pixels>,
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
        let state = window.use_keyed_state("header-state", cx, |_, _| HeaderState::default());
        if !state.read(cx).menu_open {
            let focus = window.focused(cx);
            state.update(cx, |state, _| state.action_context = focus);
        }
        let widths = self
            .stages
            .iter()
            .enumerate()
            .map(|(ix, stage)| {
                let content = stage.measurement.as_ref().unwrap_or(&stage.content);
                let body = content(window, cx);
                let body = if stage.menu.is_some() {
                    // Probes contain a stateless button-sized box. Creating live dropdowns here
                    // would retain an open popup after its stage stopped being visible.
                    let menu_width = if self.menu_left {
                        px(24.)
                    } else {
                        window.rem_size() * 1.25
                    };
                    h_flex()
                        .gap_1()
                        .px_2()
                        .when(self.menu_left, |row| row.pl_0())
                        .child(body)
                        .child(div().flex_none().w(menu_width))
                        .into_any_element()
                } else {
                    body
                };
                let mut probe = div()
                    .id(ElementId::NamedInteger("header-probe".into(), ix as u64))
                    .child(body)
                    .into_any_element();
                probe
                    .layout_as_root(
                        size(
                            AvailableSpace::MaxContent,
                            AvailableSpace::Definite(self.height),
                        ),
                        window,
                        cx,
                    )
                    .width
            })
            .collect();
        let mut frame = div().w_full().min_w_0().h(self.height).into_any_element();
        (
            frame.request_layout(window, cx),
            HeaderLayout { widths, state },
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
        let previous = layout.state.read(cx).stage;
        // Collapse as soon as controls stop fitting. Require another 8px before restoring a
        // wider stage, so small resize/font rounding changes cannot oscillate between stages.
        let selected = layout
            .widths
            .iter()
            .enumerate()
            .position(|(ix, width)| {
                let buffer = if previous.is_some_and(|previous| ix < previous) {
                    px(8.)
                } else {
                    px(0.)
                };
                *width + buffer <= bounds.size.width
            })
            .unwrap_or(self.stages.len() - 1);
        if previous != Some(selected) {
            if layout.state.read(cx).menu_open {
                if let Some(focus) = layout.state.read(cx).action_context.clone() {
                    focus.focus(window, cx);
                }
            }
            layout.state.update(cx, |state, _| {
                state.stage = Some(selected);
                state.menu_open = false;
            });
        }
        let stage = &self.stages[selected];
        let body = (stage.content)(window, cx);
        let body = if let Some(builder) = &stage.menu {
            let builder = builder.clone();
            let focus_state = layout.state.clone();
            let open_state = layout.state.clone();
            let menu = Button::new("header-overflow-trigger")
                .icon(IconName::Menu)
                .ghost()
                .xsmall()
                .when(self.menu_left, |button| {
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
            let body = div()
                .flex_1()
                .min_w_0()
                .when(stage.truncate, |body| body.truncate())
                .child(body);
            // The host occludes the native drag region; its trigger must not occlude the host.
            let menu = div().flex_none().occlude().child(menu);
            let row = h_flex()
                .size_full()
                .min_w_0()
                .gap_1()
                .px_2()
                .when(self.menu_left, |row| row.pl_0());
            if self.menu_left {
                row.child(menu).child(body)
            } else {
                row.child(body).child(menu)
            }
            .into_any_element()
        } else {
            body
        };
        // Definite available space alone does not force a root's width. Stretch the selected
        // header so its existing left/right justification still spans the panel.
        let mut visible = div()
            .id(ElementId::NamedInteger(
                "header-stage".into(),
                selected as u64,
            ))
            .size_full()
            .flex()
            .flex_col()
            .items_stretch()
            .child(body)
            .into_any_element();
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

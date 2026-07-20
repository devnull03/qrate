//! `clamped_float`: like `anchored().position_mode(Local).snap_to_window()`, but confined to a
//! caller-supplied rect (the table area) instead of the whole window. gpui's `anchored` only ever
//! clamps to the viewport, so a `deferred` floating editor spills over a side panel; this keeps the
//! floating cell editor inside the table. Wrap it in `deferred` to escape the cell's clip, exactly
//! as `anchored` is used.

use std::panic::Location;

use gpui::{
    App, Bounds, Display, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Position, Style, Window,
};

pub(crate) struct ClampedFloat {
    rect: Bounds<Pixels>,
    child: gpui::AnyElement,
}

/// Float `child` at its natural (local) position, shifted the minimum needed to stay inside `rect`.
pub(crate) fn clamped_float(rect: Bounds<Pixels>, child: impl IntoElement) -> ClampedFloat {
    ClampedFloat {
        rect,
        child: child.into_any_element(),
    }
}

impl IntoElement for ClampedFloat {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ClampedFloat {
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child_id = self.child.request_layout(window, cx);
        let style = Style {
            position: Position::Absolute,
            display: Display::Flex,
            ..Style::default()
        };
        let layout_id = window.request_layout(style, [child_id], cx);
        (layout_id, child_id)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child_id: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // `bounds.origin` is where the box lands naturally — the edited cell's top-left. Shift it the
        // least amount that keeps the box (its real, just-laid-out size) inside the table rect.
        let size = window.layout_bounds(*child_id).size;
        let mut origin = bounds.origin;
        origin.x = origin
            .x
            .min(self.rect.right() - size.width)
            .max(self.rect.left());
        origin.y = origin
            .y
            .min(self.rect.bottom() - size.height)
            .max(self.rect.top());

        let offset = origin - bounds.origin;
        window.with_element_offset(offset, |window| self.child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

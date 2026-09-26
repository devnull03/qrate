//! qrate's dock appearance: the library's `DockSkin`, with one change — a single panel's title
//! row is a strip of its own, tinted like the title bar and ruled off from the panel under it.
//!
//! The library draws a lone panel's title straight on the panel's own background with nothing
//! between the two, so the name, the view switcher and the panel's first row of content run
//! together. A tab strip (two or more panels) already gets the tint and the rule; this gives the
//! one-panel case the same, so every dock and the centre read as "header, then content".
//!
//! Done here, around the skin, rather than in each panel: the title row is the skin's, and five
//! panels each drawing a border at their top would be five places to keep in step — and would
//! double up under a tab strip, which already has its rule.

use std::rc::Rc;
use std::sync::Arc;

use gpui::*;
use gpui_base::ResizeHandleContext;
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{
    DockArea, DockAreaRenderer, DockContext, DockSkin, DropIndicator, NodeId, PanelState,
    PanelStyle, TabGroupContext, TabGroupRenderer, TilesRenderer,
};

pub(crate) struct QrateSkin {
    inner: Rc<DockSkin>,
}

impl QrateSkin {
    /// [`DockSkin::dock_area`], wearing this skin. The returned `DockSkin` is the same one this
    /// delegates to, so its settings still apply.
    pub(crate) fn dock_area(
        id: impl Into<SharedString>,
        version: Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> (Entity<DockArea>, Rc<DockSkin>) {
        let mut skin = None;
        let area = cx.new(|cx| {
            let inner = DockSkin::new(cx);
            skin = Some(inner.clone());
            DockArea::new(id, version, window, cx).with_renderer(Rc::new(Self { inner }))
        });
        (
            area,
            skin.expect("DockSkin::new ran inside the constructor"),
        )
    }
}

impl DockAreaRenderer for QrateSkin {
    fn frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.inner.frame(window, cx)
    }

    fn split_frame(
        &self,
        node: NodeId,
        axis: Axis,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.inner.split_frame(node, axis, window, cx)
    }

    fn center_frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.inner.center_frame(window, cx)
    }

    fn render_split_handle(
        &self,
        handle: &ResizeHandleContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.inner.render_split_handle(handle, window, cx)
    }

    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.inner.render_dock(dock, content, window, cx)
    }

    fn build_placeholder(
        &self,
        state: &PanelState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<dyn gpui_component::dock::BasePanelView>> {
        self.inner.build_placeholder(state, window, cx)
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(QrateTabGroup {
            inner: self.inner.tab_group_renderer(),
            skin: self.inner.clone(),
        })
    }

    fn tiles_renderer(&self) -> Rc<dyn TilesRenderer> {
        self.inner.tiles_renderer()
    }
}

struct QrateTabGroup {
    inner: Rc<dyn TabGroupRenderer>,
    skin: Rc<DockSkin>,
}

impl QrateTabGroup {
    /// Whether the library is about to draw the plain one-panel title, not a tab strip — the
    /// same test its own `render_tab_bar` makes.
    fn draws_plain_title(&self, group: &TabGroupContext, cx: &App) -> bool {
        self.skin.panel_style() == PanelStyle::Auto
            && group
                .panels()
                .iter()
                .filter(|panel| panel.visible(cx))
                .count()
                == 1
    }
}

impl TabGroupRenderer for QrateTabGroup {
    fn frame(&self, group: &TabGroupContext, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.inner.frame(group, window, cx)
    }

    fn content_frame(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.inner.content_frame(group, window, cx)
    }

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let plain = self.draws_plain_title(group, cx);
        let bar = self.inner.render_tab_bar(group, window, cx);
        if !plain {
            return bar;
        }
        div()
            .flex_none()
            .w_full()
            .bg(cx.theme().tab_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(bar)
            .into_any_element()
    }

    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.inner.render_active_panel(panel, group, window, cx)
    }

    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.inner.render_drop_indicator(indicator, window, cx)
    }

    fn render_empty(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.inner.render_empty(group, window, cx)
    }
}

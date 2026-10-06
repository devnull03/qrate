//! qrate's dock appearance: a ruled title strip and shared overflow behavior for every header.
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

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_base::ResizeHandleContext;
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{
    DockArea, DockAreaRenderer, DockContext, DockSkin, DropIndicator, NodeId, PanelHandle,
    PanelState, PanelStyle, TabGroupContext, TabGroupRenderer, TilesRenderer,
};
use gpui_component::menu::PopupMenuItem;

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
        if !group.panels().iter().any(|panel| panel.visible(cx)) {
            return self.inner.render_tab_bar(group, window, cx);
        }
        let plain = self.draws_plain_title(group, cx);
        let inner = self.inner.clone();
        let expanded_group = group.clone();
        let title_group = group.clone();
        let menu_group = group.clone();
        let bar = window_wrapper::responsive_header::ResponsiveHeader::new(
            "dock-responsive-header",
            px(30.),
            move |window, cx| inner.render_tab_bar(&expanded_group, window, cx),
            move |window, cx| {
                let title = title_group
                    .active_panel()
                    .map(|panel| match PanelHandle::of(panel) {
                        Some(handle) => {
                            let label = crate::panel_registry::PANELS
                                .iter()
                                .find(|meta| meta.name == panel.panel_name(cx))
                                .map(|meta| SharedString::from(meta.label))
                                .or_else(|| handle.tab_name(cx));
                            match label {
                                Some(label) => label.into_any_element(),
                                None => handle.title(window, cx),
                            }
                        }
                        None => SharedString::from(panel.panel_name(cx)).into_any_element(),
                    });
                div()
                    .min_w_0()
                    .truncate()
                    .children(title)
                    .into_any_element()
            },
            move |menu, window, cx, _focus| {
                let Some(panel) = menu_group.active_panel() else {
                    return menu;
                };
                let handle = PanelHandle::of(panel);
                let menu = match handle {
                    Some(handle) => handle.dropdown_menu(menu, window, cx),
                    None => menu,
                };
                let menu = menu.when(
                    menu_group
                        .panels()
                        .iter()
                        .filter(|panel| panel.visible(cx))
                        .count()
                        > 1,
                    |menu| {
                        let tabs_group = menu_group.clone();
                        menu.submenu("Panels", window, cx, move |menu, _, cx| {
                            tabs_group
                                .panels()
                                .iter()
                                .enumerate()
                                .filter(|(_, panel)| panel.visible(cx))
                                .fold(menu, |menu, (ix, panel)| {
                                    let label = crate::panel_registry::PANELS
                                        .iter()
                                        .find(|meta| meta.name == panel.panel_name(cx))
                                        .map(|meta| SharedString::from(meta.label))
                                        .or_else(|| {
                                            PanelHandle::of(panel)
                                                .and_then(|handle| handle.tab_name(cx))
                                        })
                                        .unwrap_or_else(|| panel.panel_name(cx).into());
                                    let group = tabs_group.clone();
                                    menu.item(
                                        PopupMenuItem::new(label)
                                            .checked(ix == group.active_ix())
                                            .on_click(move |_, window, cx| {
                                                group.select_tab(ix, window, cx)
                                            }),
                                    )
                                })
                        })
                    },
                );
                let zoomed = menu_group.is_zoomed();
                let zoom_allowed = zoomed
                    || (panel.zoomable(cx)
                        && handle.and_then(|handle| handle.zoom_control(cx)).is_some());
                let group = menu_group.clone();
                let menu = menu.separator().item(
                    PopupMenuItem::new(if zoomed { "Zoom out" } else { "Zoom in" })
                        .disabled(!zoom_allowed)
                        .on_click(move |_, window, cx| group.toggle_zoom(window, cx)),
                );
                if menu_group.is_closable() {
                    let group = menu_group.clone();
                    let id = panel.panel_id(cx);
                    menu.separator().item(
                        PopupMenuItem::new("Close panel")
                            .on_click(move |_, window, cx| group.close(id, window, cx)),
                    )
                } else {
                    menu
                }
            },
        );
        div()
            .flex_none()
            .w_full()
            .bg(cx.theme().tab_bar)
            .when(plain, |bar| {
                bar.border_b_1().border_color(cx.theme().border)
            })
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

//! A reusable bar button that toggles one dock of the workspace's `DockArea` open/closed.
//! Registered into the title/status bar registries so the panels can be opened from the bars.
//!
//! Rendered as a plain `div` rather than `gpui_component::Button` on purpose: the library
//! Button hardcodes `cursor_default` for every non-link variant with no override, so a div is
//! the only way to get a pointer cursor on these toggles. It also lets us set the hover
//! highlight explicitly instead of relying on the ghost variant's subtle default.

use diagnostics::{Diagnostics, Severity, severity_color};
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
    /// Whether to show the live error/warning count beside the icon.
    count: bool,
    _sub: Option<Subscription>,
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
            count: false,
            _sub: None,
        }
    }

    /// Show the diagnostics count, kept current as problems come and go.
    pub fn problem_count(mut self, cx: &mut Context<Self>) -> Self {
        self.count = true;
        self._sub = Some(cx.observe_global::<Diagnostics>(|_this, cx| cx.notify()));
        self
    }
}

/// One severity's icon and tally, in that severity's colour — greyed while it reads zero, so a
/// clean project doesn't show two loud badges.
fn severity_badge(severity: Severity, icon: IconName, count: usize, cx: &App) -> impl IntoElement {
    let color = if count == 0 {
        cx.theme().muted_foreground
    } else {
        severity_color(severity, cx)
    };
    h_flex()
        .gap_1()
        .text_color(color)
        .child(Icon::new(icon).small())
        .child(count.to_string())
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
            // The title bar wraps its children in a `WindowControlArea::Drag` hitbox, which
            // Windows hit-tests as HTCAPTION and never delivers a click from. Occluding
            // stops that hit test at this button.
            .occlude()
            .map(|this| {
                if !self.count {
                    return this.child(Icon::new(self.icon.clone()).small());
                }
                // Errors and warnings side by side rather than one total: the icon and colour say
                // which is which, so `self.icon` has nothing left to add here.
                let (errors, warnings) = Diagnostics::counts(cx);
                this.children([
                    severity_badge(Severity::Error, IconName::CircleX, errors, cx),
                    severity_badge(Severity::Warning, IconName::TriangleAlert, warnings, cx),
                ])
            })
            .on_click(move |_, window, cx| {
                dock.update(cx, |area, cx| {
                    area.toggle_dock(placement, window, cx);
                })
                .ok();
            })
    }
}

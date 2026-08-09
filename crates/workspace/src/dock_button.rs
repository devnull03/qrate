//! A bar button that opens and closes part of the workspace: either one fixed dock (the title
//! bar's generic left/bottom/right buttons) or one panel wherever it currently lives (the status
//! bar), which also right-clicks to move that panel to another dock.
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
    menu::{ContextMenuExt as _, PopupMenuItem},
};

use crate::panel_registry::{PanelMeta, PanelRegistry};

/// What the button acts on.
enum Toggles {
    /// One dock, whatever is in it.
    Dock(DockPlacement),
    /// One panel, wherever it is now.
    Panel(&'static PanelMeta),
}

pub struct DockToggleButton {
    id: SharedString,
    dock: WeakEntity<DockArea>,
    toggles: Toggles,
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
            toggles: Toggles::Dock(placement),
            icon,
            count: false,
            _sub: None,
        }
    }

    /// The status bar's per-panel button, built from what the panel declared about itself.
    pub fn for_panel(
        dock: WeakEntity<DockArea>,
        meta: &'static PanelMeta,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            id: meta.name.into(),
            dock,
            toggles: Toggles::Panel(meta),
            icon: meta.icon.clone(),
            count: meta.badge,
            // Kept current as problems come and go.
            _sub: meta
                .badge
                .then(|| cx.observe_global::<Diagnostics>(|_this, cx| cx.notify())),
        }
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
        let hover_bg = cx.theme().secondary_hover;
        let panel = match self.toggles {
            Toggles::Dock(_) => None,
            Toggles::Panel(meta) => Some(meta),
        };
        let placement = match self.toggles {
            Toggles::Dock(placement) => Some(placement),
            Toggles::Panel(meta) => PanelRegistry::placement(meta.name, cx),
        };

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
            .when_some(panel, |this, meta| {
                this.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(meta.label).build(window, cx)
                })
            })
            .on_click({
                let dock = dock.clone();
                move |_, window, cx| {
                    let Some(area) = dock.upgrade() else {
                        return;
                    };
                    match panel {
                        Some(meta) => PanelRegistry::toggle(meta.name, &area, window, cx),
                        None => {
                            if let Some(placement) = placement {
                                area.update(cx, |area, cx| area.toggle_dock(placement, window, cx));
                                crate::Workspace::persist_layout(&area, cx);
                            }
                        }
                    }
                }
            })
            // `context_menu` wraps rather than extends, so the arms only agree as `AnyElement`.
            .map(|this| match panel {
                None => this.into_any_element(),
                Some(meta) => this
                    .context_menu(move |menu, _, _| {
                        [
                            ("Dock Left", DockPlacement::Left),
                            ("Dock Right", DockPlacement::Right),
                            ("Dock Bottom", DockPlacement::Bottom),
                        ]
                        .into_iter()
                        .fold(menu, |menu, (label, target)| {
                            let dock = dock.clone();
                            menu.item(
                                PopupMenuItem::new(label)
                                    .checked(placement == Some(target))
                                    .disabled(placement == Some(target))
                                    .on_click(move |_, window, cx| {
                                        if let Some(area) = dock.upgrade() {
                                            PanelRegistry::move_panel(
                                                meta.name, target, &area, window, cx,
                                            );
                                        }
                                    }),
                            )
                        })
                    })
                    .into_any_element(),
            })
    }
}

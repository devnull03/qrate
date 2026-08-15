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
    dock::{DockArea, DockEvent, DockPlacement},
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
    /// Asset path drawn in place of `icon` while the dock is open — Lucide has no filled panel.
    open_icon: Option<SharedString>,
    /// Whether to show the live error/warning count beside the icon.
    count: bool,
    /// What to say on hover. The action comes along so the tooltip can print its shortcut in the
    /// running platform's own notation — the binding is declared in `app`, which this crate
    /// cannot name.
    hint: Option<(SharedString, Box<dyn Action>)>,
    _subs: Vec<Subscription>,
}

impl DockToggleButton {
    pub fn new(
        id: impl Into<SharedString>,
        dock: WeakEntity<DockArea>,
        placement: DockPlacement,
    ) -> Self {
        let (icon, open_icon) = match placement {
            DockPlacement::Left => (IconName::PanelLeft, "icons/panel-left-filled.svg"),
            DockPlacement::Right => (IconName::PanelRight, "icons/panel-right-filled.svg"),
            _ => (IconName::PanelBottom, "icons/panel-bottom-filled.svg"),
        };
        Self {
            id: id.into(),
            dock,
            toggles: Toggles::Dock(placement),
            icon,
            open_icon: Some(open_icon.into()),
            count: false,
            hint: None,
            _subs: Vec::new(),
        }
    }

    pub fn hint(mut self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.hint = Some((label.into(), action));
        self
    }

    /// The status bar's per-panel button, built from what the panel declared about itself.
    pub fn for_panel(
        dock: WeakEntity<DockArea>,
        meta: &'static PanelMeta,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subs = Vec::new();
        // Kept current as problems come and go.
        if meta.badge {
            subs.push(cx.observe_global::<Diagnostics>(|_this, cx| cx.notify()));
        }
        // Bringing a sibling tab to the front is a `LayoutChanged` and nothing else — no global
        // moves, so without this the highlight below would still be on the panel just covered.
        if let Some(area) = dock.upgrade() {
            subs.push(cx.subscribe(&area, |_this, _, _: &DockEvent, cx| cx.notify()));
        }
        Self {
            id: meta.name.into(),
            dock,
            toggles: Toggles::Panel(meta),
            icon: meta.icon.clone(),
            open_icon: None,
            count: meta.badge,
            hint: None,
            _subs: subs,
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
        // Two different questions, because the two buttons point at different things. A title-bar
        // button owns a whole dock, so "is it open" is all there is to say. A status-bar button
        // points at one panel, which can sit in an open dock and still be hidden behind a sibling
        // tab — lighting up for that aims the reader at something not on screen.
        let lit = self.dock.upgrade().is_some_and(|area| match self.toggles {
            Toggles::Dock(placement) => area.read(cx).is_dock_open(placement, cx),
            Toggles::Panel(meta) => PanelRegistry::visible(meta.name, &area, cx),
        });

        h_flex()
            .id(self.id.clone())
            .gap_1()
            .px(px(4.))
            .py(px(2.))
            .rounded_md()
            .cursor_pointer()
            .hover(move |this| this.bg(hover_bg))
            // Colour only, no filled background: over a status bar that is itself a tinted strip,
            // a second filled box per open panel reads as a row of buttons stuck down. The title
            // bar says the same thing by swapping its glyph, so tinting it too says it twice.
            .when(lit && panel.is_some(), |this| {
                this.text_color(cx.theme().primary)
            })
            // The title bar wraps its children in a `WindowControlArea::Drag` hitbox, which
            // Windows hit-tests as HTCAPTION and never delivers a click from. Occluding
            // stops that hit test at this button.
            .occlude()
            .map(|this| {
                if !self.count {
                    let icon = match (&self.open_icon, lit) {
                        (Some(path), true) => Icon::empty().path(path.clone()),
                        _ => Icon::new(self.icon.clone()),
                    };
                    return this.child(icon.small());
                }
                // Errors and warnings side by side rather than one total: the icon and colour say
                // which is which, so `self.icon` has nothing left to add here.
                let (errors, warnings) = Diagnostics::counts(cx);
                this.children([
                    severity_badge(Severity::Error, IconName::CircleX, errors, cx),
                    severity_badge(Severity::Warning, IconName::TriangleAlert, warnings, cx),
                ])
            })
            // A dock button carries its shortcut; a panel button has none to carry, so it is left
            // with the panel's own name.
            .when_some(
                self.hint
                    .as_ref()
                    .map(|(label, action)| (label.clone(), Some(action.boxed_clone())))
                    .or_else(|| panel.map(|meta| (meta.label.into(), None))),
                |this, (label, action)| {
                    this.tooltip(move |window, cx| {
                        let tip = gpui_component::tooltip::Tooltip::new(label.clone());
                        match &action {
                            Some(action) => tip.action(action.as_ref(), None),
                            None => tip,
                        }
                        .build(window, cx)
                    })
                },
            )
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

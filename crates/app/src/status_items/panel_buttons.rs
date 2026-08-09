//! The status bar's panel toggles, as one container view per side rather than one view per
//! button — the same reason `plugin_bar` is a container: the bar registry is push-only with no
//! stable item id, so an item that needs to change sides can't be re-registered. Instead both
//! containers hold every button and each renders the ones whose panel is currently docked on
//! its side, so dragging Details from the left dock to the right takes its icon along.

use gpui::*;
use gpui_component::h_flex;
use workspace::{BarSide, DockToggleButton, PANELS, PanelMeta, PanelRegistry, bar_side};

pub struct PanelButtons {
    side: BarSide,
    buttons: Vec<(&'static PanelMeta, Entity<DockToggleButton>)>,
    _sub: Subscription,
}

impl PanelButtons {
    /// `buttons` is built once and shared by both sides; the filter below is exclusive, so a
    /// button is only ever rendered by one of them.
    pub fn new(
        side: BarSide,
        buttons: Vec<(&'static PanelMeta, Entity<DockToggleButton>)>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            side,
            buttons,
            _sub: cx.observe_global::<PanelRegistry>(|_, cx| cx.notify()),
        }
    }

    /// Whether this side has any buttons right now — the status bar needs it to decide whether
    /// to put a divider beside this group.
    pub fn occupied(side: BarSide, cx: &App) -> bool {
        PANELS
            .iter()
            .any(|meta| PanelRegistry::placement(meta.name, cx).map(bar_side) == Some(side))
    }
}

impl Render for PanelButtons {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().gap_3().items_center().children(
            self.buttons
                .iter()
                .filter(|(meta, _)| {
                    PanelRegistry::placement(meta.name, cx).map(bar_side) == Some(self.side)
                })
                .map(|(_, button)| button.clone()),
        )
    }
}

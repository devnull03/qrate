use gpui::*;
use gpui_component::IconName;
use gpui_component::dock::{DockPlacement, Panel, PanelControl, PanelEvent};

use crate::panel_registry::PanelMeta;

/// Where Agent starts out and what it puts in the status bar.
pub static AGENT_META: PanelMeta = PanelMeta {
    name: "AgentPanel",
    icon: IconName::Star,
    label: "Agent",
    default_placement: DockPlacement::Right,
    badge: false,
};

/// Right dock. Will host the AI agent (eventually backed by the `ai` crate).
/// Placeholder for now.
pub struct AgentPanel {
    focus_handle: FocusHandle,
}

impl AgentPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Focusable for AgentPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for AgentPanel {}

impl Panel for AgentPanel {
    fn panel_name(&self) -> &'static str {
        "AgentPanel"
    }

    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Agent")
    }

    // The library always renders the ⋯ menu button; these just empty it of Close + Zoom.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("Agent (placeholder)")
    }
}

use gpui::*;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

/// Center panel. Will hold the virtualized list/table of items. Placeholder for now.
pub struct TablePanel {
    focus_handle: FocusHandle,
}

impl TablePanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Focusable for TablePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for TablePanel {}

impl Panel for TablePanel {
    fn panel_name(&self) -> &'static str {
        "TablePanel"
    }

    // Main workspace body — no fixed name, so leave the title empty for now.
    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::default()
    }

    /// Center panel: not closable so the main view always keeps its body.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    // Drop the zoom item from the ⋯ menu (the button itself is library-rendered).
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for TablePanel {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("Table (placeholder)")
    }
}

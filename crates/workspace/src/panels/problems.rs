use gpui::*;
use gpui_component::{
    Sizable,
    button::{Button, ButtonVariants},
    dock::{Panel, PanelControl, PanelEvent},
};

/// Bottom dock. Will list errors and warnings (diagnostics). Placeholder for now.
/// The set-aside `log_viewer.rs` in this folder has reusable line-coloring if needed.
pub struct ProblemsPanel {
    focus_handle: FocusHandle,
}

impl ProblemsPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Focusable for ProblemsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for ProblemsPanel {}

impl Panel for ProblemsPanel {
    fn panel_name(&self) -> &'static str {
        "ProblemsPanel"
    }

    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Problems")
    }

    /// Unlike `toolbar_buttons`, `title_suffix` is not gated on `TabPanel::collapsed`, so this
    /// element also renders — and stays clickable — inside the 29px strip a closed bottom dock
    /// leaves behind. It takes an arbitrary `IntoElement`, not just a `Button`.
    fn title_suffix(
        &mut self,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        Some(
            Button::new("problems-poc")
                .label("Ping")
                .xsmall()
                .ghost()
                .on_click(cx.listener(|_, _, _, _| {
                    eprintln!("[qrate] toolbar button clicked");
                })),
        )
    }

    // The library always renders the ⋯ menu button; these just empty it of Close + Zoom.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("Errors & warnings (placeholder)")
    }
}

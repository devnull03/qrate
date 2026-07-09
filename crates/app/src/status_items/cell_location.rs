//! Right-side status-bar widget: shows the table's selected `Row N, Col M, K words` (word count
//! of the selected cell's text). Text-only readout — sits leftmost in the right group.

use gpui::*;
use gpui_component::{ActiveTheme, table::TableState};
use table::{QrateTableDelegate, TableChanged, TableStateHandle};

pub struct CellLocation {
    state: Option<WeakEntity<TableState<QrateTableDelegate>>>,
    // Re-render this widget whenever the table's selection or a cell's text changes. Kept alive
    // by being stored.
    _sub: Option<Subscription>,
}

impl CellLocation {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.try_global::<TableStateHandle>().map(|h| h.0.clone());
        // ponytail: binds to the table present at startup. If the center panel is ever rebuilt
        // (dock layout restore edge case) this widget must be re-created to track the new state.
        let _sub = state
            .as_ref()
            .and_then(|w| w.upgrade())
            .map(|entity| cx.subscribe(&entity, |_this, _st, _ev: &TableChanged, cx| cx.notify()));

        Self { state, _sub }
    }

    fn status_text(&self, cx: &App) -> SharedString {
        let Some(state) = self.state.as_ref().and_then(|w| w.upgrade()) else {
            return "No table".into();
        };
        let state = state.read(cx);
        match state.delegate().selected() {
            Some((r, c)) => {
                let words = state
                    .delegate()
                    .cell(r, c)
                    .map(|t| t.split_whitespace().count())
                    .unwrap_or(0);
                format!("Row {}, Col {}, {} words", r + 1, c + 1, words).into()
            }
            None => "No selection".into(),
        }
    }
}

impl Render for CellLocation {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_color(cx.theme().muted_foreground)
            .child(self.status_text(cx))
    }
}

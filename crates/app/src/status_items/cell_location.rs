//! Right-side status-bar widget: shows the table's selection — `Row N, Col M, K words` for a
//! cell (word count of its text), `Row N` for a whole-row selection, `Col M` for a whole-column
//! selection. Text-only readout — sits leftmost in the right group.

use gpui::*;
use gpui_component::{ActiveTheme, table::TableState};
use table::{QrateTableDelegate, Selection, TableChanged, TableStateHandle};

pub struct CellLocation {
    state: Option<WeakEntity<TableState<QrateTableDelegate>>>,
    /// Re-binds `state` whenever `TablePanel` publishes a new table (project reload, dock
    /// layout restore rebuilding panels).
    _handle_sub: Subscription,
    /// Re-renders on any table change (selection, edits).
    _table_sub: Option<Subscription>,
}

impl CellLocation {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let _handle_sub = cx.observe_global::<TableStateHandle>(|this: &mut Self, cx| {
            this.bind(cx);
            cx.notify();
        });
        let mut this = Self {
            state: None,
            _handle_sub,
            _table_sub: None,
        };
        this.bind(cx);
        this
    }

    fn bind(&mut self, cx: &mut Context<Self>) {
        self.state = cx.try_global::<TableStateHandle>().map(|h| h.0.clone());
        self._table_sub =
            self.state.as_ref().and_then(|w| w.upgrade()).map(|entity| {
                cx.subscribe(&entity, |_this, _st, _ev: &TableChanged, cx| cx.notify())
            });
    }

    fn status_text(&self, cx: &App) -> SharedString {
        let Some(state) = self.state.as_ref().and_then(|w| w.upgrade()) else {
            return "No table".into();
        };
        let state = state.read(cx);
        match state.delegate().selection() {
            Some(Selection::Cell { row, col }) => {
                let words = state
                    .delegate()
                    .cell(row, col)
                    .map(|t| t.split_whitespace().count())
                    .unwrap_or(0);
                format!("Row {}, Col {}, {} words", row + 1, col + 1, words).into()
            }
            Some(Selection::Row(row)) => format!("Row {}", row + 1).into(),
            Some(Selection::Column(col)) => format!("Col {}", col + 1).into(),
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

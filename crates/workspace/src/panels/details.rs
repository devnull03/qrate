use gpui::*;
use gpui_component::{
    ActiveTheme,
    description_list::DescriptionList,
    dock::{Panel, PanelControl, PanelEvent},
    scroll::ScrollableElement,
    skeleton::Skeleton,
    table::TableState,
};
use table::{QrateTableDelegate, Selection, TableChanged, TableStateHandle};

/// Left dock: an image preview (skeleton placeholder for now) plus the selected row's fields
/// as a label/value list, per the main-workspace design.
pub struct DetailsPanel {
    focus_handle: FocusHandle,
    /// Live table state, read for the selected row.
    state: Option<WeakEntity<TableState<QrateTableDelegate>>>,
    /// Re-binds `state` whenever `TablePanel` publishes a new table (project reload, dock
    /// layout restore rebuilding panels) — without this, a panel constructed before the table
    /// would point at a dead entity forever.
    _handle_sub: Subscription,
    /// Re-renders on any table change (selection, edits, column moves).
    _table_sub: Option<Subscription>,
}

impl DetailsPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _handle_sub = cx.observe_global::<TableStateHandle>(|this: &mut Self, cx| {
            this.bind(cx);
            cx.notify();
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
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
}

impl Focusable for DetailsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for DetailsPanel {}

impl Panel for DetailsPanel {
    fn panel_name(&self) -> &'static str {
        "DetailsPanel"
    }

    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Details")
    }

    // The library always renders the ⋯ menu button; these just empty it of Close + Zoom.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for DetailsPanel {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = self.state.as_ref().and_then(|w| w.upgrade()).and_then(|s| {
            let state = s.read(cx);
            let row = match state.delegate().selection()? {
                Selection::Cell { row, .. } | Selection::Row(row) => row,
                // A whole-column selection has no single row to detail.
                Selection::Column(_) => return None,
            };
            Some(state.delegate().row_fields(row))
        });

        let content = match fields {
            Some(fields) if !fields.is_empty() => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    // ponytail: skeleton placeholder — swap for the row's photo once rows
                    // carry an image path.
                    div()
                        .w_full()
                        .h(px(180.))
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .overflow_hidden()
                        .child(Skeleton::new().size_full()),
                )
                .child(
                    fields.into_iter().fold(
                        DescriptionList::horizontal()
                            .columns(1)
                            .bordered(false)
                            .label_width(px(110.)),
                        |list, (k, v)| list.item(k, v, 1),
                    ),
                )
                .into_any_element(),
            _ => div()
                .text_color(cx.theme().muted_foreground)
                .child("No selection")
                .into_any_element(),
        };

        // Scroll vertically when a long field list overflows the dock height, rather than
        // clipping it (`overflow_hidden`).
        div()
            .size_full()
            .overflow_y_scrollbar()
            .p_3()
            .child(content)
    }
}

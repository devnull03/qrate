use gpui::*;
use gpui_component::{
    dock::{Panel, PanelControl, PanelEvent},
    input::{InputEvent, InputState},
    table::{Table, TableState},
};

use crate::{TableStateHandle, delegate::QrateTableDelegate, delegate::TableChanged, editing};

/// Center panel: the virtualized 2k×20 text table, with a pinned row-number column, single-cell
/// selection, and double-click-to-edit cells.
pub struct TablePanel {
    focus_handle: FocusHandle,
    state: Entity<TableState<QrateTableDelegate>>,
    /// Commits an in-progress cell edit when the inline editor loses focus or the user presses
    /// Enter. Held alive for as long as the panel exists.
    _edit_sub: Subscription,
}

impl TablePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| InputState::new(window, cx));
        let state = cx.new(|cx| {
            TableState::new(QrateTableDelegate::new(editor.clone()), window, cx)
        });
        // Publish the handle so status-bar items in the `app` crate can drive/read the table.
        cx.set_global(TableStateHandle(state.downgrade()));

        let table_state = state.clone();
        let _edit_sub = cx.subscribe(&editor, move |_this, _editor, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                return;
            }
            table_state.update(cx, |state, cx| {
                editing::commit(state.delegate_mut(), cx);
                cx.emit(TableChanged);
                cx.notify();
            });
        });

        Self {
            focus_handle: cx.focus_handle(),
            state,
            _edit_sub,
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

    // `workspace` embeds this panel with `DockItem::panel`, which has no title bar at all, so
    // this never actually renders — kept `None` regardless, since a zoom control makes no sense
    // for the main body.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for TablePanel {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_2()
            .child(Table::new(&self.state).bordered(false))
    }
}

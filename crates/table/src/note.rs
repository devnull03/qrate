//! The grid's right-click menu, the corner marker on anything a diagnostic points at, and the
//! floating note editor.
//!
//! Right-click uses `ContextMenuExt` on our own cell div, not `TableDelegate::context_menu` —
//! that hook only fires while `right_clicked_row` is `Some`, which `on_cell_right_click` clears.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Path, SharedString, Styled as _, Window, canvas, deferred, div, point, px,
};
use gpui_component::input::Input;
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::table::TableState;
use gpui_component::{ActiveTheme as _, h_flex};

use diagnostics::{Diagnostics, Location, Severity, severity_color};

use crate::TableStateHandle;
use crate::delegate::{QrateTableDelegate, TableChanged};
use crate::floating::clamped_float;

/// Side of the square the corner triangle fills.
const MARKER: f32 = 7.;

/// How long the pointer rests before a note reveals itself. gpui does the timing, so nothing here
/// tracks hover state — the table has none.
pub(crate) const HOVER_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

const EDITOR_W: f32 = 280.;
const EDITOR_H: f32 = 132.;

/// What was right-clicked. Copy/edit/filter only make sense over a cell; a row or a column can
/// only carry a note.
#[derive(Clone, Copy)]
pub(crate) enum Target {
    /// Source row and data column — not view/table indices.
    Cell {
        row: usize,
        col: usize,
    },
    Row(usize),
    Column(usize),
}

/// The corner tag on anything a diagnostic points at, painted through `canvas` because gpui has
/// no triangular border.
pub(crate) fn marker(severity: Severity, cx: &App) -> impl IntoElement {
    // Muted grey vanishes at 7px, so notes borrow the accent.
    let color = if severity == Severity::Note {
        cx.theme().accent
    } else {
        severity_color(severity, cx)
    };
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut path = Path::new(bounds.origin);
            path.line_to(bounds.origin + point(bounds.size.width, px(0.)));
            path.line_to(bounds.origin + point(bounds.size.width, bounds.size.height));
            window.paint_path(path, color);
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .size(px(MARKER))
}

/// Every diagnostic at a location, newline-joined — the cell's hover tooltip.
pub(crate) fn tooltip_text(location: &Location, cx: &App) -> Option<SharedString> {
    let messages: Vec<_> = Diagnostics::at(
        &location.dataset,
        location.row,
        location.column.as_deref(),
        cx,
    )
    .map(|d| d.message.as_ref())
    .collect();
    (!messages.is_empty()).then(|| messages.join("\n").into())
}

/// Build the right-click menu for `target`. Live state is read off the table global here rather
/// than captured at render time, the same way `filter::filter_dropdown` does it.
pub(crate) fn menu(
    target: Target,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(table) = cx
        .try_global::<TableStateHandle>()
        .and_then(|h| h.0.upgrade())
    else {
        return menu;
    };

    let (row, col) = match target {
        Target::Cell { row, col } => (Some(row), Some(col)),
        Target::Row(row) => (Some(row), None),
        Target::Column(col) => (None, Some(col)),
    };
    let (location, cell_text, row_tsv) = {
        let state = table.read(cx);
        let delegate = state.delegate();
        (
            delegate.location(row, col),
            match (row, col) {
                (Some(r), Some(c)) => delegate.cell(r, c).cloned().unwrap_or_default(),
                _ => SharedString::default(),
            },
            row.map(|r| {
                delegate
                    .row_fields(r)
                    .into_iter()
                    .map(|(_, v)| v.to_string())
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .unwrap_or_default(),
        )
    };
    let note = Diagnostics::note_at(
        &location.dataset,
        location.row,
        location.column.as_deref(),
        cx,
    );

    let copy_row = |menu: PopupMenu| {
        menu.item(PopupMenuItem::new("Copy row").on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(row_tsv.clone()))
        }))
    };
    let menu = match target {
        Target::Cell { row, col } => {
            let (edit_table, filter_table) = (table.clone(), table.clone());
            let (copied, value) = (cell_text.clone(), cell_text);
            copy_row(
                menu.item(PopupMenuItem::new("Copy").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()))
                })),
            )
            .item(
                PopupMenuItem::new("Edit cell").on_click(move |_, window, cx| {
                    edit_table.update(cx, |state, cx| {
                        crate::editing::start(state.delegate_mut(), row, col, window, cx);
                        cx.notify();
                    });
                }),
            )
            .item(
                PopupMenuItem::new("Filter by this value").on_click(move |_, _, cx| {
                    filter_table.update(cx, |state, cx| {
                        state.delegate_mut().keep_only_value(col, &value);
                        cx.emit(TableChanged);
                        cx.notify();
                    });
                }),
            )
            .separator()
        }
        Target::Row(_) => copy_row(menu).separator(),
        Target::Column(_) => menu,
    };

    menu.submenu("Notes", window, cx, move |sub, _window, _cx| {
        let (open_table, existing, loc) = (table.clone(), note.clone(), location.clone());
        let sub = sub.item(
            PopupMenuItem::new(if existing.is_some() {
                "Edit note…"
            } else {
                "Add note…"
            })
            .on_click(move |_, window, cx| {
                let text = existing.clone().unwrap_or_default();
                open(&open_table, loc.clone(), text, window, cx);
            }),
        );
        let loc = location.clone();
        sub.when(note.is_some(), |sub| {
            sub.item(
                PopupMenuItem::new("Delete note")
                    .on_click(move |_, _, cx| Diagnostics::set_note(loc.clone(), "".into(), cx)),
            )
        })
    })
}

/// Point the shared note editor at `location`, seeded with `text`, and focus it.
fn open(
    table: &Entity<TableState<QrateTableDelegate>>,
    location: Location,
    text: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    table.update(cx, |state, cx| {
        let editor = state.delegate().note_editor.clone();
        editor.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.focus(window, cx);
        });
        state.delegate_mut().note_edit = Some(location);
        cx.notify();
    });
}

/// The floating note editor, or `None` unless it is open on exactly these coordinates — every
/// cell, row-number cell, and column header calls this with its own, and one of them wins.
/// Same box as the cell editor (`cell.rs`), set apart by an accent border and a scope strip.
pub(crate) fn editor(
    delegate: &QrateTableDelegate,
    row: Option<usize>,
    data_col: Option<usize>,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> Option<AnyElement> {
    let location = delegate.note_edit.clone()?;
    // The cheap row comparison first: this runs once per rendered cell.
    if location.row != row || location.column != data_col.map(|c| delegate.column_name(c)) {
        return None;
    }
    let bounds = cx
        .try_global::<crate::TableViewportBounds>()
        .map(|b| b.0)
        .unwrap_or_default();

    let scope: SharedString = match (location.row, location.column.as_ref()) {
        (Some(r), Some(c)) => format!("Note on row {} · {c}", r + 1).into(),
        (Some(r), None) => format!("Note on row {}", r + 1).into(),
        (None, Some(c)) => format!("Note on column {c}").into(),
        (None, None) => "Note".into(),
    };

    Some(
        deferred(clamped_float(
            bounds,
            div()
                .occlude()
                .w(px(EDITOR_W))
                .h(px(EDITOR_H))
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().accent)
                .rounded(cx.theme().radius)
                .shadow_lg()
                .child(
                    h_flex()
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(scope),
                )
                .child(Input::new(&delegate.note_editor).h_full()),
        ))
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here: the parent has gpui's prelude in scope, and the chained glob
    // makes gpui's `test` macro shadow the `#[test]` its own expansion emits, recursing until
    // rustc's stack overflows.
    use crate::{TablePanel, TableStateHandle};
    use diagnostics::{DATASET_MAIN, Diagnostic, Diagnostics, Location, Severity, Source};
    use gpui::TestAppContext;

    fn at(row: Option<usize>, column: Option<&str>) -> Location {
        Location {
            dataset: DATASET_MAIN.into(),
            row,
            column: column.map(|c| c.to_string().into()),
        }
    }

    /// Builds the whole grid with a marker on a cell, a row, and a column, then opens the note
    /// editor on the cell. Catches panics in the new element construction and, more usefully, the
    /// borrow ordering — every one of these reads the `Diagnostics` global while the delegate is
    /// borrowed and before the builder takes `&mut Context`.
    #[gpui::test]
    fn renders_markers_and_the_note_editor(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::project::CurrentProject {
                file: std::env::temp_dir().join("qrate-note-smoke.qrate"),
                data: settings::project::ProjectData {
                    name: "T".into(),
                    columns: Vec::new(),
                    headers: vec!["Title".into(), "Year".into()],
                    rows: vec![
                        vec!["a".into(), "1900".into()],
                        vec!["b".into(), "1901".into()],
                    ],
                    values: Default::default(),
                },
            });

            let note = |location| Diagnostic {
                location,
                severity: Severity::Note,
                source: Source::Note,
                message: "look at this".into(),
            };
            // A cell note, a whole-row note, and a whole-column note — one per marker site.
            Diagnostics::set(
                &Source::Note,
                DATASET_MAIN,
                vec![
                    note(at(Some(0), Some("Title"))),
                    note(at(Some(1), None)),
                    note(at(None, Some("Year"))),
                ],
                cx,
            );
            // An error on the same cell as the note must win the marker's colour.
            let v = Source::Validator("v".into());
            Diagnostics::set(
                &v,
                DATASET_MAIN,
                vec![Diagnostic {
                    location: at(Some(0), Some("Title")),
                    severity: Severity::Error,
                    source: v.clone(),
                    message: "bad".into(),
                }],
                cx,
            );
            assert_eq!(
                Diagnostics::worst_at(DATASET_MAIN, Some(0), Some("Title"), cx),
                Some(Severity::Error)
            );
        });

        cx.add_window_view(TablePanel::new);

        cx.update(|cx| {
            let state = cx
                .try_global::<TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .expect("the panel publishes its state handle");
            state.update(cx, |state, cx| {
                state.delegate_mut().note_edit = Some(at(Some(0), Some("Title")));
                cx.notify();
            });
            // Written by `panel.rs`'s render canvas, so its presence is what proves the
            // virtualized cells above really got built.
            assert!(
                cx.try_global::<crate::TableViewportBounds>().is_some(),
                "the table never painted, so nothing here was exercised"
            );
        });
        cx.run_until_parked();
    }
}

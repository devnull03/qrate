//! Google-Sheets-style per-column filter, rendered in the column header via `render_th`: a
//! `Combobox` in multi-select mode listing the column's distinct values, where unchecking a value
//! hides its rows. The delegate owns the *excluded* set per column
//! (`QrateTableDelegate::filters`); this module is only the UI over it.
//!
//! The combobox tracks what is *kept*, the inverse of what the delegate stores. Selection is
//! pushed from the delegate on render and pulled back in the `Change` subscription, so the excluded
//! set stays the one answer and nothing has to reconcile two.

use diagnostics::Diagnostics;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, canvas, div, px,
};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{
    ActiveTheme, IconName, IndexPath, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    combobox::{Combobox, ComboboxEvent, ComboboxState},
    h_flex,
    searchable_list::{SearchableListItem, SearchableVec},
    table::TableState,
};

use crate::note::{self, Target};
use crate::{EditSpawn, TableStateHandle, delegate::QrateTableDelegate, editing::EditState};

/// One distinct value in a column's checklist.
#[derive(Clone, PartialEq)]
pub(crate) struct FilterValue(SharedString);

impl SearchableListItem for FilterValue {
    type Value = SharedString;

    /// A blank cell is a real, selectable value — give it a visible stand-in so the row isn't an
    /// unclickable-looking empty line.
    fn title(&self) -> SharedString {
        if self.0.is_empty() {
            "Empty".into()
        } else {
            self.0.clone()
        }
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn render(&self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Muted, so the stand-in reads as "this cell is blank" rather than a literal cell
        // containing "Empty".
        div()
            .w_full()
            .truncate()
            .when(self.0.is_empty(), |t| {
                t.text_color(cx.theme().muted_foreground)
            })
            .child(self.title())
    }
}

/// Parks a column combobox's `Change` subscription for as long as the header element lives. Only a
/// type for `use_keyed_state`, which stores one value per key.
struct FilterSub(#[allow(dead_code)] Subscription);

/// The header cell for a data column: its name, plus the filter dropdown on columns where
/// filtering is switched on. Filtering is opt-in per column (see
/// [`QrateTableDelegate::set_column_filter_enabled`]), so most headers render just the name.
pub(crate) fn render_th(
    delegate: &QrateTableDelegate,
    data_col: usize,
    window: &mut Window,
    cx: &mut Context<TableState<QrateTableDelegate>>,
) -> AnyElement {
    let name = delegate.column_name(data_col);
    let active = delegate.column_has_filter(data_col);
    // A rename floats the shared cell editor over this header, so the header measures its own rect
    // the way `cell.rs` does — once, on the frame the rename opens.
    let renaming = EditState::Renaming { col: data_col };
    let capture = delegate.editing == renaming
        && cx
            .try_global::<EditSpawn>()
            .is_none_or(|s| s.at != renaming);
    // Highlight this header while its column holds the active (edited/selected) cell. The header
    // row is sticky, so the highlight stays put as the grid scrolls under it.
    let editing_col = delegate.active_cell().is_some_and(|(_, c)| c == data_col);

    let location = delegate.location(None, Some(data_col));
    let worst = Diagnostics::worst_at(&location.dataset, None, location.column.as_deref(), cx);
    let tip = note::tooltip_text(&location, cx);
    let note_editor = note::editor(delegate, None, Some(data_col), cx);

    h_flex()
        .id(("col-note", data_col))
        .size_full()
        .justify_between()
        .items_center()
        .gap_1()
        .when(editing_col, |th| th.bg(cx.theme().secondary_hover))
        .child(div().flex_1().min_w_0().truncate().child(name))
        .when_some(worst, |th, severity| th.child(note::marker(severity, cx)))
        .when_some(tip, |th, text| {
            th.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            })
            .tooltip_show_delay(note::HOVER_DELAY)
        })
        .context_menu(move |menu, window, cx| {
            note::menu(Target::Column(data_col), menu, window, cx)
        })
        .when_some(note_editor, |th, editor| th.child(editor))
        .when(capture, |th| {
            th.child(
                canvas(
                    move |bounds, window, cx| {
                        cx.set_global(EditSpawn {
                            at: renaming,
                            bounds: crate::cell::full_cell(bounds),
                            scroll: None,
                        });
                        // `panel.rs` only reads this on its *next* render, and nothing else
                        // schedules one. See the same deferral in `cell.rs`.
                        window.defer(cx, |window, _| window.refresh());
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
        })
        .when(delegate.column_filter_enabled(data_col), |th| {
            // Wrapped and pinned to its own width: `Combobox` lays its trigger row out `w_full`,
            // which in a flex row against a `flex_1` title resolves to "all of it" — the funnel
            // button ended up spanning the header and the column name had nowhere to render.
            th.child(
                div()
                    .flex_none()
                    .child(filter_dropdown(delegate, data_col, active, window, cx)),
            )
        })
        .into_any_element()
}

/// The dropdown: a multi-select over the column's distinct values, with All/None in the footer.
fn filter_dropdown(
    delegate: &QrateTableDelegate,
    data_col: usize,
    active: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state: Entity<ComboboxState<SearchableVec<FilterValue>>> =
        window.use_keyed_state(("col-filter", data_col), cx, |window, cx| {
            ComboboxState::new(SearchableVec::new(Vec::new()), vec![], window, cx)
                .multiple(true)
                .searchable(true)
        });

    // Subscribed once per column, parked in its own keyed state. The handler reaches the table
    // through the published handle rather than a capture, so it survives the panel being rebuilt.
    window.use_keyed_state(("col-filter-sub", data_col), cx, |_window, cx| {
        FilterSub(cx.subscribe(&state, move |_, _, event, cx| {
            let ComboboxEvent::Change(kept) = event else {
                return;
            };
            let kept = kept.clone();
            let Some(table) = cx
                .try_global::<TableStateHandle>()
                .and_then(|h| h.0.upgrade())
            else {
                return;
            };
            table.update(cx, |s, cx| {
                s.delegate_mut().set_column_kept(data_col, &kept);
                cx.emit(crate::TableChanged);
                cx.notify();
            });
        }))
    });

    // `column_values` is O(rows), and `set_items` throws away the active search — so both only run
    // when the data behind the list actually moved.
    let generation = window.use_keyed_state(("col-filter-gen", data_col), cx, |_, _| u64::MAX);
    let values: Vec<FilterValue> = delegate
        .column_values(data_col)
        .into_iter()
        .map(FilterValue)
        .collect();
    if generation.read(cx) != &delegate.values_generation() {
        state.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(values.clone()), window, cx);
        });
        generation.update(cx, |g, _| *g = delegate.values_generation());
    }

    // The delegate stores exclusions; the combobox tracks what is kept.
    let kept: Vec<SharedString> = values
        .iter()
        .filter(|v| !delegate.is_filter_excluded(data_col, &v.0))
        .map(|v| v.0.clone())
        .collect();
    let selected = state.read(cx).selected_values();
    if selected.len() != kept.len() || !kept.iter().all(|v| selected.contains(v)) {
        let indices: Vec<IndexPath> = values
            .iter()
            .enumerate()
            .filter(|(_, v)| kept.contains(&v.0))
            .map(|(ix, _)| IndexPath::new(ix))
            .collect();
        state.update(cx, |state, cx| {
            state.set_selected_indices(indices, window, cx);
        });
    }

    let all = values.clone();
    Combobox::new(&state)
        .menu_width(px(240.))
        .menu_max_h(px(240.))
        .search_placeholder("Search values")
        .appearance(false)
        .render_trigger(move |_ctx, _, _| {
            Button::new(("col-filter-btn", data_col))
                // The bundled icon set has no funnel; `settings-2` is its slider glyph, the usual
                // stand-in for "filter/adjust".
                .icon(IconName::Settings2)
                .ghost()
                .xsmall()
                // Highlight the affordance while the column is narrowing the view.
                .selected(active)
        })
        .footer(move |_, _| {
            let bulk =
                |label: &'static str, id: &'static str, keep_all: bool, all: Vec<FilterValue>| {
                    let state = state.clone();
                    Button::new((id, data_col))
                        .outline()
                        .xsmall()
                        .label(label)
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            let indices: Vec<IndexPath> = if keep_all {
                                (0..all.len()).map(IndexPath::new).collect()
                            } else {
                                Vec::new()
                            };
                            state.update(cx, |state, cx| {
                                state.set_selected_indices(indices, window, cx);
                                cx.emit(ComboboxEvent::Change(state.selected_values()));
                            });
                        })
                };
            h_flex()
                .gap_1()
                .p_1()
                .child(bulk("All", "filter-all", true, all.clone()))
                .child(bulk("None", "filter-none", false, all.clone()))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note on `note.rs`'s test module.
    use crate::{TablePanel, TableStateHandle};
    use gpui::TestAppContext;
    use gpui_component::table::TableDelegate as _;

    fn project() -> settings::project::CurrentProject {
        settings::project::CurrentProject {
            file: std::env::temp_dir().join("qrate-filter-smoke.qrate"),
            data: settings::project::ProjectData {
                name: "T".into(),
                columns: Vec::new(),
                headers: vec!["Medium".into()],
                rows: vec![
                    vec!["Film".into()],
                    vec!["Video".into()],
                    // A blank cell is a real, selectable value — it gets the "Empty" stand-in.
                    vec!["".into()],
                    vec!["Film".into()],
                ],
                values: Default::default(),
            },
        }
    }

    /// Paints the header of a filter-enabled column, which is the only thing that builds the
    /// `Combobox` and its `use_keyed_state` entities. Nothing else in the suite reaches that path,
    /// and a mistake there is a panic at render, not a compile error.
    #[gpui::test]
    fn renders_the_filter_dropdown(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(project());
        });

        cx.add_window_view(TablePanel::new);

        cx.update(|cx| {
            let state = cx
                .try_global::<TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .expect("the panel publishes its state handle");
            state.update(cx, |state, cx| {
                state.delegate_mut().set_column_filter_enabled(0, true);
                cx.notify();
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            assert!(
                cx.try_global::<crate::TableViewportBounds>().is_some(),
                "the table never painted, so the dropdown was never built"
            );
        });
    }

    /// The delegate stores exclusions while the dropdown reports what is kept, so this inversion is
    /// the one place the two models meet.
    #[gpui::test]
    fn keeping_a_value_excludes_the_rest(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(project());
        });
        cx.add_window_view(TablePanel::new);

        cx.update(|cx| {
            let state = cx
                .try_global::<TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .expect("the panel publishes its state handle");
            state.update(cx, |state, cx| {
                let delegate = state.delegate_mut();
                delegate.set_column_kept(0, &["Film".into()]);

                assert!(!delegate.is_filter_excluded(0, &"Film".into()));
                assert!(delegate.is_filter_excluded(0, &"Video".into()));
                assert!(
                    delegate.is_filter_excluded(0, &"".into()),
                    "the blank value is excluded like any other"
                );
                assert_eq!(
                    delegate.rows_count(cx),
                    2,
                    "only the two Film rows stay visible"
                );

                // Keeping everything is how "All" clears the filter.
                delegate.set_column_kept(0, &["Film".into(), "Video".into(), "".into()]);
                assert!(!delegate.column_has_filter(0));
                assert_eq!(delegate.rows_count(cx), 4);
            });
        });
    }
}

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::{DiagnosticHooks, Diagnostics, Location, Severity, severity_color};

/// Which severities the active tab admits. `Info` folds in with `Note`: two quiet buckets is a
/// distinction without a difference for someone cataloguing a collection.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Errors,
    Warnings,
    Notes,
}

impl Filter {
    /// Tab order, left to right.
    const ALL: [Filter; 4] = [Filter::All, Filter::Errors, Filter::Warnings, Filter::Notes];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Errors => "Errors",
            Filter::Warnings => "Warnings",
            Filter::Notes => "Notes",
        }
    }

    fn admits(self, severity: Severity) -> bool {
        match self {
            Filter::All => true,
            Filter::Errors => severity == Severity::Error,
            Filter::Warnings => severity == Severity::Warning,
            Filter::Notes => matches!(severity, Severity::Note | Severity::Info),
        }
    }
}

/// One list entry, resolved out of the store once per change rather than once per frame. Colours
/// are not cached here because they come from the theme, which can change without the store doing.
struct Row {
    icon: IconName,
    severity: Severity,
    /// Which cell, row, or column this points at, already phrased for display.
    scope: SharedString,
    /// A row- or column-wide note, which reads differently from a cell coordinate.
    wide: bool,
    message: SharedString,
    source: SharedString,
    location: Location,
}

/// Bottom dock: every open problem, filtered by severity, click to jump to it.
pub struct ProblemsPanel {
    focus_handle: FocusHandle,
    filter: Filter,
    /// The visible list, in display order. Behind an `Rc` so each render hands the virtual list a
    /// handle instead of a copy. Rebuilt only when the store or the filter changes — a sheet with
    /// a few hundred findings would otherwise sort and rebuild all of them every frame.
    rows: Rc<Vec<Row>>,
    /// Per-tab totals, which count every severity regardless of the active filter.
    counts: [usize; 4],
    /// Refreshes on any store change. One `observe_global` and no re-binding — unlike
    /// `TableStateHandle`, the `Diagnostics` global is plain data that is never rebuilt.
    _sub: Subscription,
}

impl ProblemsPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            filter: Filter::All,
            rows: Rc::default(),
            counts: [0; 4],
            _sub: cx.observe_global::<Diagnostics>(|this, cx| {
                this.refresh(cx);
                cx.notify();
            }),
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &App) {
        self.counts = Filter::ALL.map(|f| {
            Diagnostics::all(cx)
                .iter()
                .filter(|d| f.admits(d.severity))
                .count()
        });

        let mut rows: Vec<Row> = Diagnostics::all(cx)
            .iter()
            .filter(|d| self.filter.admits(d.severity))
            .map(|d| {
                let (scope, wide) = match (d.location.row, d.location.column.as_ref()) {
                    (Some(r), Some(c)) => (format!("Row {} · {c}", r + 1).into(), false),
                    (Some(r), None) => (format!("Row {}", r + 1).into(), true),
                    (None, Some(c)) => (c.clone(), true),
                    (None, None) => (d.location.dataset.clone(), true),
                };
                Row {
                    severity: d.severity,
                    icon: match d.severity {
                        Severity::Error => IconName::CircleX,
                        Severity::Warning => IconName::TriangleAlert,
                        Severity::Info | Severity::Note => IconName::Info,
                    },
                    scope,
                    wide,
                    message: d.message.clone(),
                    source: d.source.label(),
                    location: d.location.clone(),
                }
            })
            .collect();
        // Errors first, then reading order down the table. A whole-row/column note sorts ahead
        // of the cells it covers because `None` orders before `Some`.
        rows.sort_by(|a, b| {
            (a.severity, &a.location.row, &a.location.column).cmp(&(
                b.severity,
                &b.location.row,
                &b.location.column,
            ))
        });
        self.rows = Rc::new(rows);
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

    // The library always renders the ⋯ menu button; these just empty it of Close + Zoom.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let muted = cx.theme().muted_foreground;

        v_flex()
            .size_full()
            .child(
                TabBar::new("problems-filter")
                    .selected_index(
                        Filter::ALL
                            .iter()
                            .position(|f| *f == self.filter)
                            .unwrap_or(0),
                    )
                    .children(
                        Filter::ALL
                            .iter()
                            .zip(self.counts)
                            .map(|(f, n)| Tab::new().label(format!("{} ({n})", f.label()))),
                    )
                    .on_click(cx.listener(|this, ix: &usize, _w, cx| {
                        this.filter = Filter::ALL[*ix];
                        this.refresh(cx);
                        cx.notify();
                    })),
            )
            .when(rows.is_empty(), |panel| {
                panel.child(div().p_3().text_color(muted).child("No problems"))
            })
            .when(!rows.is_empty(), |panel| {
                // Only the visible slice is built. A row is one line high — the message is
                // ellipsised rather than wrapped — which is what lets `uniform_list` place
                // thousands of them from a single measurement.
                panel.child(
                    uniform_list("problems-list", rows.len(), move |range, _window, cx| {
                        let (hover_bg, chip_bg) =
                            (cx.theme().secondary_hover, cx.theme().secondary);
                        let muted = cx.theme().muted_foreground;
                        rows[range.clone()]
                            .iter()
                            .zip(range)
                            .map(|(r, ix)| {
                                let location = r.location.clone();
                                h_flex()
                                    .id(ix)
                                    .w_full()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .cursor_pointer()
                                    .hover(|row| row.bg(hover_bg))
                                    .child(
                                        Icon::new(r.icon.clone())
                                            .small()
                                            .text_color(severity_color(r.severity, cx)),
                                    )
                                    // A row- or column-wide note gets a chip so it doesn't read
                                    // as a cell coordinate that just lost half its address.
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_sm()
                                            .text_color(muted)
                                            .when(r.wide, |s| s.px_1().rounded_sm().bg(chip_bg))
                                            .child(r.scope.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(r.message.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(r.source.clone()),
                                    )
                                    .on_click({
                                        let location = location.clone();
                                        move |_, _, cx| {
                                            if let Some(hooks) =
                                                cx.try_global::<DiagnosticHooks>().copied()
                                            {
                                                (hooks.reveal)(&location, cx);
                                            }
                                        }
                                    })
                                    // The same corrections a cell offers. Read on open, because a
                                    // row is built from a diagnostic and the cell may have been
                                    // edited since the one that produced it was published.
                                    .context_menu(move |menu, window, cx| {
                                        let Some(hooks) =
                                            cx.try_global::<DiagnosticHooks>().copied()
                                        else {
                                            return menu;
                                        };
                                        let Some(text) = (hooks.text_at)(&location, cx) else {
                                            return menu;
                                        };
                                        let menu = {
                                            let location = location.clone();
                                            crate::spelling::menu(
                                                &text,
                                                menu,
                                                window,
                                                cx,
                                                move |fixed, cx| {
                                                    (hooks.set_text)(&location, fixed, cx)
                                                },
                                            )
                                        };
                                        let applied = location.clone();
                                        crate::fixes::menu(
                                            &location,
                                            &text,
                                            menu,
                                            window,
                                            cx,
                                            move |fixed, cx| (hooks.set_text)(&applied, fixed, cx),
                                        )
                                    })
                            })
                            .collect()
                    })
                    .size_full()
                    .min_h_0(),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note in `lib.rs`'s test module.
    use crate::panel::Filter;
    use crate::{DATASET_MAIN, Diagnostic, Diagnostics, Location, ProblemsPanel, Severity, Source};
    use gpui::TestAppContext;

    #[gpui::test]
    fn renders_the_empty_state_without_a_store(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        // No `Diagnostics` global set, so `all()` yields nothing and the list takes its empty
        // branch. Mirrors a dev launch with no project open.
        cx.add_window_view(ProblemsPanel::new);
    }

    #[gpui::test]
    fn renders_every_location_shape(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            let at = |row, column: Option<&str>| Location {
                dataset: DATASET_MAIN.into(),
                row,
                column: column.map(|c| c.to_string().into()),
            };
            // One of each: cell, row-wide, column-wide, dataset-wide — the four `match` arms
            // that build a row's scope label, plus a sort across mixed `None`/`Some`.
            Diagnostics::set(
                &Source::Note,
                DATASET_MAIN,
                [
                    (Severity::Note, at(Some(4), Some("Title"))),
                    (Severity::Error, at(Some(1), None)),
                    (Severity::Warning, at(None, Some("Digital ID"))),
                    (Severity::Info, at(None, None)),
                ]
                .into_iter()
                .map(|(severity, location)| Diagnostic {
                    location,
                    severity,
                    source: Source::Note,
                    message: "m".into(),
                })
                .collect(),
                cx,
            );
        });
        cx.add_window_view(ProblemsPanel::new);
    }

    #[test]
    fn tabs_partition_every_severity() {
        for severity in [
            Severity::Error,
            Severity::Warning,
            Severity::Info,
            Severity::Note,
        ] {
            let admitting: Vec<_> = Filter::ALL
                .iter()
                .filter(|f| f.admits(severity))
                .map(|f| f.label())
                .collect();
            // Always "All" plus exactly one specific tab — nothing is unreachable or double-listed.
            assert_eq!(admitting.len(), 2, "{severity:?} lands in {admitting:?}");
        }
    }
}

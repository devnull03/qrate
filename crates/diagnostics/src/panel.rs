use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
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
            Filter::Notes => severity == Severity::Note,
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

/// Bottom dock: every open problem, filtered by severity and source, click to jump to it.
pub struct ProblemsPanel {
    focus_handle: FocusHandle,
    filter: Filter,
    /// Which producer's findings to show, or all of them. Severity says how loud a problem is;
    /// this says who is talking — with a speller, a files check, an authority, and any number of
    /// plugins all publishing into one list, "only show me the spelling" is the difference
    /// between a list and a pile.
    source: Option<SharedString>,
    /// Distinct sources currently publishing, in display order. Only what is actually here, so
    /// the menu never offers a filter that would empty the list.
    sources: Rc<Vec<SharedString>>,
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
            source: None,
            sources: Rc::default(),
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

    /// Whether the source filter lets this one through. Its own function because both the rows
    /// and the tab counts have to agree — a tab reading "Errors (12)" over a list of two is worse
    /// than no count at all.
    fn admits_source(&self, d: &crate::Diagnostic) -> bool {
        self.source
            .as_ref()
            .is_none_or(|wanted| &d.source.label() == wanted)
    }

    /// The "which producer" dropdown. Hidden until something is publishing, so a project with
    /// only spelling findings doesn't grow a menu with one entry in it.
    fn source_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (sources, selected, panel) = (self.sources.clone(), self.source.clone(), cx.entity());
        let label = selected
            .clone()
            .unwrap_or_else(|| SharedString::from("All sources"));

        div().pr_1().when(sources.len() > 1, |el| {
            el.child(
                Button::new("problems-source")
                    .ghost()
                    .small()
                    .label(label)
                    .dropdown_menu(move |menu, _window, _cx| {
                        let pick =
                            |menu: PopupMenu,
                             name: Option<SharedString>,
                             panel: &Entity<ProblemsPanel>| {
                                let (panel, chosen) = (panel.clone(), name.clone());
                                menu.item(
                                    PopupMenuItem::new(
                                        name.unwrap_or_else(|| SharedString::from("All sources")),
                                    )
                                    .on_click(
                                        move |_, _, cx| {
                                            panel.update(cx, |this, cx| {
                                                this.source = chosen.clone();
                                                this.refresh(cx);
                                                cx.notify();
                                            });
                                        },
                                    ),
                                )
                            };
                        let menu = pick(menu, None, &panel).separator();
                        sources
                            .iter()
                            .fold(menu, |menu, name| pick(menu, Some(name.clone()), &panel))
                    }),
            )
        })
    }

    fn refresh(&mut self, cx: &App) {
        let mut sources: Vec<SharedString> = Diagnostics::all(cx)
            .iter()
            .map(|d| d.source.label())
            .collect();
        sources.sort();
        sources.dedup();
        self.sources = Rc::new(sources);

        self.counts = Filter::ALL.map(|f| {
            Diagnostics::all(cx)
                .iter()
                .filter(|d| f.admits(d.severity) && self.admits_source(d))
                .count()
        });

        let mut rows: Vec<Row> = Diagnostics::all(cx)
            .iter()
            .filter(|d| self.filter.admits(d.severity) && self.admits_source(d))
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
                        Severity::Note => IconName::Info,
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
            // The dock focuses this panel's handle when its tab is clicked. Without an element
            // tracking it, focus lands nowhere: the dispatch path collapses to the window root and
            // the window-wide shortcuts stop reaching their handlers.
            .track_focus(&self.focus_handle)
            .id("problems-panel")
            .role(Role::Group)
            .aria_label("Problems")
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
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
                    .child(self.source_menu(cx)),
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
                row_id: None,
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
                    (Severity::Note, at(None, None)),
                ]
                .into_iter()
                .map(|(severity, location)| Diagnostic {
                    location,
                    severity,
                    source: Source::Note,
                    message: "m".into(),
                    filed: None,
                })
                .collect(),
                cx,
            );
        });
        cx.add_window_view(ProblemsPanel::new);
    }

    #[test]
    fn tabs_partition_every_severity() {
        for severity in [Severity::Error, Severity::Warning, Severity::Note] {
            let admitting: Vec<_> = Filter::ALL
                .iter()
                .filter(|f| f.admits(severity))
                .map(|f| f.label())
                .collect();
            // Always "All" plus exactly one specific tab — nothing is unreachable or double-listed.
            assert_eq!(admitting.len(), 2, "{severity:?} lands in {admitting:?}");
        }
    }

    /// Picking a source narrows the list *and* the tab counts: a tab reading "Errors (2)" over a
    /// list showing one is worse than no count at all.
    #[gpui::test]
    fn choosing_a_source_narrows_the_rows_and_the_counts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            let at = |row| Location {
                dataset: DATASET_MAIN.into(),
                row: Some(row),
                row_id: None,
                column: Some("Title".into()),
            };
            for (source, rows) in [("spell", vec![0, 1]), ("files", vec![2])] {
                Diagnostics::set(
                    &Source::Validator(source.into()),
                    DATASET_MAIN,
                    rows.into_iter()
                        .map(|r| Diagnostic {
                            location: at(r),
                            severity: Severity::Error,
                            source: Source::Validator(source.into()),
                            message: "m".into(),
                            filed: None,
                        })
                        .collect(),
                    cx,
                );
            }
        });

        let (panel, cx) = cx.add_window_view(ProblemsPanel::new);
        panel.update(cx, |this, cx| {
            assert_eq!(this.sources.len(), 2, "both producers are offered");
            assert_eq!(this.rows.len(), 3);
            assert_eq!(this.counts[0], 3, "the All tab counts everything");

            this.source = Some("files".into());
            this.refresh(cx);
            assert_eq!(this.rows.len(), 1, "only the files finding survives");
            assert_eq!(this.counts[0], 1, "and the tab count agrees");

            this.source = None;
            this.refresh(cx);
            assert_eq!(this.rows.len(), 3, "clearing the filter brings them back");
        });
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::{
    Diagnostic, DiagnosticHooks, Diagnostics, Location, Scope, Severity, Source, severity_color,
};

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
    group: Option<String>,
    child: bool,
}

fn group_id(d: &Diagnostic) -> Option<String> {
    if d.source == Source::Note {
        return None;
    }
    d.group.as_ref().map(|group| {
        format!(
            "{:?}",
            (&d.location.dataset, &d.source, d.severity, &group.key)
        )
    })
}

/// Atomic findings stay in the store; only this flat, virtualized view collapses them.
fn project(mut items: Vec<&Diagnostic>, expanded: &BTreeSet<String>) -> Vec<Row> {
    items.sort_by(|a, b| {
        (
            a.severity,
            &a.location.dataset,
            a.location.row,
            &a.location.column,
        )
            .cmp(&(
                b.severity,
                &b.location.dataset,
                b.location.row,
                &b.location.column,
            ))
    });
    let mut groups: BTreeMap<String, Vec<&Diagnostic>> = BTreeMap::new();
    for d in &items {
        if let Some(id) = group_id(d) {
            groups.entry(id).or_default().push(d);
        }
    }
    let mut emitted = BTreeSet::new();
    items.retain(|d| group_id(d).is_none_or(|id| emitted.insert(id)));
    items.sort_by_cached_key(|d| {
        (
            d.severity,
            if group_id(d).is_some() {
                d.group.as_ref().expect("group metadata").summary.clone()
            } else {
                d.message.clone()
            },
            group_id(d),
        )
    });
    let occurrence = |d: &Diagnostic, child| {
        let (scope, wide) = match d.location.scope() {
            Scope::Cell { row, column } => (format!("Row {} · {column}", row + 1).into(), false),
            Scope::Row(row) => (format!("Row {}", row + 1).into(), true),
            Scope::Column(column) => (column.to_owned().into(), true),
            Scope::Dataset => (d.location.dataset.clone(), true),
        };
        Row {
            icon: match d.severity {
                Severity::Error => IconName::CircleX,
                Severity::Warning => IconName::TriangleAlert,
                Severity::Note => IconName::Info,
            },
            severity: d.severity,
            scope,
            wide,
            message: d.message.clone(),
            source: d.source.label(),
            location: d.location.clone(),
            group: None,
            child,
        }
    };
    let mut rows = Vec::new();
    for d in items {
        let Some(id) = group_id(d) else {
            rows.push(occurrence(d, false));
            continue;
        };
        let Some(members) = groups.remove(&id) else {
            continue;
        };
        let mut header = occurrence(d, false);
        let open = expanded.contains(&id);
        header.scope = format!(
            "{} · {} occurrences",
            if open { "▾" } else { "▸" },
            members.len()
        )
        .into();
        header.message = d
            .group
            .as_ref()
            .expect("group identity requires metadata")
            .summary
            .clone();
        header.wide = true;
        header.group = Some(id);
        rows.push(header);
        if open {
            rows.extend(members.into_iter().map(|member| occurrence(member, true)));
        }
    }
    rows
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
    expanded: BTreeSet<String>,
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
            expanded: BTreeSet::new(),
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

        let live: BTreeSet<_> = Diagnostics::all(cx).iter().filter_map(group_id).collect();
        self.expanded.retain(|id| live.contains(id));
        let items = Diagnostics::all(cx)
            .iter()
            .filter(|d| self.filter.admits(d.severity) && self.admits_source(d))
            .collect();
        self.rows = Rc::new(project(items, &self.expanded));
    }
}

impl Focusable for ProblemsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for ProblemsPanel {}

impl BasePanel for ProblemsPanel {
    fn panel_name(&self) -> &'static str {
        "ProblemsPanel"
    }

    // The library always renders the ⋯ menu button; this just empties it of Close.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> bool {
        true
    }
}

impl Panel for ProblemsPanel {
    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Problems")
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let panel_handle = cx.entity();
        let expanded = self.expanded.clone();
        let muted = cx.theme().muted_foreground;

        v_flex()
            .size_full()
            .tab_group()
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
                                let group = r.group.clone();
                                let is_group = group.is_some();
                                let is_expanded =
                                    group.as_ref().is_some_and(|id| expanded.contains(id));
                                let panel_handle = panel_handle.clone();
                                h_flex()
                                    .id(ix)
                                    .w_full()
                                    .h_8()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .when(r.child, |row| row.pl_6())
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
                                            .when(!is_group, |scope| scope.child(r.scope.clone()))
                                            .when_some(r.group.clone(), |scope, id| {
                                                let panel_handle = panel_handle.clone();
                                                scope.child(
                                                    Button::new(("expand-problem", ix))
                                                        .ghost()
                                                        .small()
                                                        .label(r.scope.clone())
                                                        .accessibility_label(format!(
                                                            "{} {}",
                                                            if is_expanded {
                                                                "Collapse"
                                                            } else {
                                                                "Expand"
                                                            },
                                                            r.message
                                                        ))
                                                        .tooltip(format!(
                                                            "Expand or collapse: {}",
                                                            r.message
                                                        ))
                                                        .on_click(move |_, _, cx| {
                                                            cx.stop_propagation();
                                                            panel_handle.update(cx, |this, cx| {
                                                                if !this.expanded.remove(&id) {
                                                                    this.expanded
                                                                        .insert(id.clone());
                                                                }
                                                                this.refresh(cx);
                                                                cx.notify();
                                                            });
                                                        }),
                                                )
                                            }),
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
                                            if let Some(id) = &group {
                                                panel_handle.update(cx, |this, cx| {
                                                    if !this.expanded.remove(id) {
                                                        this.expanded.insert(id.clone());
                                                    }
                                                    this.refresh(cx);
                                                    cx.notify();
                                                });
                                                return;
                                            }
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
                                        if is_group
                                            || !matches!(location.scope(), Scope::Cell { .. })
                                        {
                                            return menu;
                                        }
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

    fn repeated() -> Vec<Diagnostic> {
        (0..100)
            .map(|row| Diagnostic {
                location: Location::cell(DATASET_MAIN, row, None, "Title"),
                severity: Severity::Warning,
                source: Source::Validator("spell".into()),
                message: "misspelled: recieve".into(),
                group: Some(crate::DiagnosticGroup {
                    key: "en:recieve".into(),
                    summary: "misspelled: recieve".into(),
                }),
                filed: None,
            })
            .collect()
    }

    #[test]
    fn groups_are_explicit_scoped_and_expand_to_exact_locations() {
        let mut items = repeated();
        let closed = super::project(items.iter().collect(), &Default::default());
        assert_eq!(closed.len(), 1);
        assert!(closed[0].scope.contains("100 occurrences"));
        let expanded = [closed[0].group.clone().unwrap()].into_iter().collect();
        let open = super::project(items.iter().collect(), &expanded);
        assert_eq!(open.len(), 101);
        for (row, occurrence) in open[1..].iter().enumerate() {
            assert_eq!(occurrence.location.row, Some(row));
            assert!(occurrence.child);
            assert!(occurrence.group.is_none());
        }
        items[0].severity = Severity::Error;
        items[1].source = Source::Validator("other".into());
        items[2].location.dataset = "other".into();
        items[3].group = None;
        items[4].source = Source::Note;
        assert_eq!(
            super::project(items.iter().collect(), &Default::default()).len(),
            6
        );
    }

    #[gpui::test]
    fn clicking_a_group_expands_without_navigation(cx: &mut TestAppContext) {
        #[derive(Default)]
        struct Revealed(Vec<Location>);
        impl gpui::Global for Revealed {}
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(Revealed::default());
            cx.set_global(crate::DiagnosticHooks {
                reveal: |location, cx| {
                    use gpui::BorrowAppContext as _;
                    cx.update_global::<Revealed, _>(|seen, _| seen.0.push(location.clone()));
                },
                text_at: |_, _| None,
                set_text: |_, _, _| panic!("navigation must not edit"),
            });
            Diagnostics::set(
                &Source::Validator("spell".into()),
                DATASET_MAIN,
                repeated(),
                cx,
            );
        });
        let (panel, cx) = cx.add_window_view(ProblemsPanel::new);
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        cx.simulate_click(
            gpui::point(gpui::px(400.), gpui::px(48.)),
            Default::default(),
        );
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.rows.len(), 101);
            assert!(cx.global::<Revealed>().0.is_empty());
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        cx.simulate_click(
            gpui::point(gpui::px(400.), gpui::px(80.)),
            Default::default(),
        );
        cx.run_until_parked();
        panel.read_with(cx, |_, cx| {
            assert_eq!(
                cx.global::<Revealed>().0,
                [Location::cell(DATASET_MAIN, 0, None, "Title")]
            );
        });
        cx.simulate_click(
            gpui::point(gpui::px(100.), gpui::px(48.)),
            Default::default(),
        );
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| assert_eq!(panel.rows.len(), 1));
        let focus = panel.read_with(cx, |panel, _| panel.focus_handle.clone());
        cx.update(|window, cx| {
            _ = window.draw(cx);
            window.focus(&focus, cx);
            window.focus_prev(cx);
            _ = window.draw(cx);
        });
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            assert_eq!(
                panel.rows.len(),
                101,
                "the focused expansion button supports Enter"
            );
            assert_eq!(cx.global::<Revealed>().0.len(), 1);
        });
    }

    #[gpui::test]
    fn group_refresh_preserves_counts_and_prunes_expansion(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Diagnostics::set(
                &Source::Validator("spell".into()),
                DATASET_MAIN,
                repeated(),
                cx,
            );
        });
        let (panel, cx) = cx.add_window_view(ProblemsPanel::new);
        panel.update(cx, |this, cx| {
            assert_eq!(this.counts, [100, 0, 100, 0]);
            assert_eq!(this.rows.len(), 1);
            this.expanded.insert(this.rows[0].group.clone().unwrap());
            this.refresh(cx);
            assert_eq!(this.rows.len(), 101);
            this.filter = Filter::Errors;
            this.refresh(cx);
            assert!(this.rows.is_empty());
            assert_eq!(this.counts[2], 100);
            assert_eq!(this.expanded.len(), 1);
            Diagnostics::set(
                &Source::Validator("spell".into()),
                DATASET_MAIN,
                Vec::new(),
                cx,
            );
            this.refresh(cx);
            assert!(this.expanded.is_empty());
        });
    }

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
                    group: None,
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
                            group: None,
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

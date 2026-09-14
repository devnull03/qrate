use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_component::searchable_list::{SearchableListDelegate, SearchableListItem};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Selectable as _, Sizable as _, h_flex, v_flex,
};

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

    fn admits(self, diagnostic: &Diagnostic) -> bool {
        match self {
            Filter::All => diagnostic.source != Source::Note,
            Filter::Errors => {
                diagnostic.source != Source::Note && diagnostic.severity == Severity::Error
            }
            Filter::Warnings => {
                diagnostic.source != Source::Note && diagnostic.severity == Severity::Warning
            }
            Filter::Notes => diagnostic.source == Source::Note,
        }
    }
}

#[derive(Clone, PartialEq)]
struct SourceItem(SharedString);

/// The app's multi-select list — the grid filter, this panel's source filter, and the settings
/// pickers — searchable, with the check leading each row the way a menu's does.
pub struct CheckList<I> {
    items: Vec<I>,
    matched: Vec<I>,
}

impl<I: Clone> CheckList<I> {
    pub fn new(items: Vec<I>) -> Self {
        Self {
            matched: items.clone(),
            items,
        }
    }
}

impl<I: SearchableListItem + Clone + 'static> SearchableListDelegate for CheckList<I> {
    type Item = I;

    fn items_count(&self, _: usize) -> usize {
        self.matched.len()
    }

    fn item(&self, ix: IndexPath) -> Option<&Self::Item> {
        self.matched.get(ix.row)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.matched
            .iter()
            .position(|item| item.value() == value)
            .map(IndexPath::new)
    }

    fn perform_search(&mut self, query: &str, _: &mut Window, _: &mut App) -> Task<()> {
        self.matched = self
            .items
            .iter()
            .filter(|item| item.matches(query))
            .cloned()
            .collect();
        Task::ready(())
    }

    fn render_item(
        &self,
        _: IndexPath,
        item: &Self::Item,
        checked: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        Some(
            h_flex()
                .w_full()
                .items_start()
                .gap_1()
                .child(
                    Icon::new(IconName::Check)
                        .xsmall()
                        .mt(px(2.))
                        .when(!checked, |icon| icon.invisible()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_6()
                        .overflow_hidden()
                        .child(item.render(window, cx)),
                )
                .into_any_element(),
        )
    }
}

impl SearchableListItem for SourceItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.0.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().child(self.0.clone())
    }
}

struct SourceFilterSub(#[allow(dead_code)] Subscription);

#[derive(Clone)]
struct RowMember {
    location: Location,
    message: SharedString,
    subject: Option<SharedString>,
}

/// One list entry, resolved out of the store once per change rather than once per frame. Colours
/// are not cached here because they come from the theme, which can change without the store doing.
struct Row {
    icon: IconName,
    severity: Severity,
    /// The row and column cells, each empty when the finding does not narrow to one.
    row_label: SharedString,
    column: SharedString,
    message: SharedString,
    source: SharedString,
    source_key: SharedString,
    finding: Diagnostic,
    ignored: bool,
    location: Location,
    group: Option<String>,
    count: usize,
    members: Vec<RowMember>,
    depth: usize,
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

/// Joins a group id to one observed form. `group_id` is `Debug` output, which escapes it.
const FORM_SEPARATOR: char = '\u{1f}';

fn subject(d: &Diagnostic) -> Option<SharedString> {
    d.group.as_ref().and_then(|group| group.subject.clone())
}

fn occurrence(d: &Diagnostic, depth: usize) -> Row {
    let (row_label, column) = match d.location.scope() {
        Scope::Cell { row, column } => {
            (format!("Row {}", row + 1).into(), column.to_owned().into())
        }
        Scope::Row(row) => (format!("Row {}", row + 1).into(), SharedString::default()),
        Scope::Column(column) => (SharedString::default(), column.to_owned().into()),
        Scope::Dataset => (SharedString::default(), d.location.dataset.clone()),
    };
    Row {
        icon: match d.severity {
            Severity::Error => IconName::CircleX,
            Severity::Warning => IconName::TriangleAlert,
            Severity::Note => IconName::Info,
        },
        severity: d.severity,
        row_label,
        column,
        message: d.message.clone(),
        source: match d.source {
            Source::Note => SharedString::default(),
            _ => d.source.label(),
        },
        source_key: d.source.key(),
        finding: d.clone(),
        ignored: false,
        location: d.location.clone(),
        group: None,
        count: 1,
        members: Vec::new(),
        depth,
    }
}

/// A group header over `found`, or the lone finding itself. Each observed form nests one level
/// down by the same rule, so a form seen once is a plain row.
fn push_group(
    rows: &mut Vec<Row>,
    found: &[&Diagnostic],
    cluster: &[RowMember],
    id: String,
    message: SharedString,
    depth: usize,
    expanded: &BTreeSet<String>,
) {
    if let [single] = found {
        rows.push(occurrence(single, depth));
        return;
    }
    let mut columns: Vec<_> = found.iter().map(|d| &d.location.column).collect();
    columns.sort();
    columns.dedup();
    let open = expanded.contains(&id);
    rows.push(Row {
        column: match columns.as_slice() {
            [Some(column)] => column.clone(),
            _ => format!("{} columns", columns.len()).into(),
        },
        row_label: SharedString::default(),
        message,
        group: Some(id.clone()),
        count: found.len(),
        members: cluster.to_vec(),
        ..occurrence(found[0], depth)
    });
    if !open {
        return;
    }
    let forms: Vec<_> = found.chunk_by(|a, b| subject(a) == subject(b)).collect();
    for form in &forms {
        if forms.len() == 1 {
            rows.extend(form.iter().map(|d| occurrence(d, depth + 1)));
        } else {
            let form_id = format!("{id}{FORM_SEPARATOR}{:?}", subject(form[0]));
            let message = form[0].message.clone();
            push_group(rows, form, cluster, form_id, message, depth + 1, expanded);
        }
    }
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
    let mut rows = Vec::new();
    for d in items {
        let Some(id) = group_id(d) else {
            rows.push(occurrence(d, 0));
            continue;
        };
        let Some(mut members) = groups.remove(&id) else {
            continue;
        };
        members.sort_by_cached_key(|d| {
            (
                subject(d),
                d.message.clone(),
                d.location.row,
                d.location.column.clone(),
            )
        });
        let cluster: Vec<_> = members
            .iter()
            .map(|member| RowMember {
                location: member.location.clone(),
                message: member.message.clone(),
                subject: subject(member),
            })
            .collect();
        let summary = d.group.as_ref().expect("group metadata").summary.clone();
        push_group(&mut rows, &members, &cluster, id, summary, 0, expanded);
    }
    rows
}

/// Bottom dock: every open problem, filtered by severity and source, click to jump to it.
pub struct ProblemsPanel {
    focus_handle: FocusHandle,
    filter: Filter,
    /// Sources unchecked in the multi-select filter. Empty means all diagnostic sources.
    excluded_sources: BTreeSet<SharedString>,
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
    show_ignored: bool,
    ignored_count: usize,
    /// Refreshes on any store change. One `observe_global` and no re-binding — unlike
    /// `TableStateHandle`, the `Diagnostics` global is plain data that is never rebuilt.
    _sub: Subscription,
}

impl ProblemsPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            filter: Filter::All,
            excluded_sources: BTreeSet::new(),
            sources: Rc::default(),
            rows: Rc::default(),
            counts: [0; 4],
            expanded: BTreeSet::new(),
            show_ignored: false,
            ignored_count: 0,
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
        !self.excluded_sources.contains(&d.source.label())
    }

    fn source_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sources = self.sources.clone();
        let excluded = self.excluded_sources.clone();
        let panel = cx.entity();
        let state = window.use_keyed_state("problems-source-filter", cx, |window, cx| {
            ComboboxState::new(
                CheckList::<SourceItem>::new(Vec::new()),
                Vec::new(),
                window,
                cx,
            )
            .multiple(true)
            .searchable(false)
        });
        window.use_keyed_state("problems-source-sub", cx, |_window, cx| {
            SourceFilterSub(cx.subscribe(&state, move |_, _, event, cx| {
                let ComboboxEvent::Change(kept) = event else {
                    return;
                };
                panel.update(cx, |this, cx| {
                    this.excluded_sources = this
                        .sources
                        .iter()
                        .filter(|source| !kept.contains(source))
                        .cloned()
                        .collect();
                    this.refresh(cx);
                    cx.notify();
                });
            }))
        });
        let cached = window.use_keyed_state("problems-source-items", cx, |_, _| Vec::new());
        if cached.read(cx).as_slice() != sources.as_slice() {
            state.update(cx, |state, cx| {
                state.set_items(
                    CheckList::new(sources.iter().cloned().map(SourceItem).collect()),
                    window,
                    cx,
                );
            });
            cached.update(cx, |cached, _| *cached = sources.to_vec());
        }
        let kept: Vec<_> = sources
            .iter()
            .filter(|source| !excluded.contains(*source))
            .cloned()
            .collect();
        let selected = state.read(cx).selected_values();
        if selected.len() != kept.len() || !kept.iter().all(|source| selected.contains(source)) {
            state.update(cx, |state, cx| {
                state.set_selected_indices(
                    sources
                        .iter()
                        .enumerate()
                        .filter_map(|(ix, source)| {
                            (!excluded.contains(source)).then_some(IndexPath::new(ix))
                        })
                        .collect::<Vec<_>>(),
                    window,
                    cx,
                );
            });
        }
        div().w_48().pr_1().when(sources.len() > 1, |element| {
            element.child(Combobox::new(&state).placeholder("Filter sources").small())
        })
    }

    fn refresh(&mut self, cx: &App) {
        let started = std::time::Instant::now();
        let mut sources: Vec<SharedString> = Diagnostics::all(cx)
            .iter()
            .filter(|d| d.source != Source::Note)
            .map(|d| d.source.label())
            .collect();
        sources.sort();
        sources.dedup();
        self.excluded_sources
            .retain(|source| sources.contains(source));
        self.sources = Rc::new(sources);

        self.counts = Filter::ALL.map(|f| {
            Diagnostics::all(cx)
                .iter()
                .filter(|d| f.admits(d) && self.admits_source(d))
                .count()
        });

        let ignored = Diagnostics::ignored(cx);
        self.ignored_count = ignored.len();
        self.show_ignored &= self.ignored_count > 0;
        let live: BTreeSet<_> = Diagnostics::all(cx)
            .iter()
            .chain(ignored)
            .filter_map(group_id)
            .collect();
        self.expanded
            .retain(|id| live.contains(id.split(FORM_SEPARATOR).next().unwrap_or_default()));
        let admitted = |d: &&Diagnostic| self.filter.admits(d) && self.admits_source(d);
        let visible = Diagnostics::all(cx).iter().filter(admitted).collect();
        let mut rows = project(visible, &self.expanded);
        if self.show_ignored {
            rows.extend(
                project(ignored.iter().filter(admitted).collect(), &self.expanded)
                    .into_iter()
                    .map(|row| Row {
                        ignored: true,
                        ..row
                    }),
            );
        }
        log::debug!(
            "problems panel: {} rows from {} findings in {:?}",
            rows.len(),
            Diagnostics::all(cx).len(),
            started.elapsed()
        );
        self.rows = Rc::new(rows);
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .when(self.filter != Filter::Notes, |bar| {
                        bar.child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .when(self.ignored_count > 0, |bar| {
                                    bar.child(
                                        Button::new("problems-show-ignored")
                                            .ghost()
                                            .small()
                                            .selected(self.show_ignored)
                                            .label(format!("Show ignored ({})", self.ignored_count))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.show_ignored = !this.show_ignored;
                                                this.refresh(cx);
                                                cx.notify();
                                            })),
                                    )
                                })
                                .child(self.source_menu(window, cx)),
                        )
                    }),
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
                        let (guide, nested_bg) =
                            (cx.theme().border, cx.theme().muted.opacity(0.35));
                        rows[range.clone()]
                            .iter()
                            .zip(range)
                            .map(|(r, ix)| {
                                let location = r.location.clone();
                                let group = r.group.clone();
                                let group_members = r.members.clone();
                                let source_key = r.source_key.clone();
                                let finding = r.finding.clone();
                                let ignored = r.ignored;
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
                                    .when(r.depth > 0, |row| row.bg(nested_bg))
                                    .cursor_pointer()
                                    .when(ignored, |row| row.opacity(0.6))
                                    .hover(|row| row.bg(hover_bg))
                                    .child(
                                        Icon::new(r.icon.clone())
                                            .small()
                                            .text_color(severity_color(r.severity, cx)),
                                    )
                                    .child(
                                        h_flex()
                                            .w(px(104.))
                                            .flex_shrink_0()
                                            .h_full()
                                            .children((0..r.depth).map(|_| {
                                                div()
                                                    .w(px(12.))
                                                    .h_full()
                                                    .border_l_1()
                                                    .border_color(guide)
                                            }))
                                            .text_sm()
                                            .text_color(muted)
                                            .when(!is_group, |cell| cell.child(r.row_label.clone()))
                                            .when_some(r.group.clone(), |cell, id| {
                                                let panel_handle = panel_handle.clone();
                                                cell.child(
                                                    Button::new(("expand-problem", ix))
                                                        .text()
                                                        .small()
                                                        .icon(if is_expanded {
                                                            IconName::ChevronDown
                                                        } else {
                                                            IconName::ChevronRight
                                                        })
                                                        .label(format!("({})", r.count))
                                                        .accessibility_label(format!(
                                                            "{} {}",
                                                            if is_expanded {
                                                                "Collapse"
                                                            } else {
                                                                "Expand"
                                                            },
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
                                            .w(px(160.))
                                            .flex_shrink_0()
                                            .text_sm()
                                            .text_color(muted)
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(r.column.clone())
                                            .id(("problem-column", ix))
                                            .tooltip({
                                                let text = r.column.clone();
                                                move |window, cx| {
                                                    gpui_component::tooltip::Tooltip::new(
                                                        text.clone(),
                                                    )
                                                    .build(window, cx)
                                                }
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(r.message.clone())
                                            // ponytail: always offered, since gpui cannot tell us whether the text was cut
                                            .id(("problem-message", ix))
                                            .tooltip({
                                                let text = r.message.clone();
                                                move |window, cx| {
                                                    gpui_component::tooltip::Tooltip::new(
                                                        text.clone(),
                                                    )
                                                    .build(window, cx)
                                                }
                                            }),
                                    )
                                    .when(ignored, |row| {
                                        row.child(
                                            div()
                                                .flex_shrink_0()
                                                .px_1()
                                                .rounded_sm()
                                                .bg(chip_bg)
                                                .text_xs()
                                                .text_color(muted)
                                                .child("Ignored"),
                                        )
                                    })
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
                                        let Some(hooks) =
                                            cx.try_global::<DiagnosticHooks>().copied()
                                        else {
                                            return menu;
                                        };
                                        let findings: Vec<Diagnostic> = if is_group {
                                            group_members
                                                .iter()
                                                .map(|member| Diagnostic {
                                                    location: member.location.clone(),
                                                    message: member.message.clone(),
                                                    ..finding.clone()
                                                })
                                                .collect()
                                        } else {
                                            vec![finding.clone()]
                                        };
                                        if ignored {
                                            return menu.item(
                                                PopupMenuItem::new("Unignore").on_click(
                                                    move |_, _, cx| {
                                                        crate::fixes::unignore(&findings, cx)
                                                    },
                                                ),
                                            );
                                        }
                                        if !is_group {
                                            let text =
                                                matches!(location.scope(), Scope::Cell { .. })
                                                    .then(|| (hooks.text_at)(&location, cx))
                                                    .flatten();
                                            let applied = location.clone();
                                            return crate::fixes::finding_menu(
                                                &finding,
                                                text.as_ref(),
                                                menu,
                                                cx,
                                                move |fixed, cx| {
                                                    (hooks.set_text)(&applied, fixed, cx)
                                                },
                                            );
                                        }
                                        let menu = {
                                            let members = group_members
                                                .iter()
                                                .filter_map(|member| {
                                                    Some(crate::GroupMember {
                                                        location: member.location.clone(),
                                                        text: (hooks.text_at)(
                                                            &member.location,
                                                            cx,
                                                        )?,
                                                        message: member.message.clone(),
                                                        subject: member.subject.clone(),
                                                    })
                                                })
                                                .collect::<Vec<_>>();
                                            crate::fixes::group_menu(
                                                &source_key,
                                                &members,
                                                menu,
                                                window,
                                                cx,
                                            )
                                        };
                                        // Value variants already persist "these are distinct", the precise form of ignore.
                                        if source_key == "value variants" {
                                            return menu;
                                        }
                                        let mut columns: BTreeMap<SharedString, Vec<Diagnostic>> =
                                            BTreeMap::new();
                                        for member in findings {
                                            if let Some(column) = member.location.column.clone() {
                                                columns.entry(column).or_default().push(member);
                                            }
                                        }
                                        let firsts: Vec<Diagnostic> = columns
                                            .values()
                                            .map(|found| found[0].clone())
                                            .collect();
                                        let total: usize = columns.values().map(Vec::len).sum();
                                        let menu = menu.item(
                                            PopupMenuItem::new(format!("Ignore all ({total})"))
                                                .on_click(move |_, window, cx| {
                                                    crate::fixes::ignore(&firsts, false, window, cx)
                                                }),
                                        );
                                        if columns.len() < 2 {
                                            return menu;
                                        }
                                        menu.submenu(
                                            "Ignore in column",
                                            window,
                                            cx,
                                            move |sub, _, _| {
                                                columns.iter().fold(
                                                    sub.max_w(px(360.)),
                                                    |sub, (column, members)| {
                                                        let members = members.clone();
                                                        sub.item(
                                                            PopupMenuItem::new(
                                                                crate::fixes::menu_label(&format!(
                                                                    "“{column}” ({})",
                                                                    members.len()
                                                                )),
                                                            )
                                                            .on_click(move |_, window, cx| {
                                                                crate::fixes::ignore(
                                                                    &members[..1],
                                                                    false,
                                                                    window,
                                                                    cx,
                                                                )
                                                            }),
                                                        )
                                                    },
                                                )
                                            },
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
                    subject: Some("recieve".into()),
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
        assert_eq!((closed[0].count, closed[0].column.as_ref()), (100, "Title"));
        let expanded = [closed[0].group.clone().unwrap()].into_iter().collect();
        let open = super::project(items.iter().collect(), &expanded);
        assert_eq!(open.len(), 101);
        for (row, occurrence) in open[1..].iter().enumerate() {
            assert_eq!(occurrence.location.row, Some(row));
            assert_eq!(occurrence.depth, 1);
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

    #[test]
    fn one_occurrence_is_a_plain_finding() {
        let items = repeated();
        let rows = super::project(vec![&items[0]], &Default::default());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].group.is_none());
        assert_eq!(
            (rows[0].row_label.as_ref(), rows[0].column.as_ref()),
            ("Row 1", "Title")
        );
    }

    #[test]
    fn expanded_variants_nest_forms_and_a_single_form_is_a_plain_row() {
        let mut items = repeated();
        items.truncate(5);
        for (row, item) in items.iter_mut().enumerate() {
            let form = if row < 3 {
                "Akbar, Mohamed"
            } else if row == 3 {
                "Akbar, Mohammed"
            } else {
                "Akbar, Muhammad"
            };
            item.message = form.into();
            item.group.as_mut().unwrap().subject = Some(form.into());
        }
        items[4].location.column = Some("Creator".into());
        let closed = super::project(items.iter().collect(), &Default::default());
        assert_eq!(
            (closed[0].count, closed[0].column.as_ref()),
            (5, "2 columns")
        );
        let group = closed[0].group.clone().unwrap();
        let summarize = |rows: &[super::Row]| {
            rows.iter()
                .map(|row| {
                    (
                        row.depth,
                        row.group.is_some(),
                        row.count,
                        row.row_label.to_string(),
                        row.message.to_string(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let open = super::project(items.iter().collect(), &[group.clone()].into());
        assert_eq!(
            summarize(&open[1..]),
            [
                (1, true, 3, "".into(), "Akbar, Mohamed".into()),
                (1, false, 1, "Row 4".into(), "Akbar, Mohammed".into()),
                (1, false, 1, "Row 5".into(), "Akbar, Muhammad".into()),
            ]
        );
        assert_eq!(
            open[1].members.len(),
            5,
            "a form still resolves the whole cluster"
        );
        let form = open[1].group.clone().unwrap();
        let nested = super::project(items.iter().collect(), &[group, form].into());
        assert_eq!(
            summarize(&nested[2..5]),
            [
                (2, false, 1, "Row 1".into(), "Akbar, Mohamed".into()),
                (2, false, 1, "Row 2".into(), "Akbar, Mohamed".into()),
                (2, false, 1, "Row 3".into(), "Akbar, Mohamed".into()),
            ]
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
                set_texts: |_, _| panic!("navigation must not edit"),
                revalidate: |_| panic!("navigation must not revalidate"),
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
    fn notes_are_separate_from_computed_diagnostics() {
        for severity in [Severity::Error, Severity::Warning, Severity::Note] {
            let computed = Diagnostic {
                location: Location::dataset(DATASET_MAIN),
                severity,
                source: Source::Validator("test".into()),
                message: "computed".into(),
                group: None,
                filed: None,
            };
            assert!(Filter::All.admits(&computed));
            assert!(!Filter::Notes.admits(&computed));

            let note = Diagnostic {
                source: Source::Note,
                ..computed
            };
            assert!(!Filter::All.admits(&note));
            assert!(Filter::Notes.admits(&note));
        }
    }

    #[test]
    fn source_labels_are_descriptive_and_notes_are_not_diagnostic_sources() {
        for (key, label) in [
            ("spell", "Spelling"),
            ("files", "Missing files"),
            ("date", "Date format"),
            ("value variants", "Value variants"),
        ] {
            let source = Source::Validator(key.into());
            assert_eq!(source.key(), key);
            assert_eq!(source.label(), label);
        }
        assert_eq!(Source::Note.key(), "note");
        assert_eq!(Source::Note.label(), "User notes");
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
            Diagnostics::set(
                &Source::Note,
                DATASET_MAIN,
                vec![Diagnostic {
                    location: at(3),
                    severity: Severity::Note,
                    source: Source::Note,
                    message: "user note".into(),
                    group: None,
                    filed: None,
                }],
                cx,
            );
        });

        let (panel, cx) = cx.add_window_view(ProblemsPanel::new);
        panel.update(cx, |this, cx| {
            assert_eq!(this.sources.len(), 2, "both producers are offered");
            assert_eq!(this.rows.len(), 3);
            assert_eq!(this.counts[0], 3, "the All tab excludes user notes");
            assert_eq!(this.counts[3], 1, "the Notes tab counts user notes");

            this.excluded_sources.insert("Spelling".into());
            this.refresh(cx);
            assert_eq!(this.rows.len(), 1, "the files finding survives");
            assert_eq!(this.counts[0], 1, "and the tab count agrees");
            assert_eq!(this.counts[3], 1, "source filters do not hide user notes");

            this.excluded_sources.clear();
            this.refresh(cx);
            assert_eq!(this.rows.len(), 3, "clearing the filter brings them back");
        });
    }
}

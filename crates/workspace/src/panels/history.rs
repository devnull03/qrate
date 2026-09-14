use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::dock::{BasePanel, DockPlacement, Panel, PanelEvent};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use settings::history::{Change, Entry, EntryId, Listed, Origin, ShowCellHistory};
use settings::project::{CurrentProject, RowId};
use table::{TableChanged, TableStateHandle};

use crate::BottomDockCrop;
use crate::panel_registry::PanelMeta;

pub static HISTORY_META: PanelMeta = PanelMeta {
    name: "HistoryPanel",
    icon: "icons/history.svg",
    label: "History",
    default_placement: DockPlacement::Right,
    badge: false,
};

/// How many saved entries one "Load older" brings in.
const PAGE: i64 = 200;

/// Consecutive edits closer together than this, by the same person and the same means, read as
/// one burst of work and collapse under one row.
const BURST_SECS: i64 = 5 * 60;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Cells,
    Structure,
    Notes,
    Fixes,
}

impl Filter {
    const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Cells,
        Filter::Structure,
        Filter::Notes,
        Filter::Fixes,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All changes",
            Filter::Cells => "Cell edits",
            Filter::Structure => "Rows and columns",
            Filter::Notes => "Notes",
            Filter::Fixes => "Fixes",
        }
    }

    fn admits(self, entry: &Entry) -> bool {
        let any = |f: fn(&Change) -> bool| entry.changes.iter().any(f);
        match self {
            Filter::All => true,
            Filter::Cells => any(|c| matches!(c, Change::Cell { .. })),
            Filter::Structure => any(|c| !matches!(c, Change::Cell { .. } | Change::Note { .. })),
            Filter::Notes => any(|c| matches!(c, Change::Note { .. })),
            Filter::Fixes => matches!(entry.origin, Origin::Fix(_) | Origin::Spelling),
        }
    }
}

/// Every change to the project's data and notes, newest first, with a way back to any of them.
///
/// Saved entries are read from the `.qrate` file a page at a time; entries not saved yet come
/// straight from the table and sit on top, where there is nothing to restore to yet.
pub struct HistoryPanel {
    focus_handle: FocusHandle,
    scroll: ScrollHandle,
    saved: Vec<Listed>,
    /// Whether a "Load older" would find anything.
    more: bool,
    /// The file and modification time `saved` was read at. Everything that can add an entry
    /// rewrites the file, so an unchanged stamp means there is nothing new to read.
    read_at: Option<(PathBuf, SystemTime)>,
    filter: Filter,
    named_only: bool,
    /// Bursts opened up, by the id of their newest entry.
    expanded: HashSet<EntryId>,
    /// The entry being named, and the box the name is typed in.
    naming: Option<(EntryId, Entity<InputState>, Subscription)>,
    /// One cell to show the changes of, as its row and every name its column has had.
    cell: Option<(RowId, Vec<String>)>,
    _cell_sub: Subscription,
    _handle_sub: Subscription,
    _table_sub: Option<Subscription>,
    _notes_sub: Subscription,
    _crop_sub: Subscription,
}

impl HistoryPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            saved: Vec::new(),
            more: false,
            read_at: None,
            filter: Filter::All,
            named_only: false,
            expanded: HashSet::new(),
            naming: None,
            cell: None,
            _cell_sub: cx.observe_global::<ShowCellHistory>(|this: &mut Self, cx| {
                this.show_cell(cx);
            }),
            _handle_sub: cx.observe_global::<TableStateHandle>(|this: &mut Self, cx| {
                this.bind(cx);
                cx.notify();
            }),
            _table_sub: None,
            // Notes are written the moment they change, without the table hearing about it.
            _notes_sub: cx.observe_global::<diagnostics::Diagnostics>(|this: &mut Self, cx| {
                this.reload(false, cx);
            }),
            _crop_sub: cx.observe_global::<BottomDockCrop>(|_this: &mut Self, cx| cx.notify()),
        };
        this.bind(cx);
        this
    }

    fn bind(&mut self, cx: &mut Context<Self>) {
        self._table_sub = cx
            .try_global::<TableStateHandle>()
            .and_then(|h| h.0.upgrade())
            .map(|entity| {
                cx.subscribe(&entity, |this, _state, _event: &TableChanged, cx| {
                    this.reload(false, cx);
                    cx.notify();
                })
            });
        self.reload(false, cx);
    }

    /// Re-read the saved history if the project file has changed since it was last read. `force`
    /// re-reads regardless, for this panel's own writes.
    fn reload(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(file) = cx.try_global::<CurrentProject>().map(|p| p.file.clone()) else {
            self.saved.clear();
            self.read_at = None;
            return;
        };
        let stamp = std::fs::metadata(&file)
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        let fresh = Some((file.clone(), stamp));
        if !force && self.read_at == fresh {
            return;
        }
        let limit = PAGE.max(self.saved.len() as i64);
        let row = self.cell.as_ref().map(|(row, _)| *row);
        match settings::history::page(&file, EntryId::MAX, limit, row) {
            Ok(page) => {
                self.more = page.len() as i64 == limit;
                self.saved = page;
            }
            Err(err) => log::error!("couldn't read the project history: {err}"),
        }
        self.read_at = fresh;
        cx.notify();
    }

    /// Narrow the list to the cell the grid asked about. Its column's former names come from both
    /// halves of the log, unsaved renames being the newest.
    fn show_cell(&mut self, cx: &mut Context<Self>) {
        let (Some(request), Some(file)) = (
            cx.try_global::<ShowCellHistory>(),
            cx.try_global::<CurrentProject>().map(|p| p.file.clone()),
        ) else {
            return;
        };
        let unsaved: Vec<(String, String)> = cx
            .try_global::<TableStateHandle>()
            .and_then(|h| h.0.upgrade())
            .map(|state| {
                state
                    .read(cx)
                    .delegate()
                    .unsaved_history()
                    .iter()
                    .rev()
                    .flat_map(|e| e.changes.iter().rev())
                    .filter_map(|c| match c {
                        Change::ColumnRenamed { before, after } => {
                            Some((before.clone(), after.clone()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let saved = settings::history::renames(&file).unwrap_or_else(|err| {
            log::error!("couldn't read column renames from the project history: {err}");
            Vec::new()
        });
        let names = settings::history::former_names(
            &request.column,
            unsaved
                .iter()
                .chain(&saved)
                .map(|(b, a)| (b.as_str(), a.as_str())),
        );
        self.cell = Some((request.row, names));
        self.filter = Filter::All;
        self.named_only = false;
        self.saved.clear();
        self.reload(true, cx);
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        let (Some(file), Some(last)) = (
            cx.try_global::<CurrentProject>().map(|p| p.file.clone()),
            self.saved.last().map(|l| l.entry.id),
        ) else {
            return;
        };
        let row = self.cell.as_ref().map(|(row, _)| *row);
        match settings::history::page(&file, last, PAGE, row) {
            Ok(page) => {
                self.more = page.len() as i64 == PAGE;
                self.saved.extend(page);
            }
            Err(err) => log::error!("couldn't read older project history: {err}"),
        }
        cx.notify();
    }

    fn start_naming(&mut self, id: EntryId, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .saved
            .iter()
            .find(|l| l.entry.id == id)
            .and_then(|l| l.label.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Name this version")
                .default_value(current)
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, _window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let name = input.read(cx).value().trim().to_string();
                    if let Some((id, _, _)) = this.naming.take() {
                        this.name(id, (!name.is_empty()).then_some(name.as_str()), cx);
                    }
                }
                InputEvent::Blur => {
                    this.naming = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        self.naming = Some((id, input, sub));
        cx.notify();
    }

    fn name(&mut self, id: EntryId, label: Option<&str>, cx: &mut Context<Self>) {
        if let Some(file) = cx.try_global::<CurrentProject>().map(|p| p.file.clone())
            && let Err(err) = settings::history::set_label(&file, id, label)
        {
            log::error!("couldn't name history entry #{id}: {err}");
        }
        self.reload(true, cx);
    }

    /// Ask before putting the whole project back, saying how much that reverses.
    fn confirm_restore(listed: &Listed, window: &mut Window, cx: &mut App) {
        let Some(file) = cx.try_global::<CurrentProject>().map(|p| p.file.clone()) else {
            return;
        };
        let since = settings::history::entries_after(&file, listed.entry.id)
            .map(|entries| entries.iter().map(|e| e.changes.len()).sum::<usize>())
            .unwrap_or_default()
            + cx.try_global::<TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .map_or(0, |state| {
                    let delegate = state.read(cx).delegate();
                    delegate
                        .unsaved_history()
                        .iter()
                        .map(|e| e.changes.len())
                        .sum()
                });
        let id = listed.entry.id;
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Restore the project to {} at {}?",
                day_label(&listed.day, listed.days_ago),
                listed.time
            ),
            Some(&format!(
                "{since} change{} made since will be reversed. The restore is added to the history, and Undo takes it back.",
                if since == 1 { "" } else { "s" }
            )),
            &["Restore", "Cancel"],
            cx,
        );
        cx.spawn(async move |cx| {
            if answer.await == Ok(0) {
                cx.update(|cx| table::restore_to(id, cx));
            }
        })
        .detach();
    }

    fn confirm_clear(panel: WeakEntity<Self>, window: &mut Window, cx: &mut App) {
        let answer = window.prompt(
            PromptLevel::Critical,
            "Clear this project's history?",
            Some("Every recorded change is forgotten and nothing before now can be restored. The data itself is not changed."),
            &["Clear History", "Cancel"],
            cx,
        );
        cx.spawn(async move |cx| {
            if answer.await != Ok(0) {
                return;
            }
            cx.update(|cx| {
                if let Some(file) = cx.try_global::<CurrentProject>().map(|p| p.file.clone())
                    && let Err(err) = settings::history::clear(&file)
                {
                    log::error!("couldn't clear the project history: {err}");
                }
                panel.update(cx, |this, cx| this.reload(true, cx)).ok();
            });
        })
        .detach();
    }
}

/// `entry` cut down to what it did to one cell — its value, its notes, its row coming or going — or
/// `None` when it did nothing there. Borrowed untouched when there is no cell to narrow to.
fn narrowed<'a>(entry: &'a Entry, cell: Option<&(RowId, Vec<String>)>) -> Option<Cow<'a, Entry>> {
    let Some((row, names)) = cell else {
        return Some(Cow::Borrowed(entry));
    };
    let named = |column: &str| names.iter().any(|name| name == column);
    let changes: Vec<Change> = entry
        .changes
        .iter()
        .filter(|change| match change {
            Change::Cell { row: r, column, .. } => r == row && named(column),
            Change::Note { row: r, column, .. } => {
                *r == Some(*row) && column.as_deref().is_none_or(named)
            }
            Change::RowAdded { row: r, .. } | Change::RowRemoved { row: r, .. } => r == row,
            _ => false,
        })
        .cloned()
        .collect();
    (!changes.is_empty()).then(|| {
        Cow::Owned(Entry {
            changes,
            ..entry.clone()
        })
    })
}

/// `3 Sep 2026`, from a `YYYY-MM-DD` day.
fn date_label(day: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = day.split('-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d)) => {
            let month = m
                .parse::<usize>()
                .ok()
                .and_then(|m| MONTHS.get(m.checked_sub(1)?));
            format!("{} {} {y}", d.trim_start_matches('0'), month.unwrap_or(&m))
        }
        _ => day.into(),
    }
}

/// What a day's header says: `Today`, `Yesterday`, or the date.
fn day_label(day: &str, days_ago: i64) -> String {
    match days_ago {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => date_label(day),
    }
}

/// When one change was made: `3 Sep 2026, 14:02`.
pub(crate) fn when(day: &str, time: &str) -> String {
    format!("{}, {time}", date_label(day))
}

/// One line saying what an entry did, in the grid's current terms: rows by the number the `#`
/// column shows now, and a row that has since gone as exactly that.
fn describe(entry: &Entry, rows: &HashMap<RowId, usize>) -> String {
    let row = |id: &RowId| {
        rows.get(id)
            .map_or_else(|| "a deleted row".to_string(), |p| format!("row {}", p + 1))
    };
    let quote = |text: &str| match text.chars().count() {
        0 => "(empty)".to_string(),
        n if n > 40 => format!("“{}…”", text.chars().take(40).collect::<String>()),
        _ => format!("“{text}”"),
    };
    let count = |n: usize, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let changes = &entry.changes;
    match changes.as_slice() {
        [
            Change::Cell {
                row: id,
                column,
                before,
                after,
            },
        ] => format!(
            "{column}, {}: {} → {}",
            row(id),
            quote(before),
            quote(after)
        ),
        [Change::ColumnAdded { column, .. }] => format!("Added column {}", quote(column)),
        [Change::ColumnRemoved { column, .. }] => format!("Deleted column {}", quote(column)),
        [Change::ColumnRenamed { before, after }] => {
            format!("Renamed column {} to {}", quote(before), quote(after))
        }
        [Change::ColumnMoved { column, .. }] => format!("Moved column {}", quote(column)),
        [
            Change::Note {
                row: id,
                column,
                before,
                after,
            },
        ] => {
            let what = match (before, after) {
                (None, _) => "Added a note",
                (_, None) => "Removed a note",
                _ => "Edited a note",
            };
            match (id, column) {
                (Some(id), Some(column)) => format!("{what} on {column}, {}", row(id)),
                (Some(id), None) => format!("{what} on {}", row(id)),
                (None, Some(column)) => format!("{what} on column {column}"),
                (None, None) => format!("{what} on the project"),
            }
        }
        _ if changes.iter().all(|c| matches!(c, Change::Cell { .. })) => {
            let mut columns: Vec<&str> = Vec::new();
            for change in changes {
                if let Change::Cell { column, .. } = change
                    && !columns.contains(&column.as_str())
                {
                    columns.push(column);
                }
            }
            let named = match columns.len() {
                n if n > 3 => format!("{} and {} more", columns[..3].join(", "), n - 3),
                _ => columns.join(", "),
            };
            format!("{} in {named}", count(changes.len(), "cell", "cells"))
        }
        _ if changes.iter().all(|c| matches!(c, Change::RowAdded { .. })) => {
            format!("Added {}", count(changes.len(), "row", "rows"))
        }
        _ if changes
            .iter()
            .all(|c| matches!(c, Change::RowRemoved { .. })) =>
        {
            format!("Deleted {}", count(changes.len(), "row", "rows"))
        }
        _ => count(changes.len(), "change", "changes"),
    }
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for HistoryPanel {}

impl BasePanel for HistoryPanel {
    fn panel_name(&self) -> &'static str {
        "HistoryPanel"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> bool {
        true
    }
}

impl Panel for HistoryPanel {
    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("History")
    }

    fn title_suffix(
        &mut self,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let panel = cx.entity().downgrade();
        let (filter, named_only) = (self.filter, self.named_only);
        Some(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new("history-filter")
                        .ghost()
                        .xsmall()
                        .label(if named_only {
                            "Named versions"
                        } else {
                            filter.label()
                        })
                        .dropdown_menu({
                            let panel = panel.clone();
                            move |menu: PopupMenu, _window, _cx| {
                                let menu = Filter::ALL.into_iter().fold(menu, |menu, pick| {
                                    let panel = panel.clone();
                                    menu.item(
                                        PopupMenuItem::new(pick.label())
                                            .checked(!named_only && pick == filter)
                                            .on_click(move |_, _, cx| {
                                                panel
                                                    .update(cx, |this, cx| {
                                                        this.filter = pick;
                                                        this.named_only = false;
                                                        cx.notify();
                                                    })
                                                    .ok();
                                            }),
                                    )
                                });
                                let panel = panel.clone();
                                menu.separator().item(
                                    PopupMenuItem::new("Named versions")
                                        .checked(named_only)
                                        .on_click(move |_, _, cx| {
                                            panel
                                                .update(cx, |this, cx| {
                                                    this.named_only = !this.named_only;
                                                    cx.notify();
                                                })
                                                .ok();
                                        }),
                                )
                            }
                        }),
                )
                .child(
                    Button::new("history-more")
                        .icon(IconName::Ellipsis)
                        .ghost()
                        .xsmall()
                        .dropdown_menu(move |menu: PopupMenu, _window, _cx| {
                            let panel = panel.clone();
                            menu.item(PopupMenuItem::new("Clear History…").on_click(
                                move |_, window, cx| Self::confirm_clear(panel.clone(), window, cx),
                            ))
                        }),
                ),
        )
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, hover_bg, accent) = (
            theme.muted_foreground,
            theme.border,
            theme.secondary_hover,
            theme.accent_foreground,
        );
        let crop = cx.try_global::<BottomDockCrop>().map_or(px(0.), |c| c.0);
        let panel = cx.entity().downgrade();

        let (rows, unsaved): (HashMap<RowId, usize>, Vec<Entry>) = cx
            .try_global::<TableStateHandle>()
            .and_then(|h| h.0.upgrade())
            .map(|state| {
                let delegate = state.read(cx).delegate();
                (
                    delegate
                        .row_ids()
                        .iter()
                        .enumerate()
                        .map(|(p, id)| (*id, p))
                        .collect(),
                    delegate.unsaved_history().to_vec(),
                )
            })
            .unwrap_or_default();
        let (filter, named_only) = (self.filter, self.named_only);
        let admits = |entry: &Entry, label: bool| {
            if named_only {
                label
            } else {
                filter.admits(entry)
            }
        };

        let cell = self.cell.as_ref();
        let unsaved: Vec<Entry> = unsaved
            .iter()
            .rev()
            .filter_map(|e| narrowed(e, cell).map(Cow::into_owned))
            .filter(|e| admits(e, false))
            .collect();
        let saved: Vec<Cow<Listed>> = self
            .saved
            .iter()
            .filter_map(|l| match narrowed(&l.entry, cell)? {
                Cow::Borrowed(_) => Some(Cow::Borrowed(l)),
                Cow::Owned(entry) => Some(Cow::Owned(Listed { entry, ..l.clone() })),
            })
            .filter(|l| admits(&l.entry, l.label.is_some()))
            .collect();
        let cell_label = cell.map(|(row, names)| {
            let at = rows
                .get(row)
                .map_or_else(|| "a deleted row".to_string(), |p| format!("row {}", p + 1));
            format!("{}, {at}", names[0])
        });

        // Bursts: runs of adjacent, unnamed entries by one person, one means, one day.
        let mut bursts: Vec<Vec<&Listed>> = Vec::new();
        for listed in saved.iter().map(|l| &**l) {
            let joins = bursts.last().and_then(|b| b.last()).is_some_and(|prev| {
                prev.label.is_none()
                    && listed.label.is_none()
                    && prev.day == listed.day
                    && prev.entry.author == listed.entry.author
                    && prev.entry.origin == listed.entry.origin
                    && matches!(
                        listed.entry.origin,
                        Origin::Typed | Origin::Details | Origin::Paste | Origin::Clear
                    )
                    && prev.entry.at - listed.entry.at <= BURST_SECS
            });
            match (joins, bursts.last_mut()) {
                (true, Some(burst)) => burst.push(listed),
                _ => bursts.push(vec![listed]),
            }
        }

        // ponytail: always offered, since gpui cannot tell us whether the text was cut
        let clipped = |text: String| {
            div()
                .id("text")
                .overflow_hidden()
                .text_ellipsis()
                .child(text.clone())
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
                })
        };
        let entry_row = |id: ElementId,
                         entry: &Entry,
                         listed: Option<&Listed>,
                         meta: String,
                         indent: bool| {
            let dimmed = matches!(entry.origin, Origin::Undo | Origin::Redo);
            let naming = listed.and_then(|l| {
                self.naming
                    .as_ref()
                    .filter(|(id, _, _)| *id == l.entry.id)
                    .map(|(_, input, _)| input.clone())
            });
            let actions = listed.cloned().map(|listed| {
                let panel = panel.clone();
                move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
                    let single = match listed.entry.changes.as_slice() {
                        [
                            Change::Cell {
                                row,
                                column,
                                before,
                                ..
                            },
                        ] => Some((*row, column.clone(), before.clone())),
                        _ => None,
                    };
                    let (restore, name, unname) = (listed.clone(), panel.clone(), panel.clone());
                    let id = listed.entry.id;
                    menu.item(
                        PopupMenuItem::new("Restore Project to Here…").on_click(
                            move |_, window, cx| Self::confirm_restore(&restore, window, cx),
                        ),
                    )
                    .when_some(single, |menu, (row, column, before)| {
                        menu.item(PopupMenuItem::new("Restore This Value").on_click(
                            move |_, _, cx| {
                                table::restore_value(row, &column, before.clone().into(), id, cx)
                            },
                        ))
                    })
                    .separator()
                    .item(PopupMenuItem::new("Name This Version…").on_click(
                        move |_, window, cx| {
                            name.update(cx, |this, cx| this.start_naming(id, window, cx))
                                .ok();
                        },
                    ))
                    .when(listed.label.is_some(), |menu| {
                        menu.item(PopupMenuItem::new("Remove Name").on_click(move |_, _, cx| {
                            unname.update(cx, |this, cx| this.name(id, None, cx)).ok();
                        }))
                    })
                }
            });
            v_flex()
                .id(id)
                .w_full()
                .gap_0p5()
                .px_2()
                .py_1()
                .when(indent, |row| row.pl_6())
                .border_b_1()
                .border_color(border)
                .hover(|row| row.bg(hover_bg))
                .when(dimmed, |row| row.opacity(0.6))
                .when_some(listed.and_then(|l| l.label.clone()), |row, label| {
                    row.child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent)
                            .child(label),
                    )
                })
                .when_some(naming, |row, input| row.child(Input::new(&input).xsmall()))
                .child(clipped(describe(entry, &rows)).text_sm())
                .child(div().text_xs().text_color(muted).child(meta))
                .map(|row| match actions {
                    Some(actions) => row.context_menu(actions).into_any_element(),
                    None => row.into_any_element(),
                })
        };
        let meta = |entry: &Entry, when: String| {
            [Some(entry.origin.label()), entry.author.clone(), Some(when)]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let header = |text: String| {
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(text)
        };

        let empty = unsaved.is_empty() && bursts.is_empty();
        let mut list = v_flex()
            .id("history-list")
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .vertical_scrollbar(&self.scroll)
            .pr_2()
            .pb(px(8.) + crop);

        if empty {
            list = list.child(div().p_3().text_sm().text_color(muted).child(
                match (cx.has_global::<CurrentProject>(), named_only) {
                    (false, _) => "Open a project to see its history.",
                    (true, true) => "No named versions yet. Right-click a change to name it.",
                    (true, false) => "No changes yet. Every edit to this project's data and notes will be listed here.",
                },
            ));
        }
        if !unsaved.is_empty() {
            list = list.child(header("Not saved yet".into()));
            let ats: Vec<i64> = unsaved.iter().map(|e| e.at).collect();
            let times = settings::history::local_times(&ats).unwrap_or_else(|err| {
                log::error!("couldn't read the local time of unsaved changes: {err}");
                Vec::new()
            });
            for (ix, entry) in unsaved.iter().enumerate() {
                let at = times.get(ix).map_or_else(
                    || "not saved".to_string(),
                    |(day, time)| format!("{}, not saved", when(day, time)),
                );
                list = list.child(entry_row(
                    ElementId::NamedInteger("history-unsaved".into(), ix as u64),
                    entry,
                    None,
                    meta(entry, at),
                    false,
                ));
            }
        }
        let mut day = None;
        for burst in &bursts {
            let head = burst[0];
            if day != Some(&head.day) {
                day = Some(&head.day);
                list = list.child(header(day_label(&head.day, head.days_ago)));
            }
            let id = head.entry.id;
            let row_id = |listed: &Listed| {
                ElementId::NamedInteger("history-entry".into(), listed.entry.id as u64)
            };
            if burst.len() == 1 {
                list = list.child(entry_row(
                    row_id(head),
                    &head.entry,
                    Some(head),
                    meta(&head.entry, when(&head.day, &head.time)),
                    false,
                ));
                continue;
            }
            let open = self.expanded.contains(&id);
            let last = burst[burst.len() - 1];
            let changes: Vec<Change> = burst
                .iter()
                .flat_map(|l| l.entry.changes.iter().cloned())
                .collect();
            let summary = Entry {
                changes,
                ..head.entry.clone()
            };
            let toggle = panel.clone();
            list = list.child(
                h_flex()
                    .id(ElementId::NamedInteger("history-burst".into(), id as u64))
                    .w_full()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .hover(|row| row.bg(hover_bg))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        toggle
                            .update(cx, |this, cx| {
                                if !this.expanded.remove(&id) {
                                    this.expanded.insert(id);
                                }
                                cx.notify();
                            })
                            .ok();
                    })
                    .child(
                        gpui_component::Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .text_color(muted),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                clipped(format!(
                                    "{} edits · {}",
                                    burst.len(),
                                    describe(&summary, &rows)
                                ))
                                .text_sm(),
                            )
                            .child(div().text_xs().text_color(muted).child(meta(
                                &head.entry,
                                format!("{}, {}–{}", date_label(&head.day), last.time, head.time),
                            ))),
                    ),
            );
            if open {
                for listed in burst {
                    list = list.child(entry_row(
                        row_id(listed),
                        &listed.entry,
                        Some(listed),
                        meta(&listed.entry, when(&listed.day, &listed.time)),
                        true,
                    ));
                }
            }
        }
        if self.more && !named_only {
            list = list.child(
                div().p_2().child(
                    Button::new("history-older")
                        .ghost()
                        .small()
                        .label("Load Older Changes")
                        .on_click(cx.listener(|this, _, _, cx| this.load_older(cx))),
                ),
            );
        }

        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .id("history-panel")
            .role(Role::Group)
            .aria_label("History")
            .when_some(cell_label, |panel, label| {
                panel.child(
                    h_flex()
                        .flex_none()
                        .gap_1()
                        .items_center()
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(border)
                        .child(
                            clipped(format!("Changes to {label}"))
                                .flex_1()
                                .min_w_0()
                                .text_xs(),
                        )
                        .child(
                            Button::new("history-cell-clear")
                                .icon(IconName::Close)
                                .ghost()
                                .xsmall()
                                .tooltip("Show every change")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cell = None;
                                    this.saved.clear();
                                    this.reload(true, cx);
                                })),
                        ),
                )
            })
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here: the parent has `use gpui::*` in scope.
    use super::{day_label, describe, narrowed};
    use settings::history::{Change, Entry, Origin};
    use std::collections::HashMap;

    /// Narrowed to one cell, an entry keeps that cell's edits under any name its column has had,
    /// its row's notes and its row coming or going — and nothing about the cells beside it.
    #[test]
    fn a_cell_keeps_its_own_changes_under_any_former_name() {
        let cell = |row, column: &str| Change::Cell {
            row,
            column: column.into(),
            before: "a".into(),
            after: "b".into(),
        };
        let entry = Entry::new(
            Origin::Paste,
            vec![cell(1, "Name"), cell(1, "Date"), cell(2, "Name")],
            None,
        );
        let names = (1, vec!["Title".to_string(), "Name".to_string()]);
        let kept = narrowed(&entry, Some(&names)).unwrap();
        assert_eq!(kept.changes, vec![cell(1, "Name")]);
        let elsewhere = Entry::new(Origin::Typed, vec![cell(2, "Date")], None);
        assert!(narrowed(&elsewhere, Some(&names)).is_none());
    }

    #[test]
    fn days_read_as_a_person_would_say_them() {
        assert_eq!(day_label("2026-09-13", 0), "Today");
        assert_eq!(day_label("2026-09-12", 1), "Yesterday");
        assert_eq!(day_label("2026-09-03", 10), "3 Sep 2026");
        assert_eq!(super::when("2026-09-03", "14:02"), "3 Sep 2026, 14:02");
    }

    /// A row is named by the number the grid shows for it now, not by where it was when edited.
    #[test]
    fn a_cell_edit_names_its_row_as_the_grid_numbers_it_now() {
        let cell = |row, before: &str, after: &str| Change::Cell {
            row,
            column: "Title".into(),
            before: before.into(),
            after: after.into(),
        };
        let rows = HashMap::from([(7, 2)]);
        let one = Entry::new(Origin::Typed, vec![cell(7, "", "Harbour")], None);
        assert_eq!(describe(&one, &rows), "Title, row 3: (empty) → “Harbour”");
        let gone = Entry::new(Origin::Typed, vec![cell(9, "a", "b")], None);
        assert_eq!(describe(&gone, &rows), "Title, a deleted row: “a” → “b”");
        let many = Entry::new(
            Origin::Paste,
            vec![cell(7, "a", "b"), cell(9, "c", "d")],
            None,
        );
        assert_eq!(describe(&many, &rows), "2 cells in Title");
    }
}

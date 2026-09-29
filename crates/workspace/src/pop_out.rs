//! The pop-out viewer: a second window that shows the selected row's file at full size beside its
//! fields. It follows the grid's selection, so on two monitors the table sits on one screen and the
//! scan fills the other — unless it is pinned, when it holds its item while the grid moves on.
//!
//! The stage is the full-screen viewer itself, mounted in [`Scope::PopOut`], and the sidebar is a
//! [`DetailsPanel`] describing whatever this window shows. Neither knows about the other: this
//! window decides which rows they are both about.

use std::ops::Range;
use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Root, Selectable as _, Sizable, StyledExt as _,
    TitleBar,
    button::{Button, ButtonVariants},
    h_flex,
    resizable::{ResizableState, h_resizable, resizable_panel},
    table::TableState,
    v_flex,
};
use settings::MainWindowBounds;
use settings::project::{CurrentProject, RowId};
use table::{QrateTableDelegate, TableChanged, TablePanelHandle, TableStateHandle};

use crate::panels::DetailsPanel;
use crate::viewer::{self, Scope, Viewer};

/// How wide the sidebar opens, and how far it may be dragged either way — the same range as the
/// viewer's find panel, which it replaces for a document.
const SIDEBAR: Pixels = px(320.);
const SIDEBAR_RANGE: Range<Pixels> = px(240.)..px(720.);
/// The smallest window that still fits a readable page beside the sidebar.
const MIN_SIZE: Size<Pixels> = Size {
    width: px(800.),
    height: px(600.),
};
const DEFAULT_SIZE: Size<Pixels> = Size {
    width: px(1120.),
    height: px(700.),
};
/// Height of the sidebar's header strip, the Details title or the Details | Find tabs.
const HEADER_H: Pixels = px(30.);
/// The stage's own text colours. Not theme colours, for the reason the backdrop is not one.
const STAGE_FG: u32 = 0xe8e8e8;
const STAGE_MUTED: u32 = 0xa3a3a3;

/// `.qrate` setting key for the window's last size and display, as a JSON [`MainWindowBounds`] —
/// per project, the same way the main window keeps its own.
const BOUNDS_KEY: &str = "pop_out_window_bounds";

/// The bounds last seen, for the project they belong to. The `.qrate` write is debounced, so a
/// window closed and reopened inside that interval would otherwise read the size it had before.
#[derive(Default)]
struct LastBounds(Option<(PathBuf, MainWindowBounds)>);

impl Global for LastBounds {}

/// The open pop-out window. There is one per project, and one project open at a time.
#[derive(Default)]
pub(crate) struct PopOutWindow(Option<AnyWindowHandle>);

impl Global for PopOutWindow {}

pub fn is_open(cx: &App) -> bool {
    cx.try_global::<PopOutWindow>()
        .is_some_and(|window| window.0.is_some())
}

/// Open the pop-out window, or bring the open one forward.
pub fn open(cx: &mut App) {
    if let Some(handle) = cx.try_global::<PopOutWindow>().and_then(|window| window.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let file = cx.try_global::<CurrentProject>().map(|p| p.file.clone());
    let saved = file.as_ref().and_then(|file| {
        let remembered = cx
            .try_global::<LastBounds>()
            .and_then(|last| last.0.as_ref())
            .filter(|(of, _)| of == file)
            .map(|(_, bounds)| bounds.clone());
        remembered.or_else(|| {
            settings::project::read_setting(file, BOUNDS_KEY)
                .ok()
                .flatten()
                .and_then(|raw| serde_json::from_str::<MainWindowBounds>(&raw).ok())
        })
    });
    // The shared placement's fallback is the main window's portrait default, narrower than this
    // window's minimum; nothing saved means this window's own default.
    let (bounds, display) = match saved {
        Some(saved) => MainWindowBounds::startup_placement(Some(&saved), cx),
        None => (Bounds::centered(None, DEFAULT_SIZE, cx), None),
    };
    let bounds = Bounds::centered(display, bounds.size.max(&MIN_SIZE), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id: display,
        window_min_size: Some(MIN_SIZE),
        ..TitleBar::window_options()
    };
    match cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| PopOut::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        Ok(handle) => cx.set_global(PopOutWindow(Some(handle.into()))),
        Err(err) => log::error!("couldn't open the pop-out viewer window: {err:#}"),
    }
}

/// Close the pop-out window, if one is open. For the main window closing: the table this one
/// follows goes with it.
pub fn close(cx: &mut App) {
    let Some(handle) = cx.try_global::<PopOutWindow>().and_then(|window| window.0) else {
        return;
    };
    cx.set_global(PopOutWindow::default());
    handle
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

pub struct PopOut {
    focus_handle: FocusHandle,
    /// The project this window was opened for. It closes when another one is opened.
    project: Option<PathBuf>,
    state: Option<WeakEntity<TableState<QrateTableDelegate>>>,
    /// The items held while pinned, by id rather than position, so rows added or removed above
    /// them cannot move the pin onto a neighbour. `None` while following the grid.
    pinned: Option<Vec<RowId>>,
    /// What this window shows — the pinned items or the grid's selection — as source rows.
    rows: Vec<usize>,
    /// Which of several items the stage shows. Only the stage steps: the sidebar's shared fields
    /// and the grid's selection both keep all of them.
    stack: usize,
    /// The stack's step arrows only exist under the pointer, so they never cover the page at rest.
    stack_hover: bool,
    /// The front item's file, when there is one to draw.
    viewer: Option<Entity<Viewer>>,
    details: Entity<DetailsPanel>,
    sidebar: bool,
    split: Entity<ResizableState>,
    _table_sub: Option<Subscription>,
    /// Repaints when the viewer's pages, controls or find tab change under it.
    _viewer_sub: Option<Subscription>,
    /// What the OS title bar was last told, so a table edit does not set it again unchanged.
    window_title: String,
    _subs: Vec<Subscription>,
}

impl PopOut {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let details = cx.new(|cx| {
            let mut details = DetailsPanel::new(window, cx);
            details.show_rows(Vec::new(), cx);
            details
        });
        let me = window.window_handle();
        cx.on_release(move |this: &mut Self, cx| {
            // A recording playing here would otherwise go on with nothing on screen to stop it.
            if let Some(viewer) = &this.viewer {
                preview::playback::stop(viewer.entity_id(), cx);
            }
            // Only this window's own entry: a new pop-out may already have replaced it.
            if cx
                .try_global::<PopOutWindow>()
                .is_some_and(|window| window.0 == Some(me))
            {
                cx.set_global(PopOutWindow::default());
            }
        })
        .detach();

        let _subs = vec![
            cx.observe_global_in::<TableStateHandle>(window, |this, window, cx| {
                this.bind(window, cx)
            }),
            cx.observe_global_in::<CurrentProject>(window, |this, window, cx| {
                if cx.try_global::<CurrentProject>().map(|p| &p.file) != this.project.as_ref() {
                    window.remove_window();
                }
            }),
            cx.observe_global::<settings::dirty::Dirty>(|_, cx| cx.notify()),
            // The stage offers to install what a PDF or a video needs; once it lands, draw it.
            cx.observe_global_in::<components::Components>(window, |this, window, cx| {
                let installed = this.viewer.as_ref().is_some_and(|viewer| {
                    let viewer = viewer.read(cx);
                    viewer.needs.is_some() && preview::missing(&viewer.path).is_none()
                });
                if installed {
                    this.viewer = None;
                    this.sync(window, cx);
                }
            }),
            // Debounced by the writer: this fires on every pixel of a drag.
            cx.observe_window_bounds(window, |this, window, cx| {
                let Some(file) = &this.project else {
                    return;
                };
                let bounds = MainWindowBounds::capture_from_window(window, cx);
                if let Ok(json) = serde_json::to_string(&bounds) {
                    settings::project::queue_write(file, BOUNDS_KEY, &json, cx);
                }
                cx.set_global(LastBounds(Some((file.clone(), bounds))));
            }),
        ];

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut this = Self {
            focus_handle,
            project: cx.try_global::<CurrentProject>().map(|p| p.file.clone()),
            state: None,
            pinned: None,
            rows: Vec::new(),
            stack: 0,
            stack_hover: false,
            viewer: None,
            details,
            sidebar: true,
            split: cx.new(|_| ResizableState::default()),
            _table_sub: None,
            _viewer_sub: None,
            window_title: String::new(),
            _subs,
        };
        this.bind(window, cx);
        this
    }

    fn bind(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = cx.try_global::<TableStateHandle>().map(|h| h.0.clone());
        self._table_sub = self.state.as_ref().and_then(|w| w.upgrade()).map(|table| {
            cx.subscribe_in(&table, window, |this, _, _: &TableChanged, window, cx| {
                this.sync(window, cx)
            })
        });
        self.sync(window, cx);
    }

    fn table(&self) -> Option<Entity<TableState<QrateTableDelegate>>> {
        self.state.as_ref().and_then(|w| w.upgrade())
    }

    /// The item the stage shows: the stack's front card, clamped so a smaller selection never
    /// leaves it pointing past the end.
    fn front(&self) -> Option<usize> {
        crate::panels::details::stack_front(&self.rows, self.stack)
    }

    /// Bring the rows, the stage and the sidebar up to date with the grid.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut kept = None;
        let rows = self.table().map_or_else(Vec::new, |state| {
            let delegate = state.read(cx).delegate();
            match &self.pinned {
                // Where the pin was last found, while every row is still there: a cell edit is the
                // common change, and it moves nothing.
                Some(ids)
                    if ids.len() == self.rows.len()
                        && ids
                            .iter()
                            .zip(&self.rows)
                            .all(|(id, &row)| delegate.row_id(row) == Some(*id)) =>
                {
                    self.rows.clone()
                }
                // Rows were added, removed or moved: one pass to find the pin again, dropping the
                // items that are gone so the pass above matches again from the next change.
                Some(ids) => {
                    let at = delegate.row_positions();
                    let (ids, rows): (Vec<_>, Vec<_>) = ids
                        .iter()
                        .filter_map(|id| Some((*id, *at.get(id)?)))
                        .unzip();
                    kept = Some(ids);
                    rows
                }
                None => delegate.selected_source_rows(),
            }
        });
        if kept.is_some() {
            self.pinned = kept;
        }
        // Every pinned item deleted: there is nothing left to hold, so follow again.
        if self.pinned.is_some() && rows.is_empty() {
            self.pinned = None;
            return self.sync(window, cx);
        }
        if rows != self.rows {
            self.stack = 0;
            self.rows = rows.clone();
        }
        self.details
            .update(cx, |details, cx| details.show_rows(rows, cx));

        let file = self
            .front()
            .zip(self.table())
            .and_then(|(row, state)| viewer::previewable(state.read(cx).delegate(), row));
        if self.viewer.as_ref().map(|viewer| &viewer.read(cx).path) != file.as_ref() {
            if let Some(leaving) = &self.viewer {
                preview::playback::stop(leaving.entity_id(), cx);
            }
            self.viewer = file.map(|file| viewer::build(file, Scope::PopOut, window, cx));
            self._viewer_sub = self.viewer.as_ref().map(|viewer| {
                // The viewer repaints itself on every pan and zoom; this window only draws
                // its controls, tabs and find results.
                let mut seen = None;
                cx.observe(viewer, move |_, viewer, cx| {
                    let viewer = viewer.read(cx);
                    let now = (viewer.has_controls(), viewer.document, viewer.find_open);
                    if viewer.find_open || seen != Some(now) {
                        seen = Some(now);
                        cx.notify();
                    }
                })
            });
        }
        let (file, rest) = self.title(cx);
        let title = format!("{file}{rest}");
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        cx.notify();
    }

    /// The row ids of `rows`, which is what a pin holds on to.
    fn ids(&self, rows: &[usize], cx: &App) -> Vec<RowId> {
        self.table().map_or_else(Vec::new, |state| {
            let delegate = state.read(cx).delegate();
            rows.iter()
                .filter_map(|&row| delegate.row_id(row))
                .collect()
        })
    }

    /// Step to the neighbouring row with a file to show. Following, that moves the grid, and this
    /// window comes along; pinned, it moves only this window.
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pinned.is_none() {
            viewer::step_row(delta, cx);
            return;
        }
        let Some(state) = self.table() else {
            return;
        };
        let target = {
            let delegate = state.read(cx).delegate();
            self.front()
                .and_then(|row| viewer::next_previewable(delegate, row, delta))
                .and_then(|view| delegate.row_id(delegate.visible()[view]))
        };
        if let Some(id) = target {
            self.pinned = Some(vec![id]);
            self.sync(window, cx);
        }
    }

    /// Walk the stack of selected items, wrapping at both ends like the Details preview does.
    fn step_stack(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows.len() < 2 {
            return;
        }
        self.stack = crate::panels::details::stack_step(self.stack, self.rows.len(), forward);
        self.sync(window, cx);
    }

    fn toggle_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pinned = match self.pinned {
            Some(_) => None,
            None => Some(self.ids(&self.rows, cx)),
        };
        self.sync(window, cx);
    }

    /// Stay pinned, but on whatever the grid has selected now.
    fn jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.table() else {
            return;
        };
        let rows = state.read(cx).delegate().selected_source_rows();
        self.pinned = Some(self.ids(&rows, cx));
        self.sync(window, cx);
    }

    /// Switch the sidebar to Find, opening it if it was collapsed — only a document has text.
    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(viewer) = self.viewer.clone().filter(|v| v.read(cx).document) else {
            return;
        };
        self.sidebar = true;
        viewer.update(cx, |viewer, cx| viewer.open_find(window, cx));
        cx.notify();
    }

    /// Back to the Details tab — what Escape means here, and all it means.
    fn show_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(viewer) = self.viewer.clone().filter(|v| v.read(cx).find_open) {
            viewer.update(cx, |viewer, cx| viewer.close_find(window, cx));
        }
    }

    fn key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &ev.keystroke;
        // Arrows are a text box's caret wherever one has focus; they mean rows only on the page
        // or the bare window.
        let reading = self.focus_handle.is_focused(window)
            || self
                .viewer
                .as_ref()
                .is_some_and(|viewer| viewer.read(cx).focus_handle.is_focused(window));
        match keys.key.as_str() {
            "f" if keys.modifiers.secondary() => self.open_find(window, cx),
            "up" | "down" if reading && !keys.modifiers.alt => {
                self.step(if keys.key == "up" { -1 } else { 1 }, window, cx)
            }
            "left" | "right" if reading && keys.modifiers.alt => {
                self.step_stack(keys.key == "right", window, cx)
            }
            _ => {}
        }
    }

    /// The window's title, as the file part and the rest, which the title bar mutes.
    fn title(&self, cx: &App) -> (String, String) {
        let project = cx
            .try_global::<CurrentProject>()
            .map(|p| p.display_name())
            .unwrap_or_default();
        let file = match (self.rows.len(), self.front().zip(self.table())) {
            (0, _) | (_, None) => String::new(),
            (1, Some((row, state))) => {
                let delegate = state.read(cx).delegate();
                delegate
                    .row_image(row)
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .or_else(|| {
                        table::file_links::missing_file(delegate, row, cx)
                            .map(|(_, name)| name.to_string())
                    })
                    .unwrap_or_default()
            }
            (count, _) => format!("{count} items"),
        };
        let rest = match file.is_empty() {
            true => format!("{project} — qrate"),
            false => format!(" — {project} — qrate"),
        };
        (file, rest)
    }

    /// Where this window is in the grid's order, and — while pinned somewhere else — which row
    /// the grid is on.
    fn readouts(&self, cx: &App) -> (String, Option<usize>) {
        let Some(state) = self.table() else {
            return ("No selection".into(), None);
        };
        let delegate = state.read(cx).delegate();
        let total = delegate.visible().len();
        let views: Vec<usize> = self
            .rows
            .iter()
            .filter_map(|&row| delegate.view_row(row))
            .collect();
        let readout = match (self.rows.len(), views.iter().min(), views.iter().max()) {
            (0, ..) => "No selection".to_string(),
            (1, Some(view), _) => format!("Row {} of {total}", view + 1),
            (1, None, _) => "Row hidden by the filter".to_string(),
            (count, Some(low), Some(high)) => {
                format!("Rows {}–{} · {count} selected", low + 1, high + 1)
            }
            (count, ..) => format!("{count} selected"),
        };
        let table_row = self
            .pinned
            .as_ref()
            .and_then(|_| delegate.cursor_row())
            // Only the grid's cursor, not its whole selection: this runs on every repaint.
            .filter(|row| !self.rows.contains(row))
            .and_then(|row| delegate.view_row(row))
            .map(|view| view + 1);
        (readout, table_row)
    }

    fn title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let pinned = self.pinned.is_some();
        let (readout, table_row) = self.readouts(cx);
        let (file, rest) = self.title(cx);
        let dirty = settings::dirty::Dirty::has(settings::dirty::PROJECT_DATA, cx);
        let theme = cx.theme();
        let (fg, muted, border, chip, link) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.muted,
            theme.primary,
        );

        TitleBar::new()
            .text_xs()
            .text_color(fg)
            .child(
                h_flex().flex_1().min_w_0().child(
                    // Occluded: the title bar is a drag region, which Windows never delivers a
                    // click from.
                    h_flex()
                        .min_w_0()
                        .gap_1()
                        .occlude()
                        .child(
                            Button::new("pop-out-follow")
                                .icon(Icon::empty().path(match pinned {
                                    true => "icons/pin-filled.svg",
                                    false => "icons/pin.svg",
                                }))
                                .label(match pinned {
                                    true => "Pinned",
                                    false => "Following",
                                })
                                .ghost()
                                .xsmall()
                                .selected(pinned)
                                .disabled(!pinned && self.rows.is_empty())
                                .tooltip(match pinned {
                                    true => "Follow the table's selection again",
                                    false => "Keep this item here while the table moves on",
                                })
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.toggle_pin(window, cx)),
                                ),
                        )
                        .child(div().flex_none().w_px().h(px(14.)).mx_0p5().bg(border))
                        .child(
                            Button::new("pop-out-previous-row")
                                .icon(IconName::ChevronUp)
                                .ghost()
                                .xsmall()
                                .tooltip("Previous row (↑)")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.step(-1, window, cx)),
                                ),
                        )
                        .child(
                            Button::new("pop-out-next-row")
                                .icon(IconName::ChevronDown)
                                .ghost()
                                .xsmall()
                                .tooltip("Next row (↓)")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.step(1, window, cx)),
                                ),
                        )
                        .child(div().min_w_0().truncate().text_color(muted).child(readout))
                        .when_some(table_row, |bar, row| {
                            bar.child(
                                h_flex()
                                    .flex_none()
                                    .items_center()
                                    .gap_1p5()
                                    .h(px(20.))
                                    .px_2()
                                    .ml_1()
                                    .rounded_full()
                                    .bg(chip)
                                    .whitespace_nowrap()
                                    .child(format!("Table is on row {row}"))
                                    .child(
                                        div()
                                            .id("pop-out-jump")
                                            .text_color(link)
                                            .cursor_pointer()
                                            .child("Jump")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.jump(window, cx)
                                            })),
                                    ),
                            )
                        }),
                ),
            )
            // The file name gives way before the row controls do.
            .child(
                h_flex()
                    .min_w_0()
                    .max_w(relative(0.42))
                    .gap_1p5()
                    .items_center()
                    .when(dirty, |title| {
                        title.child(div().flex_none().size(px(6.)).rounded_full().bg(fg))
                    })
                    .child(div().min_w_0().truncate().child(file))
                    .child(div().min_w_0().truncate().text_color(muted).child(rest)),
            )
            .child(
                h_flex().flex_1().justify_end().pr_2().child(
                    div().occlude().child(
                        Button::new("pop-out-sidebar")
                            .icon(Icon::empty().path(match self.sidebar {
                                true => "icons/panel-right-filled.svg",
                                false => "icons/panel-right.svg",
                            }))
                            .ghost()
                            .small()
                            .tooltip(match self.sidebar {
                                true => "Hide details",
                                false => "Show details",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar = !this.sidebar;
                                cx.notify();
                            })),
                    ),
                ),
            )
            .into_any_element()
    }

    /// The file, or what the stage says in its place, over the viewer's own backdrop.
    fn stage(&self, cx: &mut Context<Self>) -> AnyElement {
        let (fg, muted) = (rgb(STAGE_FG), rgb(STAGE_MUTED));
        // Only the messages read it, and they show only when there is no viewer.
        let front = self
            .front()
            .filter(|_| self.viewer.is_none())
            .zip(self.table())
            .map(|(row, state)| {
                let delegate = state.read(cx).delegate();
                (
                    row,
                    delegate.row_image(row).map(|path| path.to_path_buf()),
                    table::file_links::missing_file(delegate, row, cx).map(|(_, name)| name),
                )
            });

        // What to say when there is nothing the viewer can draw. The one useful action goes with
        // it, where there is one.
        let message = self.viewer.is_none().then(|| match front {
            None => v_flex()
                .items_center()
                .gap_1p5()
                .max_w(px(360.))
                .p_6()
                .text_center()
                .child(
                    Icon::new(IconName::LayoutDashboard)
                        .size_8()
                        .text_color(muted),
                )
                .child(div().mt_1().text_sm().text_color(fg).child("Nothing selected"))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(muted)
                        .child("Select a row in the table to see its file here. This window follows the selection."),
                ),
            Some((row, None, Some(name))) => v_flex()
                .items_center()
                .gap_2()
                .max_w(px(420.))
                .p_6()
                .text_center()
                .child(
                    Icon::empty()
                        .path("icons/file-x.svg")
                        .size_12()
                        .text_color(cx.theme().warning),
                )
                .child(div().text_sm().text_color(fg).child("File not found"))
                .child(
                    div()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(muted)
                        .child(name),
                )
                .child(
                    Button::new("pop-out-locate-file")
                        .small()
                        .mt_1()
                        .label("Locate file…")
                        .on_click(move |_, window, cx| {
                            TablePanelHandle::update(cx, |table, cx| {
                                table.locate_file(row, window, cx)
                            });
                        }),
                ),
            Some((_, file, _)) => {
                let tag = file
                    .as_deref()
                    .and_then(|path| path.extension())
                    .map(|ext| ext.to_string_lossy().to_uppercase());
                v_flex()
                    .items_center()
                    .gap_2p5()
                    .p_6()
                    .text_center()
                    .child(
                        div()
                            .relative()
                            .size_16()
                            .text_color(muted)
                            .child(Icon::new(IconName::File).size_16())
                            .children(tag.clone().map(|tag| {
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom(px(14.))
                                    .text_center()
                                    .text_size(px(11.))
                                    .font_semibold()
                                    .child(tag)
                            })),
                    )
                    .child(div().text_size(px(13.)).text_color(muted).child(
                        match (&file, &tag) {
                            (Some(_), Some(tag)) => format!("No preview for {tag} files"),
                            (Some(_), None) => "No preview for this file".to_string(),
                            (None, _) => "No file is linked to this item".to_string(),
                        },
                    ))
                    .children(file.map(|path| {
                        Button::new("pop-out-open-default")
                            .small()
                            .icon(IconName::ExternalLink)
                            .label("Open in the default app")
                            .on_click(move |_, _, _| {
                                if let Err(err) = settings::os_open::open_in_default_app(&path) {
                                    log::error!("could not open {}: {err}", path.display());
                                }
                            })
                    }))
            }
        });

        let count = self.rows.len();
        let at = self.stack.min(count.saturating_sub(1));
        // Above the viewer's own pill when it has one, so a stack of PDFs keeps its page controls.
        let lift = match self
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.read(cx).has_controls())
        {
            true => px(64.),
            false => px(16.),
        };
        let theme = cx.theme();
        let (background, popover, border, radius) =
            (theme.background, theme.popover, theme.border, theme.radius);

        div()
            .id("pop-out-stage")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(background)
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                this.stack_hover = *over;
                cx.notify();
            }))
            // Back from the sidebar, a click on the page gives the arrows and zoom keys back to it.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    let focus = match &this.viewer {
                        Some(viewer) => viewer.read(cx).focus_handle.clone(),
                        None => this.focus_handle.clone(),
                    };
                    focus.focus(window, cx);
                }),
            )
            .child(
                // The viewer's backdrop, painted here so every state of the stage shares it.
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(0.85)),
            )
            .children(self.viewer.clone())
            .children(message.map(|message| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(message)
            }))
            .when(count > 1, |stage| {
                stage
                    .when(self.stack_hover, |stage| {
                        stage.children([true, false].map(|left| {
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .map(|side| match left {
                                    true => side.left_4(),
                                    false => side.right_4(),
                                })
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .rounded_full()
                                        .bg(popover.opacity(0.8))
                                        .border_1()
                                        .border_color(border)
                                        .occlude()
                                        .child(
                                            Button::new(match left {
                                                true => "pop-out-stack-previous",
                                                false => "pop-out-stack-next",
                                            })
                                            .icon(match left {
                                                true => IconName::ChevronLeft,
                                                false => IconName::ChevronRight,
                                            })
                                            .ghost()
                                            .rounded_full()
                                            .tooltip(match left {
                                                true => "Previous selected item (Alt+←)",
                                                false => "Next selected item (Alt+→)",
                                            })
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.step_stack(!left, window, cx)
                                            })),
                                        ),
                                )
                        }))
                    })
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(lift)
                            .flex()
                            .justify_center()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .p_1()
                                    .rounded(radius * 2.)
                                    .bg(popover)
                                    .border_1()
                                    .border_color(border)
                                    .shadow_lg()
                                    .occlude()
                                    .text_color(cx.theme().foreground)
                                    .text_size(px(13.))
                                    .child(
                                        Button::new("pop-out-stack-back")
                                            .icon(IconName::ChevronLeft)
                                            .ghost()
                                            .small()
                                            .tooltip("Previous selected item (Alt+←)")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.step_stack(false, window, cx)
                                            })),
                                    )
                                    .child(
                                        div().px_1().child(format!("Item {} of {count}", at + 1)),
                                    )
                                    // Past a dozen the dots stop telling anyone anything.
                                    .when(count <= 12, |pill| {
                                        pill.child(h_flex().gap_1().px_1().children(
                                            (0..count).map(|ix| {
                                                div().size(px(6.)).rounded_full().bg(
                                                    match ix == at {
                                                        true => cx.theme().primary,
                                                        false => cx.theme().muted,
                                                    },
                                                )
                                            }),
                                        ))
                                    })
                                    .child(
                                        Button::new("pop-out-stack-forward")
                                            .icon(IconName::ChevronRight)
                                            .ghost()
                                            .small()
                                            .tooltip("Next selected item (Alt+→)")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.step_stack(true, window, cx)
                                            })),
                                    ),
                            ),
                    )
            })
            .into_any_element()
    }

    /// Details, or — for a document — Details and Find as two tabs. A tab rather than a third
    /// column, so the window still works at its smallest.
    fn sidebar(&self, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let document = self
            .viewer
            .clone()
            .filter(|viewer| viewer.read(cx).document);
        let finding = document
            .as_ref()
            .is_some_and(|viewer| viewer.read(cx).find_open);
        let theme = cx.theme();
        let (fg, muted, border, primary, strip, background) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.primary,
            theme.tab_bar,
            theme.background,
        );
        let tab = |id: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .px_2()
                .cursor_pointer()
                .text_color(if on { fg } else { muted })
                .when(on, |tab| tab.border_b_2().border_color(primary))
                .child(label)
        };
        let header =
            h_flex()
                .flex_none()
                .h(HEADER_H)
                .bg(strip)
                .border_b_1()
                .border_color(border)
                .text_size(px(13.))
                .map(|header| match document.is_some() {
                    false => header.items_center().px_3().child("Details"),
                    true => header
                        .items_stretch()
                        .gap_0p5()
                        .px_2()
                        .child(tab("pop-out-tab-details", "Details", !finding).on_click(
                            cx.listener(|this, _, window, cx| this.show_details(window, cx)),
                        ))
                        .child(tab("pop-out-tab-find", "Find", finding).on_click(
                            cx.listener(|this, _, window, cx| this.open_find(window, cx)),
                        )),
                });
        let body = match document.filter(|_| finding) {
            Some(viewer) => viewer.update(cx, |viewer, cx| {
                viewer::find::panel(&viewer.find, width, true, cx)
            }),
            None => self.details.clone().into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(background)
            .border_l_1()
            .border_color(border)
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

impl Render for PopOut {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);
        // The live width, so the find rows re-trim as the sidebar is dragged.
        let width = self
            .split
            .read(cx)
            .sizes()
            .get(1)
            .copied()
            .unwrap_or(SIDEBAR);

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .key_context("PopOut")
            .track_focus(&self.focus_handle)
            .id("pop-out")
            .role(Role::Group)
            .aria_label("Pop-out viewer")
            // Escape from Details would deselect the grid's rows; here it only ever means "back
            // to Details", and past that, nothing.
            .on_action(
                cx.listener(|this, _: &table::Deselect, window, cx| this.show_details(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                    this.show_details(window, cx)
                }),
            )
            .on_key_down(cx.listener(Self::key_down))
            .child(self.title_bar(cx))
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("pop-out-split")
                        .with_state(&self.split)
                        .child(resizable_panel().child(self.stage(cx)))
                        // `visible` rather than adding and removing the panel, so a collapse keeps
                        // the width the sidebar was dragged to.
                        .child(
                            resizable_panel()
                                .size(SIDEBAR)
                                .size_range(SIDEBAR_RANGE)
                                .visible(self.sidebar)
                                .child(self.sidebar(width, cx)),
                        ),
                ),
            )
            .children(dialog_layer)
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use gpui::{Entity, TestAppContext, VisualTestContext};
    use gpui_component::table::TableState;
    use table::{QrateTableDelegate, TableChanged};

    use super::PopOut;

    /// Three rows in a real grid, and the pop-out window watching it.
    fn window_over_a_table(
        cx: &mut TestAppContext,
    ) -> (
        Entity<PopOut>,
        Entity<TableState<QrateTableDelegate>>,
        &mut VisualTestContext,
    ) {
        let state = crate::test_support::open_table(
            cx,
            "qrate-pop-out.qrate",
            settings::project::ProjectData {
                name: "Aderman Collection".into(),
                columns: Vec::new(),
                headers: vec!["Identifier".into(), "Title".into()],
                rows: vec![
                    vec!["ADR-0042".into(), "Beacon Hill Park".into()],
                    vec!["ADR-0043".into(), "Sawmill crew".into()],
                    vec!["ADR-0044".into(), "Saanich mill".into()],
                ],
                row_ids: vec![11, 12, 13],
                values: Default::default(),
            },
        );
        let (pop_out, cx) = cx.add_window_view(PopOut::new);
        (pop_out, state, cx)
    }

    fn select(
        state: &Entity<TableState<QrateTableDelegate>>,
        rows: &[usize],
        cx: &mut VisualTestContext,
    ) {
        state.update(cx, |state, cx| {
            state.delegate_mut().select_only_row(rows[0]);
            for &row in &rows[1..] {
                state.delegate_mut().toggle_row(row);
            }
            cx.emit(TableChanged);
        });
        cx.run_until_parked();
    }

    /// The window's reason to exist: it follows the grid until pinned, then holds its item while
    /// the grid moves on — and says where the grid went, so Jump can catch up.
    #[gpui::test]
    fn a_pinned_window_holds_its_item_while_the_table_moves_on(cx: &mut TestAppContext) {
        let (pop_out, state, cx) = window_over_a_table(cx);

        select(&state, &[0], cx);
        pop_out.read_with(cx, |pop_out, _| assert_eq!(pop_out.rows, [0]));

        pop_out.update_in(cx, |pop_out, window, cx| pop_out.toggle_pin(window, cx));
        select(&state, &[1], cx);
        pop_out.read_with(cx, |pop_out, cx| {
            assert_eq!(pop_out.rows, [0], "pinned, the grid moving is not ours");
            assert_eq!(
                pop_out.readouts(cx),
                ("Row 1 of 3".to_string(), Some(2)),
                "and the chip says where the grid is"
            );
        });

        pop_out.update_in(cx, |pop_out, window, cx| pop_out.jump(window, cx));
        pop_out.read_with(cx, |pop_out, cx| {
            assert_eq!(pop_out.rows, [1], "Jump moves this window to the grid");
            assert!(pop_out.pinned.is_some(), "and leaves it pinned there");
            assert_eq!(
                pop_out.readouts(cx).1,
                None,
                "so there is nowhere left to jump"
            );
        });

        pop_out.update_in(cx, |pop_out, window, cx| pop_out.toggle_pin(window, cx));
        select(&state, &[2], cx);
        pop_out.read_with(cx, |pop_out, _| {
            assert_eq!(pop_out.rows, [2], "unpinned, it follows again")
        });
    }

    /// A pin is held by row id, so the item stays put when the grid's selection changes — and the
    /// sidebar describes the pinned item, not the grid's.
    #[gpui::test]
    fn the_sidebar_describes_what_the_window_shows(cx: &mut TestAppContext) {
        let (pop_out, state, cx) = window_over_a_table(cx);

        select(&state, &[1], cx);
        pop_out.update_in(cx, |pop_out, window, cx| pop_out.toggle_pin(window, cx));
        pop_out.read_with(cx, |pop_out, _| {
            assert_eq!(pop_out.pinned.as_deref(), Some(&[12][..]))
        });
        select(&state, &[0, 2], cx);

        let details = pop_out.read_with(cx, |pop_out, _| pop_out.details.clone());
        details.read_with(cx, |details, cx| assert_eq!(details.picked(cx), [1]));
    }

    /// A pin is an item, not a position: a row added above it leaves the window on the same item.
    #[gpui::test]
    fn a_pin_stays_on_its_item_when_rows_are_added_above_it(cx: &mut TestAppContext) {
        let (pop_out, state, cx) = window_over_a_table(cx);
        select(&state, &[1], cx);
        pop_out.update_in(cx, |pop_out, window, cx| pop_out.toggle_pin(window, cx));

        state.update(cx, |state, cx| {
            state.delegate_mut().set_data(
                &["Identifier".into(), "Title".into()],
                &[10, 11, 12, 13],
                &[
                    vec!["ADR-0041".into(), "Added above".into()],
                    vec!["ADR-0042".into(), "Beacon Hill Park".into()],
                    vec!["ADR-0043".into(), "Sawmill crew".into()],
                    vec!["ADR-0044".into(), "Saanich mill".into()],
                ],
            );
            cx.emit(TableChanged);
        });
        cx.run_until_parked();

        pop_out.read_with(cx, |pop_out, _| {
            assert_eq!(
                pop_out.rows,
                [2],
                "row id 12 moved down one, and the pin with it"
            )
        });
    }

    /// An item deleted out of a pin of several leaves the pin holding only what is left, so the
    /// next cell edit takes the cheap path rather than re-finding every row.
    #[gpui::test]
    fn a_deleted_item_leaves_the_pin(cx: &mut TestAppContext) {
        let (pop_out, state, cx) = window_over_a_table(cx);
        select(&state, &[0, 2], cx);
        pop_out.update_in(cx, |pop_out, window, cx| pop_out.toggle_pin(window, cx));

        state.update(cx, |state, cx| {
            state.delegate_mut().set_data(
                &["Identifier".into(), "Title".into()],
                &[12, 13],
                &[
                    vec!["ADR-0043".into(), "Sawmill crew".into()],
                    vec!["ADR-0044".into(), "Saanich mill".into()],
                ],
            );
            cx.emit(TableChanged);
        });
        cx.run_until_parked();

        pop_out.read_with(cx, |pop_out, _| {
            assert_eq!(
                pop_out.pinned.as_deref(),
                Some(&[13][..]),
                "11 is gone from the pin"
            );
            assert_eq!(pop_out.rows, [1]);
        });
    }

    /// Several rows: the stage steps through them, wrapping, while the sidebar keeps all of them.
    #[gpui::test]
    fn the_stack_steps_through_the_selection_and_wraps(cx: &mut TestAppContext) {
        let (pop_out, state, cx) = window_over_a_table(cx);
        select(&state, &[0, 2], cx);

        pop_out.update_in(cx, |pop_out, window, cx| {
            assert_eq!(pop_out.front(), Some(0));
            pop_out.step_stack(true, window, cx);
            assert_eq!(pop_out.front(), Some(2));
            pop_out.step_stack(true, window, cx);
            assert_eq!(pop_out.front(), Some(0), "wraps past the end");
            pop_out.step_stack(false, window, cx);
            assert_eq!(pop_out.front(), Some(2), "and past the start");
            assert_eq!(pop_out.rows, [0, 2], "without touching the selection");
            let (readout, _) = pop_out.readouts(cx);
            assert_eq!(readout, "Rows 1–3 · 2 selected");
            assert_eq!(pop_out.title(cx).0, "2 items");
        });

        // A new selection starts at its first item rather than wherever the last one was left.
        select(&state, &[1, 2], cx);
        pop_out.read_with(cx, |pop_out, _| assert_eq!(pop_out.front(), Some(1)));
    }

    struct Blank;

    impl gpui::Render for Blank {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            gpui::div()
        }
    }

    /// Replacing or closing the pop-out's viewer stops only its own recording. The player is the
    /// app's, and what it is playing may be the main window's.
    ///
    /// Like the viewer's playback test, this is the real path on a machine with an output device
    /// and holds trivially on one without.
    #[gpui::test]
    fn leaving_a_file_stops_only_its_own_recording(cx: &mut TestAppContext) {
        let wav = preview::playback::silent_wav(8000);
        let shown = std::env::temp_dir().join("qrate-pop-out-shown.wav");
        std::fs::write(&shown, &wav).unwrap();

        let (_, cx) = cx.add_window_view(|_, _| Blank);
        cx.update(|window, cx| {
            let popped =
                crate::viewer::build(shown.clone(), crate::viewer::Scope::PopOut, window, cx);
            // The same recording, open in the main window too.
            let main =
                crate::viewer::build(shown.clone(), crate::viewer::Scope::Workspace, window, cx);
            preview::playback::play(&shown, main.entity_id(), cx);
            let before = preview::playback::playing(cx).map(|path| path.to_path_buf());
            preview::playback::stop(popped.entity_id(), cx);
            assert_eq!(
                preview::playback::playing(cx).map(|path| path.to_path_buf()),
                before,
                "the main window's playback of the same file plays on"
            );

            preview::playback::play(&shown, popped.entity_id(), cx);
            let before = preview::playback::playing(cx).map(|path| path.to_path_buf());
            crate::viewer::open_viewer(shown.clone(), crate::viewer::Scope::Workspace, window, cx);
            crate::viewer::close_viewer(window, cx);
            assert_eq!(
                preview::playback::playing(cx).map(|path| path.to_path_buf()),
                before,
                "closing the main window's viewer leaves the pop-out playing"
            );

            preview::playback::stop(popped.entity_id(), cx);
            assert!(preview::playback::playing(cx).is_none(), "its own stops");
        });

        let _ = std::fs::remove_file(&shown);
    }

    /// Nothing selected is a state of the stage, not an empty window.
    #[gpui::test]
    fn with_nothing_selected_the_window_says_so(cx: &mut TestAppContext) {
        let (pop_out, _state, cx) = window_over_a_table(cx);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        pop_out.read_with(cx, |pop_out, cx| {
            assert!(pop_out.rows.is_empty());
            assert!(pop_out.viewer.is_none());
            assert_eq!(pop_out.readouts(cx).0, "No selection");
            assert_eq!(
                pop_out.title(cx),
                (String::new(), "Aderman Collection — qrate".to_string())
            );
        });
    }
}

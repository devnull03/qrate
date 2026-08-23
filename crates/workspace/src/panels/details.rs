use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, Sizable, StyledExt as _,
    button::{Button, ButtonVariants},
    dock::{DockPlacement, Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Escape, InputEvent, InputState},
    resizable::{resizable_panel, v_resizable},
    scroll::ScrollableElement,
    table::TableState,
    v_flex,
};
use preview::{can_preview, thumb};
use table::{QrateTableDelegate, TableChanged, TableStateHandle};

use crate::BottomDockCrop;
use crate::panel_registry::PanelMeta;
use crate::viewer::transport::{self, Transport};

/// Project-scoped height of the details panel's image pane, in pixels.
const IMAGE_PANE_HEIGHT_KEY: &str = "details_image_height";

/// Height of the Notes sub-panel's header bar, which is the whole of it while collapsed.
const NOTES_HEADER_H: f32 = 28.;

/// How much of the window the open field editor may span when the panel itself is narrower.
const EDITOR_MAX_WINDOW_SHARE: f32 = 0.4;

/// Lines of a field's value shown in the list before it is cut off with an ellipsis. A description
/// can run to paragraphs, and one field must not push every other field off the panel — the full
/// text is a click away in the editor.
const VALUE_LINE_CLAMP: usize = 4;

/// Where Details starts out and what it puts in the status bar. `default_placement` is only the
/// starting point — the user can dock it anywhere, and that choice is what gets persisted.
pub static DETAILS_META: PanelMeta = PanelMeta {
    name: "DetailsPanel",
    icon: IconName::Info,
    label: "Details",
    default_placement: DockPlacement::Left,
    badge: false,
};

/// Left dock: the selected row's photo (if the files folder resolved one) plus its fields as a
/// label/value list, per the main-workspace design.
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
    /// Re-renders when the bottom dock opens/closes, so the strip-crop padding tracks it.
    _crop_sub: Subscription,
    /// The field editor, shared across whichever field is open — the same one-per-panel
    /// arrangement the grid uses for its cell editor.
    editor: Entity<InputState>,
    /// `(source_rows, data_col)` of the field being edited, in the grid's own coordinates so a
    /// filter change between opening and committing can't redirect the write. Several rows when
    /// the field belongs to a bundle: one edit box writing the same value down the selection.
    editing: Option<(Vec<usize>, usize)>,
    /// Which of the selected items the preview stack is showing, and whether the pointer is over
    /// it — the step arrows only exist while it is, so they never cover the photo at rest.
    stack: usize,
    stack_hover: bool,
    /// Whether the Notes sub-panel is expanded. Collapsed, it leaves the split and becomes a
    /// header strip along the bottom of the panel — the chevron there is what opens it again.
    notes_open: bool,
    /// Commits the open field on Enter or when the editor loses focus.
    _editor_sub: Subscription,
    /// Window-space rect of the field row being edited, and of the scrolling field list the
    /// editor is confined to. Written from `canvas` prepaint, which only gets an `&mut App` —
    /// a shared cell is how the measurement reaches the next render without a global.
    anchor: Rc<Cell<Option<Bounds<Pixels>>>>,
    viewport: Rc<Cell<Bounds<Pixels>>>,
    /// Playback controls for the selected row, when what it links to is a recording. An oral
    /// history is catalogued *while* it is listened to, so the transport belongs beside the fields
    /// being typed — not only in the fullscreen viewer.
    transport: Option<Transport>,
    /// `preview::describe` of the selected file, taken once per selection. It stats the file, and
    /// `render` runs on every scroll tick — asking per frame put a syscall in the scroll path.
    caption: Option<String>,
    /// What `transport` and `caption` were built from. `retarget` runs on every table change, so
    /// without it a keystroke in the grid would re-stat the file.
    file: Option<PathBuf>,
}

impl DetailsPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _handle_sub = cx.observe_global::<TableStateHandle>(|this: &mut Self, cx| {
            this.bind(cx);
            cx.notify();
        });
        // `multi_line`, like the grid's cell editor: `editor_box` sizes the box to the value's
        // *wrapped* height, and a single-line input runs the text off the right edge instead.
        // `submit_on_enter` keeps Enter committing the field (Shift+Enter inserts a newline).
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .submit_on_enter(true)
        });
        let _editor_sub = cx.subscribe(&editor, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.commit(cx);
            }
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            state: None,
            _handle_sub,
            _table_sub: None,
            _crop_sub: cx.observe_global::<BottomDockCrop>(|_this: &mut Self, cx| cx.notify()),
            editor,
            editing: None,
            stack: 0,
            stack_hover: false,
            notes_open: true,
            _editor_sub,
            anchor: Rc::default(),
            viewport: Rc::new(Cell::new(Bounds::default())),
            transport: None,
            caption: None,
            file: None,
        };
        this.bind(cx);
        this
    }

    /// The selected items as source rows in view order — what the whole panel is about, and the
    /// same list the grid, the gallery and the status bar count.
    fn picked(&self, cx: &App) -> Vec<usize> {
        self.state
            .as_ref()
            .and_then(|w| w.upgrade())
            .map(|s| s.read(cx).delegate().selected_source_rows())
            .unwrap_or_default()
    }

    /// The item the preview is showing: the stack's front card. Clamped rather than remembered, so
    /// stepping to the fifth of five and then selecting two doesn't leave the preview blank.
    fn front(&self, cx: &App) -> Option<usize> {
        let picked = self.picked(cx);
        picked
            .get(self.stack.min(picked.len().checked_sub(1)?))
            .copied()
    }

    /// The file the front item links to, which is both what the preview frame draws and what the
    /// transport would play.
    fn selected_file(&self, cx: &App) -> Option<PathBuf> {
        let state = self.state.as_ref()?.upgrade()?;
        let row = self.front(cx)?;
        state
            .read(cx)
            .delegate()
            .row_image(row)
            .map(Path::to_path_buf)
    }

    /// Point the transport at whatever is selected now. A no-op while the selection stays on the
    /// same file — this runs on every table change, and rebuilding would re-probe the file and
    /// throw away the position on every keystroke in the grid.
    fn retarget(&mut self, cx: &mut Context<Self>) {
        let path = self.selected_file(cx);
        if self.file == path {
            return;
        }
        self.file = path.clone();
        self.caption = path.as_deref().and_then(preview::describe);
        // Whatever was playing belonged to the row being left. Leaving it running would narrate
        // one item while the panel details another.
        if self.transport.is_some() {
            preview::playback::stop(cx);
        }
        self.transport = path.and_then(|path| Transport::new(path, cx));
    }

    /// Open `header`'s field for editing, seeded with its current text. The column is resolved by
    /// name — `row_fields` is in display order, which is not the data-column order after a move.
    fn edit_field(
        &mut self,
        header: &SharedString,
        value: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Whatever was open loses focus rather than being silently dropped.
        self.commit(cx);
        let rows = self.picked(cx);
        let located = self
            .state
            .as_ref()
            .and_then(|w| w.upgrade())
            .and_then(|s| s.read(cx).delegate().data_col(header))
            .filter(|_| !rows.is_empty());
        let Some(col) = located else {
            log::warn!("details: no column named {header} to edit");
            return;
        };
        // The rows are captured here, not read back at commit: the write must land on the items
        // the archivist was looking at when they started typing, whatever the grid does meanwhile.
        let at = (rows, col);
        self.editing = Some(at);
        // Re-measured for the new field; the box renders on the frame after the capture.
        self.anchor.set(None);
        self.editor.update(cx, |editor, cx| {
            editor.set_value(value.clone(), window, cx);
            editor.focus(window, cx);
        });
        cx.notify();
    }

    /// The Notes sub-panel: a collapsible list of what has been written about the selection, newest
    /// group last, headed by the item it belongs to once more than one item is picked.
    ///
    /// Returns `AnyElement` because it is one child of a deeply chained builder — see the note on
    /// `render_image_frame`.
    fn notes_panel(&self, picked: &[usize], cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.state.as_ref().and_then(|w| w.upgrade()) else {
            return div().into_any_element();
        };
        let delegate = state.read(cx);
        let delegate = delegate.delegate();
        /// One note as the panel draws it: what it says, and who filed it when.
        type Note = (SharedString, Option<SharedString>);
        /// The notes on one selected item, under that item's title.
        type Group = (SharedString, Vec<Note>);

        let groups: Vec<Group> = picked
            .iter()
            .filter_map(|&row| {
                let notes: Vec<_> =
                    diagnostics::Diagnostics::notes_in_row(diagnostics::DATASET_MAIN, row, cx)
                        .map(|note| {
                            // Which field it hangs off, when it hangs off one: without it a note
                            // about the date and a note about the photographer read as two
                            // remarks on the same thing.
                            let filed = note.filed.as_ref().and_then(diagnostics::Filed::label);
                            let meta = match (note.location.column.as_ref(), filed) {
                                (Some(column), Some(filed)) => Some(format!("{column} · {filed}")),
                                (Some(column), None) => Some(column.to_string()),
                                (None, filed) => filed.map(Into::into),
                            };
                            (note.message.clone(), meta.map(SharedString::from))
                        })
                        .collect();
                if notes.is_empty() {
                    return None;
                }
                let title = delegate
                    .row_fields(row)
                    .into_iter()
                    .map(|(_, value)| value)
                    .find(|value| !value.is_empty())
                    .unwrap_or_default();
                Some((title, notes))
            })
            .collect();
        let total: usize = groups.iter().map(|(_, notes)| notes.len()).sum();
        let several = picked.len() > 1;

        let open = self.notes_open;
        v_flex()
            .size_full()
            .min_h_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(NOTES_HEADER_H))
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .py_1()
                    // Opaque: the notes scroll under this, and a transparent strip let them read
                    // through the heading.
                    .bg(cx.theme().background)
                    .child(
                        Button::new("details-notes-toggle")
                            .icon(match open {
                                true => IconName::ChevronDown,
                                false => IconName::ChevronRight,
                            })
                            .ghost()
                            .xsmall()
                            .tooltip(match open {
                                true => "Collapse notes",
                                false => "Expand notes",
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_open = !this.notes_open;
                                cx.notify();
                            })),
                    )
                    .child(div().text_xs().font_semibold().child("Notes"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match total {
                                0 => "none".to_string(),
                                n => n.to_string(),
                            }),
                    ),
            )
            .when(open, |section| {
                let crop = cx.try_global::<BottomDockCrop>().map_or(px(0.), |c| c.0);
                section.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .px_3()
                        // Same crop compensation the field list makes: the closed bottom dock's
                        // strip covers this panel's last 29px, which is where the newest note sits.
                        .pb(px(8.) + crop)
                        .child(
                            v_flex()
                                .gap_2()
                                .children(groups.into_iter().map(|(title, notes)| {
                                    v_flex()
                                        .gap_1()
                                        // Only worth saying whose note this is when the selection
                                        // holds more than one item to confuse it with.
                                        .when(several, |group| {
                                            group.child(
                                                div().text_xs().truncate().child(title.clone()),
                                            )
                                        })
                                        .children(notes.into_iter().map(|(text, meta)| {
                                            v_flex()
                                                .gap_0p5()
                                                .p_1p5()
                                                .rounded(cx.theme().radius)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .bg(cx.theme().muted.opacity(0.4))
                                                .child(div().text_xs().child(text))
                                                .children(meta.map(|meta| {
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(meta)
                                                }))
                                        }))
                                }))
                                .when(total == 0, |list| {
                                    list.child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("No notes on this selection."),
                                    )
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    /// Move the preview stack one item along, wrapping at both ends so a bundle can be walked in
    /// either direction without hunting for the end of it.
    fn step_stack(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.picked(cx).len().max(1);
        let at = self.stack.min(count - 1);
        self.stack = match forward {
            true => (at + 1) % count,
            false => (at + count - 1) % count,
        };
        self.retarget(cx);
        cx.notify();
    }

    /// Write the open field back through the grid's own mutation path, so dirty-tracking,
    /// validation and undo stay single-sourced. Clearing `editing` first keeps the `TableChanged`
    /// this provokes from re-entering as a second commit.
    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some((rows, col)) = self.editing.take() else {
            return;
        };
        let value = self.editor.read(cx).value().clone();
        // One batch, so setting a field across a bundle is a single undo step — and `apply_edit`
        // drops the rows whose text this didn't change, so committing an untouched shared field
        // costs nothing.
        let cells = rows.into_iter().map(|row| (row, col, value.clone()));
        table::write_cells(cells.collect(), cx);
        cx.notify();
    }

    /// The floating field editor — the grid's own box, anchored over the field it edits and
    /// clamped to the field list. `None` until the field has measured itself, one frame after the
    /// click.
    fn field_editor(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.editing.as_ref()?;
        let anchor = self.anchor.get()?;
        let within = self.editor_area(window);
        let (box_el, _) = table::editor_box(&self.editor, anchor, within, window, cx);
        Some(deferred(table::floating::float_at(anchor.origin, within, box_el)).into_any_element())
    }

    /// The rect the editor grows within: the field list, widened rightward over whatever is beside
    /// the panel. A dock is narrow and a paragraph-length description confined to it is a column of
    /// two-word lines nobody can read. Capped at a share of the window rather than left unbounded,
    /// so the box never becomes the thing covering the record it belongs to, and never past the
    /// window edge — a right dock has nowhere to spill, and gets the panel width it already had.
    fn editor_area(&self, window: &Window) -> Bounds<Pixels> {
        let list = self.viewport.get();
        let window_width = window.viewport_size().width;
        let width = list
            .size
            .width
            .max(window_width * EDITOR_MAX_WINDOW_SHARE)
            .min(window_width - list.left());
        Bounds {
            origin: list.origin,
            size: size(width, list.size.height),
        }
    }

    fn bind(&mut self, cx: &mut Context<Self>) {
        self.state = cx.try_global::<TableStateHandle>().map(|h| h.0.clone());
        self._table_sub = self.state.as_ref().and_then(|w| w.upgrade()).map(|entity| {
            cx.subscribe(&entity, |this, _st, _ev: &TableChanged, cx| {
                this.retarget(cx);
                cx.notify();
            })
        });
        self.retarget(cx);
    }
}

impl transport::Host for DetailsPanel {
    fn transport(&mut self) -> Option<&mut Transport> {
        self.transport.as_mut()
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

/// The image frame: the selected row's photo if one resolved and is a decodable image, else a
/// muted placeholder icon chosen from the file's extension — same rounded/bordered box either
/// way. Whenever a path resolved at all, two ghost icon buttons overlay the top-right: open in
/// the OS default viewer, reveal in the file manager (plain shell-outs, no panel state touched).
/// Those matter *most* for the icon case, where the OS is the only way to actually see the file.
/// `preview` caches by path and size, in memory and on disk, so stepping back to a row already
/// looked at costs neither a re-decode nor a re-read.
///
/// Returns `AnyElement` (erased), not `impl IntoElement` — this is called from several `Render`
/// impls (the real panel plus test probes), and gpui's chained builder calls produce deeply
/// nested generic types; propagating that concrete type into every caller overflowed rustc's
/// stack during type-checking instead of just hitting a slow compile.
fn render_image_frame(
    image_path: Option<PathBuf>,
    caption: Option<String>,
    transport: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let show_image = image_path.as_deref().is_some_and(can_preview);

    let action = |id: &'static str, icon: IconName, tip: &'static str, path: PathBuf| {
        Button::new(id)
            .icon(icon)
            .ghost()
            .small()
            .tooltip(tip)
            .on_click(move |_, _, _| {
                let result = match id {
                    "open-image" => settings::os_open::open_in_default_app(&path),
                    _ => settings::os_open::reveal_in_folder(&path),
                };
                if let Err(err) = result {
                    log::error!("image action {id} failed: {err}");
                }
            })
    };

    // `h_full`, not a fixed height: the caller sizes the frame (a resizable panel), so dragging
    // the split re-letterboxes the photo instead of stretching or cropping it.
    div()
        .relative()
        .size_full()
        // `min_w_0`/`min_h_0` drop the min-content floor so the frame tracks the pane and the image letterboxes.
        .min_w_0()
        .min_h_0()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .bg(cx.theme().muted)
        .map(|frame| match image_path {
            Some(path) => frame.child(thumb(Some(&path), preview::PANE, cx)).child(
                div()
                    .absolute()
                    .top_1()
                    .right_1()
                    .flex()
                    .gap_1()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background)
                    // Fullscreen only makes sense for something we can actually render; an
                    // icon-placeholder file has nothing to zoom into.
                    .when(show_image, |group| {
                        let path = path.clone();
                        group.child(
                            Button::new("fullscreen-image")
                                .icon(IconName::Maximize)
                                .ghost()
                                .small()
                                .tooltip("View fullscreen")
                                .on_click(move |_, _, cx| {
                                    crate::open_viewer(
                                        path.clone(),
                                        crate::ViewerScope::Workspace,
                                        cx,
                                    )
                                }),
                        )
                    })
                    .child(action(
                        "open-image",
                        IconName::ExternalLink,
                        "Open in default app",
                        path.clone(),
                    ))
                    .child(action(
                        "reveal-image",
                        IconName::FolderOpen,
                        "Reveal in folder",
                        path,
                    )),
            ),
            None => frame.child(thumb(None, preview::PANE, cx)),
        })
        // Top-left, over the picture rather than taking a row out of the frame — the pane is a
        // height the user drags, and the other two edges are spoken for: the action buttons sit
        // top-right and a recording's transport along the bottom. Opaque background and full
        // `foreground` text, same as the buttons opposite it: a translucent chip in
        // `muted_foreground` disappeared into a bright scan behind it.
        .children(caption.map(|caption| {
            div()
                .absolute()
                .top_1()
                .left_1()
                .px_1p5()
                .py_0p5()
                .rounded(cx.theme().radius)
                .bg(cx.theme().background)
                .text_xs()
                .text_color(cx.theme().foreground)
                .child(caption)
        }))
        // Along the bottom of the frame, over the cover art rather than beside it: the pane is a
        // fixed height the user drags, and a bar taking a row out of it would shrink the picture
        // every time a recording is selected.
        .children(transport.map(|bar| {
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .p_1()
                .bg(cx.theme().background.opacity(0.8))
                .child(bar)
        }))
        .into_any_element()
}

/// One of the preview stack's step arrows, pinned to the edge its chevron points at and centred
/// down the card. Full-height flex rather than a top offset: the pane is a height the user drags,
/// so there is no fixed centre to hardcode.
fn step(
    id: &'static str,
    left: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .map(|side| match left {
            true => side.left(px(6.)),
            false => side.right(px(6.)),
        })
        .flex()
        .items_center()
        .child(
            // Opaque chip under the chevron, like the caption and the action buttons: a ghost
            // button alone is a thin glyph over whatever the photo happens to be, and it vanished
            // against both a dark scan and a blown-out one.
            div()
                .rounded(cx.theme().radius)
                .bg(cx.theme().background)
                .shadow_sm()
                .child(
                    Button::new(id)
                        .icon(match left {
                            true => IconName::ChevronLeft,
                            false => IconName::ChevronRight,
                        })
                        .ghost()
                        .small()
                        .tooltip(match left {
                            true => "Previous selected item",
                            false => "Next selected item",
                        })
                        .on_click(on_click),
                ),
        )
        .into_any_element()
}

/// What a bundle of items has to say about each column: the value when they agree, and how many
/// distinct values there are when they don't. Display order, so it lines up with the grid.
///
/// A field that reads `Mixed` is still editable — typing into it sets that value on every selected
/// item, which is the whole reason to select several rows before touching a field.
fn shared_fields(
    delegate: &QrateTableDelegate,
    picked: &[usize],
) -> Vec<(SharedString, SharedString, bool)> {
    let Some((&first, rest)) = picked.split_first() else {
        return Vec::new();
    };
    let base = delegate.row_fields(first);
    let mut distinct: Vec<HashSet<SharedString>> = base
        .iter()
        .map(|(_, value)| HashSet::from([value.clone()]))
        .collect();
    for &row in rest {
        for (ix, (_, value)) in delegate.row_fields(row).into_iter().enumerate() {
            if let Some(values) = distinct.get_mut(ix) {
                values.insert(value);
            }
        }
    }
    base.into_iter()
        .zip(distinct)
        .map(|((key, value), values)| match values.len() {
            1 => (key, value, false),
            n => (key, format!("Mixed ({n} values)").into(), true),
        })
        .collect()
}

impl Render for DetailsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picked = self.picked(cx);
        let front = self.front(cx);
        let count = picked.len();
        let selection = self.state.as_ref().and_then(|w| w.upgrade()).map(|s| {
            let delegate = s.read(cx).delegate();
            let image = front.and_then(|row| delegate.row_image(row).map(Path::to_path_buf));
            (shared_fields(delegate, &picked), image)
        });

        // Compact: this dock is one the user drags narrow, and the full-width scrubber would push
        // the total time out past the frame's edge.
        let transport = self
            .transport
            .as_ref()
            .map(|it| transport::bar(it, true, cx));

        let crop = cx.try_global::<BottomDockCrop>().map_or(px(0.), |c| c.0);
        // The gallery is already a wall of the same thumbnails, so this panel's own copy is
        // redundant there — read from the setting `views::switch` writes, which is also what
        // survives a relaunch, rather than reaching into the centre panel.
        let gallery = crate::ViewMode::parse(&settings::effective_text(crate::VIEW_MODE_KEY, cx))
            == crate::ViewMode::Gallery;

        let Some((fields, image_path)) = selection.filter(|(f, _)| !f.is_empty()) else {
            // Says what this panel is for and how to fill it, rather than only reporting that it
            // is empty — the multi-select gesture is the one thing here nobody discovers by luck.
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .p_6()
                .text_center()
                .child(
                    gpui_component::Icon::new(IconName::LayoutDashboard)
                        .size_6()
                        .text_color(cx.theme().muted_foreground.opacity(0.7)),
                )
                .child(div().text_sm().child("Nothing selected"))
                .child(
                    div()
                        .max_w(px(190.))
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "Select a row or a thumbnail to see its fields here. Hold {} to pick several.",
                            match cfg!(target_os = "macos") {
                                true => "⌘",
                                false => "Ctrl",
                            }
                        )),
                )
                .text_color(cx.theme().muted_foreground)
                .into_any_element();
        };

        // Built before the field rows below, which borrow `cx` for as long as they stay a lazy
        // iterator — this needs `&mut cx` and cannot wait for them.
        let notes = (gallery && count > 0).then(|| self.notes_panel(&picked, cx));
        // Collapsing pins the panel's size range to its header instead of taking it out of the
        // split. Removing it re-syncs the group — every panel's size is rescaled to the container
        // when the count changes — so the fields jumped on the way out and the notes came back at
        // whatever height that rescale had left, not the one they were dragged to. Pinned, the
        // stored size is never touched, and reopening restores it exactly.
        //
        // The floor includes the crop because the header sits at the top of the panel: a closed
        // bottom dock covers the side docks' last 29px, and without the allowance the whole
        // collapsed bar lands inside that band.
        let collapsed_h = NOTES_HEADER_H + f32::from(crop);
        let notes_range = match self.notes_open {
            true => px(80.)..px(320.),
            false => px(collapsed_h)..px(collapsed_h),
        };

        // Hand-built attribute list, not `DescriptionList`/`DataTable`: the fields are fixed pairs,
        // and it reads as a list rather than a second grid — alternating rows carry the structure,
        // no borders.
        let editing_col = self.editing.as_ref().map(|(_, col)| *col);
        let rows = fields.into_iter().enumerate().map(|(ix, (k, v, mixed))| {
            // Guarded on `editing_col`: `data_col` is a linear scan of every column, and this runs
            // per row on every render — including the scroll ticks that dirty the whole panel.
            let open = editing_col.is_some()
                && self
                    .state
                    .as_ref()
                    .and_then(|w| w.upgrade())
                    .and_then(|s| s.read(cx).delegate().data_col(&k))
                    == editing_col;
            div()
                .flex()
                .items_start()
                .when(ix % 2 == 1, |r| r.bg(cx.theme().muted.opacity(0.4)))
                .child(
                    // Both columns are shares of the panel's width, not fixed pixels: this dock
                    // resizes, and a fixed label column either wastes half a wide panel or crushes
                    // the values in a narrow one.
                    div()
                        .w(relative(0.35))
                        .flex_shrink_0()
                        .px_2()
                        .py_1p5()
                        .text_color(cx.theme().muted_foreground)
                        .child(k.clone()),
                )
                // Plain text rather than `TextView`, which parses markdown/html and mangles raw
                // metadata. Click opens the floating editor over it; copy is Ctrl+C once it's open.
                // `min_w_0` overrides flex `min-width: auto` so the value wraps instead of
                // overflowing right.
                .child(
                    div()
                        .id(ix)
                        .flex_1()
                        .min_w_0()
                        // Positions the measuring canvas below against this field, not against
                        // whatever ancestor happens to be positioned.
                        .relative()
                        .px_2()
                        .py_1p5()
                        .cursor_text()
                        // `text_ellipsis` is what puts the … on the last kept line; `line_clamp`
                        // alone would cut the text off mid-word with nothing to say it had.
                        .line_clamp(VALUE_LINE_CLAMP)
                        .text_ellipsis()
                        // A mixed field shows what the items disagree about but seeds the editor
                        // empty: `Mixed (3 values)` is a summary, and committing it as text would
                        // write that literal string onto every item.
                        .when(mixed, |value| {
                            value.italic().text_color(cx.theme().muted_foreground)
                        })
                        .on_click(cx.listener({
                            let (k, seed) = (
                                k.clone(),
                                if mixed {
                                    SharedString::default()
                                } else {
                                    v.clone()
                                },
                            );
                            move |this, _, window, cx| this.edit_field(&k, &seed, window, cx)
                        }))
                        .child(v)
                        // The editor floats over the field rather than replacing it in the row, so
                        // it can grow past the row's width — the grid's editor, same box.
                        .when(open, |value| {
                            let anchor = self.anchor.clone();
                            value.child(
                                canvas(
                                    move |bounds, window, cx| {
                                        if anchor.replace(Some(bounds)).is_none() {
                                            // Read on the *next* render, and nothing else schedules
                                            // one. Deferred because `Window::refresh` is a no-op
                                            // while a frame is drawing — which is when this runs.
                                            window.defer(cx, |window, _| window.refresh());
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                // `top_0`/`left_0` are load-bearing: an absolute element with no
                                // insets takes a *static* position after its in-flow siblings, so
                                // without them this measures a rect one line below the value text
                                // and the box opens under the field it edits.
                                .top_0()
                                .left_0()
                                .size_full(),
                            )
                        }),
                )
        });

        // Split so the image stays put while only the fields scroll, with a drag handle to trade heights.
        // `.size()` is the initial size only — once dragged, `ResizableState` owns it, so re-reading restores.
        let image_height = cx
            .try_global::<settings::project::CurrentProject>()
            .and_then(|p| p.data.values.get(IMAGE_PANE_HEIGHT_KEY))
            .and_then(|v| v.text().parse::<f32>().ok())
            .unwrap_or(180.);

        // Own context + tracked focus so Ctrl+Z reaches the grid's history from in here. While the
        // field editor holds focus its own deeper `Input` context wins, which keeps Ctrl+Z as
        // text-undo mid-edit.
        div()
            .size_full()
            .key_context(DETAILS_META.name)
            .track_focus(&self.focus_handle)
            .id("details-panel")
            .role(Role::Group)
            .aria_label("Details")
            // The editor propagates Escape rather than consuming it, so discard the edit here. With
            // no editor open the key isn't this action at all — the panel's own `escape` binding
            // makes it `table::Deselect`, which is what replaces the Clear button the bundle used
            // to carry.
            // An action stops propagating by default, so an Escape this panel has no edit to
            // discard has to be handed back explicitly — otherwise it dies here instead of
            // reaching whatever else was listening.
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.editing.take().is_some() {
                    this.focus_handle.focus(window, cx);
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .child(
                div().size_full().min_h_0().child(
                    v_resizable("details-split")
                        .on_resize(|state, _, cx| {
                            if cx.has_global::<settings::project::CurrentProject>()
                                && let Some(height) = state.read(cx).sizes().first().copied()
                            {
                                settings::project::CurrentProject::set_text(
                                    IMAGE_PANE_HEIGHT_KEY,
                                    format!("{}", f32::from(height)).into(),
                                    cx,
                                );
                            }
                        })
                        // Dropped entirely in the gallery: the cards are already showing this photo,
                        // so the pane is just less room for the fields. It comes back with the grid.
                        .when(!gallery, |split| {
                            split.child(
                                resizable_panel()
                                    .size(px(image_height))
                                    .size_range(px(80.)..px(600.))
                                    .p_3()
                                    // One item is a plain frame. Several are a stack of offset cards
                                    // with the front one live: the bundle keeps a slot per item —
                                    // including an item with no file, which shows its placeholder
                                    // rather than being skipped — so stepping through is a walk over
                                    // the selection, not over the subset that happens to have photos.
                                    .map(|pane| match count > 1 {
                                        false => pane.child(render_image_frame(
                                            image_path,
                                            self.caption.clone(),
                                            transport,
                                            cx,
                                        )),
                                        true => pane.child(
                                            div()
                                                .id("details-stack")
                                                .relative()
                                                .size_full()
                                                .on_hover(cx.listener(
                                                    |this, over: &bool, _, cx| {
                                                        this.stack_hover = *over;
                                                        cx.notify();
                                                    },
                                                ))
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .left(px(14.))
                                                        .right_0()
                                                        .top(px(10.))
                                                        .bottom_0()
                                                        .rounded(cx.theme().radius)
                                                        .border_1()
                                                        .border_color(cx.theme().border)
                                                        .bg(cx.theme().muted),
                                                )
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .left(px(7.))
                                                        .right(px(7.))
                                                        .top(px(5.))
                                                        .bottom(px(5.))
                                                        .rounded(cx.theme().radius)
                                                        .border_1()
                                                        .border_color(cx.theme().border)
                                                        .bg(cx.theme().background),
                                                )
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .left_0()
                                                        .right(px(14.))
                                                        .top_0()
                                                        .bottom(px(10.))
                                                        .child(render_image_frame(
                                                            image_path,
                                                            self.caption.clone(),
                                                            transport,
                                                            cx,
                                                        ))
                                                        .when(self.stack_hover, |front| {
                                                            front
                                                                .child(step(
                                                                    "details-stack-prev",
                                                                    true,
                                                                    cx.listener(
                                                                        |this, _, _, cx| {
                                                                            this.step_stack(
                                                                                false, cx,
                                                                            )
                                                                        },
                                                                    ),
                                                                    cx,
                                                                ))
                                                                .child(step(
                                                                    "details-stack-next",
                                                                    false,
                                                                    cx.listener(
                                                                        |this, _, _, cx| {
                                                                            this.step_stack(
                                                                                true, cx,
                                                                            )
                                                                        },
                                                                    ),
                                                                    cx,
                                                                ))
                                                        })
                                                        .child(
                                                            div()
                                                                .absolute()
                                                                .bottom_1()
                                                                .right_1()
                                                                .px_1p5()
                                                                .py_0p5()
                                                                .rounded(cx.theme().radius)
                                                                .bg(cx.theme().background)
                                                                .text_xs()
                                                                .text_color(cx.theme().foreground)
                                                                .child(format!(
                                                                    "{} of {count}",
                                                                    self.stack.min(count - 1) + 1
                                                                )),
                                                        ),
                                                ),
                                        ),
                                    }),
                            )
                        })
                        .child(
                            // `pr_2` on the panel insets the scrollbar from the resize edge so dragging it doesn't catch.
                            resizable_panel().pr_2().child(
                                // This wrapper does not scroll, and that is the whole point: it is the
                                // rect the floating editor grows within and is clamped to, so a long
                                // value wraps inside the visible list rather than over the preview or
                                // the grid. `overflow_y_scrollbar` makes the div it is called on the
                                // scrolled *content* (auto height, sliding under the viewport), so
                                // measuring there would hand the editor a rect taller than the panel.
                                // Same arrangement the grid uses for its cell editor.
                                // A flex column, not a plain block: the bundle heading below is a
                                // sibling of the scrolling list, and with `size_full` on both the list
                                // ran the heading's height past the bottom of the panel and cut its
                                // last field row in half.
                                v_flex()
                                    .size_full()
                                    .min_h_0()
                                    .relative()
                                    .child({
                                        let viewport = self.viewport.clone();
                                        canvas(
                                            move |bounds, _, _| viewport.set(bounds),
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .size_full()
                                    })
                                    // Says how many items the fields below speak for, and warns that
                                    // typing into one writes down the whole bundle — before the edit,
                                    // not after it.
                                    .when(count > 1, |list| {
                                        list.child(
                                            div()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .px_3()
                                                .pt_2()
                                                .pb_1p5()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("{count} items · shared fields"))
                                                .child("edits apply to all"),
                                        )
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .w_full()
                                            // `min_h_0` overrides flex `min-height: auto` so this scrolls instead of growing past the panel.
                                            .min_h_0()
                                            .overflow_y_scrollbar()
                                            .px_3()
                                            // Clear of the resize handle above, so the first field
                                            // doesn't sit flush against it.
                                            .pt_2()
                                            // Pad by the bottom-strip crop (29px closed / 0 open) so it doesn't eat the last field row.
                                            .pb(px(12.) + crop)
                                            .child(
                                                div()
                                                    .rounded(cx.theme().radius)
                                                    .overflow_hidden()
                                                    .children(rows),
                                            ),
                                    )
                                    .children(self.field_editor(window, cx)),
                            ),
                        )
                        // Last, along the bottom: the fields are what the panel is for, and a note is
                        // commentary on them. Reading order puts the thing before what is said about
                        // it, and it keeps the fields anchored under the photo as the notes grow.
                        //
                        .when_some(notes, |split, notes| {
                            split.child(
                                resizable_panel()
                                    .size(px(180.))
                                    .size_range(notes_range)
                                    .child(notes),
                            )
                        }),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // No `use super::*`: chain-globbing `gpui::*` shadows the built-in `#[test]` and recurses (see CLAUDE.md).
    use std::path::{Path, PathBuf};

    use gpui::{Context, IntoElement, Render, TestAppContext, Window};

    use gpui::VisualTestContext;

    use super::{DetailsPanel, render_image_frame};

    /// Wraps `render_image_frame` in a root `Render` view so a test can actually draw it —
    /// `Img`'s real load/fallback logic runs during layout/paint, not at element construction,
    /// so building the element tree alone (without a window draw) wouldn't exercise it.
    struct ImageFrameProbe(Option<PathBuf>);

    impl Render for ImageFrameProbe {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let caption = self.0.as_deref().and_then(preview::describe);
            render_image_frame(self.0.clone(), caption, None, cx)
        }
    }

    fn sample_photo(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sample/photos")
            .join(name)
            .canonicalize()
            .expect("sample/photos present in repo")
    }

    #[gpui::test]
    fn renders_a_resolved_image_without_panicking(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let path = sample_photo("1.jpg");
        cx.add_window_view(|_, _| ImageFrameProbe(Some(path)));
    }

    #[gpui::test]
    fn falls_back_when_the_resolved_path_is_missing_on_disk(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let path = PathBuf::from("/nonexistent/qrate-test-image.jpg");
        cx.add_window_view(|_, _| ImageFrameProbe(Some(path)));
    }

    /// The captioned case: a file nothing in the ladder can draw. It has to reach paint with the
    /// icon *and* the caption, since the caption is built from a `fs::metadata` call that a
    /// missing or unreadable file makes fail.
    #[gpui::test]
    fn a_file_with_no_preview_draws_its_icon_and_its_details(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let path = std::env::temp_dir().join("qrate-details-caption.sqlite");
        std::fs::write(&path, vec![0u8; 2048]).unwrap();
        cx.add_window_view(|_, _| ImageFrameProbe(Some(path.clone())));

        // And with the file gone, so only the extension is left to say anything.
        std::fs::remove_file(&path).unwrap();
        cx.add_window_view(|_, _| ImageFrameProbe(Some(path)));
    }

    #[gpui::test]
    fn renders_the_no_image_placeholder_without_a_path(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(|_, _| ImageFrameProbe(None));
    }

    #[gpui::test]
    fn details_panel_shows_no_selection_without_a_table(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            // `render` reads the view mode through `settings`, which `main` initializes before
            // any window exists — so a panel with no settings at all is not a state the app has.
            cx.set_global(settings::AppSettings::default());
        });
        // No `TableStateHandle` global set — `DetailsPanel::bind` finds nothing, so `render`
        // takes the "No selection" branch. Mirrors dev-launch-with-no-project.
        cx.add_window_view(DetailsPanel::new);
    }

    /// A real table behind the panel, with autosave off so a committed edit doesn't write the
    /// temp project file. Same shape as `table::delegate`'s own fixture.
    fn project_with_table(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            let mut app = settings::AppSettings::default();
            app.values.insert(
                settings::AUTOSAVE_KEY.into(),
                settings::Val::Text("off".into()),
            );
            cx.set_global(app);
            cx.set_global(settings::project::CurrentProject {
                file: std::env::temp_dir().join("qrate-details-edit.qrate"),
                data: settings::project::ProjectData {
                    name: "T".into(),
                    columns: Vec::new(),
                    headers: vec!["Medium".into(), "Title".into()],
                    rows: vec![
                        vec!["Film".into(), "one".into()],
                        vec!["Video".into(), "two".into()],
                        // Shares a Medium with row 0 but not a Title, so a selection of the two
                        // has one agreed field and one mixed.
                        vec!["Film".into(), "three".into()],
                    ],
                    row_ids: vec![1, 2, 3],
                    values: Default::default(),
                },
            });
        });
        cx.add_window_view(table::TablePanel::new);
    }

    /// A bundle reports what its items agree on and counts what they don't, and typing into a
    /// mixed field sets it on every one of them — as a single undo step, so a bulk edit is one
    /// mistake to take back rather than five.
    #[gpui::test]
    fn a_mixed_field_edits_every_selected_item_as_one_step(cx: &mut TestAppContext) {
        project_with_table(cx);
        let state = cx.update(|cx| {
            cx.try_global::<table::TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .expect("the table panel publishes its state handle")
        });
        let (panel, cx) = cx.add_window_view(DetailsPanel::new);
        // Source rows 0 and 2: both Film, titled "one" and "three".
        state.update(cx, |state, cx| {
            state.delegate_mut().select_only_row(0);
            state.delegate_mut().toggle_row(2);
            cx.notify();
        });

        panel.update_in(cx, |panel, window, cx| {
            let fields = state.read(cx).delegate();
            let fields = super::shared_fields(fields, &panel.picked(cx));
            assert_eq!(
                fields,
                vec![
                    ("Medium".into(), "Film".into(), false),
                    ("Title".into(), "Mixed (2 values)".into(), true),
                ]
            );

            panel.edit_field(&"Title".into(), &"".into(), window, cx);
            assert_eq!(
                panel.editing,
                Some((vec![0, 2], 1)),
                "the write is aimed at both selected items"
            );
            panel
                .editor
                .update(cx, |editor, cx| editor.set_value("retitled", window, cx));
            panel.commit(cx);
        });

        let titles = |cx: &mut VisualTestContext| {
            state.read_with(cx, |s, _| {
                (0..3)
                    .map(|row| s.delegate().cell(row, 1).cloned().unwrap_or_default())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(titles(cx), vec!["retitled", "two", "retitled"]);

        // One undo, not two: the batch is what makes a bundle edit safe to try.
        cx.update(|_, cx| table::history_step(false, cx));
        assert_eq!(titles(cx), vec!["one", "two", "three"]);
    }

    /// The DoD: a field edited in the panel lands in the grid, and undo — the grid's own history,
    /// which the panel must not have bypassed — puts it back. Also pins the name→column lookup:
    /// each field must write its own column, not the one at its position in the list.
    #[gpui::test]
    fn editing_a_field_writes_the_grid_and_undoes(cx: &mut TestAppContext) {
        project_with_table(cx);
        let state = cx.update(|cx| {
            cx.try_global::<table::TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .expect("the table panel publishes its state handle")
        });
        let (panel, cx) = cx.add_window_view(DetailsPanel::new);
        // Select source row 1 ("Video", "two"), as clicking that cell in the grid would.
        state.update(cx, |state, cx| state.set_selected_cell(1, 2, cx));

        panel.update_in(cx, |panel, window, cx| {
            panel.edit_field(&"Title".into(), &"two".into(), window, cx);
            assert_eq!(panel.editing, Some((vec![1], 1)), "Title is data column 1");
            panel.editor.update(cx, |editor, cx| {
                editor.set_value("two, revised", window, cx)
            });
            panel.commit(cx);

            panel.edit_field(&"Medium".into(), &"Video".into(), window, cx);
            assert_eq!(panel.editing, Some((vec![1], 0)), "Medium is data column 0");
            panel.editing = None;
        });

        let title = |cx: &mut VisualTestContext| {
            state.read_with(cx, |s, _| s.delegate().cell(1, 1).cloned())
        };
        assert_eq!(title(cx).unwrap_or_default(), "two, revised");

        cx.update(|_, cx| table::history_step(false, cx));
        assert_eq!(
            title(cx).unwrap_or_default(),
            "two",
            "the panel wrote through the grid's own history"
        );
    }

    /// The rect the floating editor is clamped to must be the *visible* field list, not the
    /// scrolled content. Measuring on the div `overflow_y_scrollbar` is called on gets the content
    /// instead — auto-height, taller than the window — and the editor then clamps to nothing,
    /// painting over the preview above it and growing off the bottom of the screen.
    #[gpui::test]
    fn the_editor_is_clamped_to_the_visible_list_not_the_scrolled_content(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::project::CurrentProject {
                file: std::env::temp_dir().join("qrate-details-viewport.qrate"),
                data: settings::project::ProjectData {
                    name: "T".into(),
                    columns: Vec::new(),
                    // Far more fields than fit, so the list genuinely scrolls.
                    headers: (0..80).map(|i| format!("field_{i}")).collect(),
                    rows: vec![(0..80).map(|i| format!("value {i}")).collect()],
                    row_ids: vec![1],
                    values: Default::default(),
                },
            });
        });
        cx.add_window_view(table::TablePanel::new);
        let state = cx.update(|cx| {
            cx.try_global::<table::TableStateHandle>()
                .and_then(|h| h.0.upgrade())
                .expect("the table panel publishes its state handle")
        });
        let (panel, cx) = cx.add_window_view(DetailsPanel::new);
        state.update(cx, |state, cx| state.set_selected_cell(0, 1, cx));
        cx.run_until_parked();
        cx.draw(
            gpui::point(gpui::px(0.), gpui::px(0.)),
            gpui::size(gpui::px(400.), gpui::px(600.)),
            |_, _| gpui::div(),
        );

        let window_height = cx.update(|window, _| window.viewport_size().height);
        let measured = panel.read_with(cx, |panel, _| panel.viewport.get());
        assert!(
            measured.size.height > gpui::px(0.),
            "the viewport canvas never measured"
        );
        assert!(
            measured.size.height <= window_height,
            "the editor's clamp rect is {:?} tall in a {window_height:?} window — it measured the \
             scrolled content, not the visible list",
            measured.size.height
        );
    }
}

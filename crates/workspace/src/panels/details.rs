use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    dock::{DockPlacement, Panel, PanelControl, PanelEvent},
    input::{Escape, InputEvent, InputState},
    resizable::{resizable_panel, v_resizable},
    scroll::ScrollableElement,
    table::TableState,
};
use preview::{can_preview, thumb};
use table::{QrateTableDelegate, Selection, TableChanged, TableStateHandle};

use crate::BottomDockCrop;
use crate::panel_registry::PanelMeta;

/// Project-scoped height of the details panel's image pane, in pixels.
const IMAGE_PANE_HEIGHT_KEY: &str = "details_image_height";

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
    /// `(source_row, data_col)` of the field being edited, in the grid's own coordinates so a
    /// filter change between opening and committing can't redirect the write.
    editing: Option<(usize, usize)>,
    /// Commits the open field on Enter or when the editor loses focus.
    _editor_sub: Subscription,
    /// Window-space rect of the field row being edited, and of the scrolling field list the
    /// editor is confined to. Written from `canvas` prepaint, which only gets an `&mut App` —
    /// a shared cell is how the measurement reaches the next render without a global.
    anchor: Rc<Cell<Option<Bounds<Pixels>>>>,
    viewport: Rc<Cell<Bounds<Pixels>>>,
}

impl DetailsPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _handle_sub = cx.observe_global::<TableStateHandle>(|this: &mut Self, cx| {
            this.bind(cx);
            cx.notify();
        });
        let editor = cx.new(|cx| InputState::new(window, cx));
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
            _editor_sub,
            anchor: Rc::default(),
            viewport: Rc::new(Cell::new(Bounds::default())),
        };
        this.bind(cx);
        this
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
        let located = self.state.as_ref().and_then(|w| w.upgrade()).and_then(|s| {
            let delegate = s.read(cx).delegate();
            let row = match delegate.selection()? {
                Selection::Cell { row, .. } | Selection::Row(row) => row,
                Selection::Column(_) => return None,
            };
            Some((row, delegate.data_col(header)?))
        });
        let Some(at) = located else {
            log::warn!("details: no column named {header} to edit");
            return;
        };
        self.editing = Some(at);
        // Re-measured for the new field; the box renders on the frame after the capture.
        self.anchor.set(None);
        self.editor.update(cx, |editor, cx| {
            editor.set_value(value.clone(), window, cx);
            editor.focus(window, cx);
        });
        cx.notify();
    }

    /// Write the open field back through the grid's own mutation path, so dirty-tracking,
    /// validation and undo stay single-sourced. Clearing `editing` first keeps the `TableChanged`
    /// this provokes from re-entering as a second commit.
    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some((row, col)) = self.editing.take() else {
            return;
        };
        let value = self.editor.read(cx).value().clone();
        table::write_cell(row, col, value, cx);
        cx.notify();
    }

    /// The floating field editor — the grid's own box, anchored over the field it edits and
    /// clamped to the field list. `None` until the field has measured itself, one frame after the
    /// click.
    fn field_editor(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.editing?;
        let anchor = self.anchor.get()?;
        let (box_el, _) = table::editor_box(&self.editor, anchor, self.viewport.get(), window, cx);
        Some(
            deferred(table::floating::float_at(
                anchor.origin,
                self.viewport.get(),
                box_el,
            ))
            .into_any_element(),
        )
    }

    fn bind(&mut self, cx: &mut Context<Self>) {
        self.state = cx.try_global::<TableStateHandle>().map(|h| h.0.clone());
        self._table_sub =
            self.state.as_ref().and_then(|w| w.upgrade()).map(|entity| {
                cx.subscribe(&entity, |_this, _st, _ev: &TableChanged, cx| cx.notify())
            });
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
fn render_image_frame(image_path: Option<PathBuf>, cx: &App) -> AnyElement {
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
                    .bg(cx.theme().background.opacity(0.7))
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
                                    crate::open_image_viewer(
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
        .into_any_element()
}

impl Render for DetailsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.state.as_ref().and_then(|w| w.upgrade()).and_then(|s| {
            let state = s.read(cx);
            let row = match state.delegate().selection()? {
                Selection::Cell { row, .. } | Selection::Row(row) => row,
                // A whole-column selection has no single row to detail.
                Selection::Column(_) => return None,
            };
            let delegate = state.delegate();
            let image = delegate.row_image(row).map(Path::to_path_buf);
            Some((delegate.row_fields(row), image))
        });

        let crop = cx.try_global::<BottomDockCrop>().map_or(px(0.), |c| c.0);
        // The gallery is already a wall of the same thumbnails, so this panel's own copy is
        // redundant there — read from the setting `views::switch` writes, which is also what
        // survives a relaunch, rather than reaching into the centre panel.
        let gallery = crate::ViewMode::parse(&settings::effective_text(crate::VIEW_MODE_KEY, cx))
            == crate::ViewMode::Gallery;

        let Some((fields, image_path)) = selection.filter(|(f, _)| !f.is_empty()) else {
            return div()
                .size_full()
                .p_3()
                .text_color(cx.theme().muted_foreground)
                .child("No selection")
                .into_any_element();
        };

        // Hand-built attribute list, not `DescriptionList`/`DataTable`: the fields are fixed pairs,
        // and it reads as a list rather than a second grid — alternating rows carry the structure,
        // no borders.
        let editing_col = self.editing.map(|(_, col)| col);
        let rows = fields.into_iter().enumerate().map(|(ix, (k, v))| {
            let open = self
                .state
                .as_ref()
                .and_then(|w| w.upgrade())
                .and_then(|s| s.read(cx).delegate().data_col(&k))
                .is_some_and(|col| editing_col == Some(col));
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
                        .on_click(cx.listener({
                            let (k, v) = (k.clone(), v.clone());
                            move |this, _, window, cx| this.edit_field(&k, &v, window, cx)
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
            // The editor propagates Escape rather than consuming it, so discard the edit here.
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.editing.take().is_some() {
                    this.focus_handle.focus(window, cx);
                    cx.notify();
                }
            }))
            .child(
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
                                .child(render_image_frame(image_path, cx)),
                        )
                    })
                    .child(
                        // `pr_2` on the panel insets the scrollbar from the resize edge so dragging it doesn't catch.
                        resizable_panel().pr_2().child(
                            div()
                                .size_full()
                                // `min_h_0` overrides flex `min-height: auto` so this scrolls instead of growing past the panel.
                                .min_h_0()
                                .relative()
                                .overflow_y_scrollbar()
                                .px_3()
                                // Clear of the resize handle above, so the first field doesn't sit
                                // flush against it.
                                .pt_2()
                                // Pad by the bottom-strip crop (29px closed / 0 open) so it doesn't eat the last field row.
                                .pb(px(12.) + crop)
                                // The rect the floating editor grows within and is clamped to, so a
                                // long value wraps inside this panel rather than over the grid.
                                .child({
                                    let viewport = self.viewport.clone();
                                    canvas(
                                        move |bounds, _, _| viewport.set(bounds),
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full()
                                })
                                .child(
                                    div()
                                        .rounded(cx.theme().radius)
                                        .overflow_hidden()
                                        .children(rows),
                                )
                                .children(self.field_editor(window, cx)),
                        ),
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
            render_image_frame(self.0.clone(), cx)
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
                    ],
                    values: Default::default(),
                },
            });
        });
        cx.add_window_view(table::TablePanel::new);
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
            assert_eq!(panel.editing, Some((1, 1)), "Title is data column 1");
            panel.editor.update(cx, |editor, cx| {
                editor.set_value("two, revised", window, cx)
            });
            panel.commit(cx);

            panel.edit_field(&"Medium".into(), &"Video".into(), window, cx);
            assert_eq!(panel.editing, Some((1, 0)), "Medium is data column 0");
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
}

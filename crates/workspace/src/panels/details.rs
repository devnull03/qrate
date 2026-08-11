use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    dock::{DockPlacement, Panel, PanelControl, PanelEvent},
    input::{Escape, Input, InputEvent, InputState},
    resizable::{resizable_panel, v_resizable},
    scroll::ScrollableElement,
    table::TableState,
};
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

/// Whether `gpui`'s `img()` can decode this path. Anything else gets an icon instead of a
/// failed load + fallback, which also keeps the placeholder honest about *what* it stands for.
/// Matches gpui's `image_cache` decoders (`ImageFormat` + svg); extension-only, like the rest of
/// this app's file handling (`table::photos` resolves rows by filename too) — sniffing magic
/// bytes would mean reading every selected file off disk to pick an icon.
fn is_previewable_image(path: &Path) -> bool {
    matches!(
        extension(path).as_deref(),
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "svg")
    )
}

fn extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

/// Placeholder icon for a file we can't render inline. `gpui_component`'s bundled icon set has
/// no media glyphs (no camera/film/music), so these are the nearest stand-ins available —
/// swap in custom SVGs via `Icon::path` if the set ever grows.
///
/// ponytail: four buckets, extension-keyed. Add a real mime crate only if the icon actually
/// needs to be right for files with no/wrong extension.
fn placeholder_icon(path: Option<&Path>) -> IconName {
    match path.and_then(extension).as_deref() {
        Some("pdf" | "epub" | "doc" | "docx" | "txt" | "md") => IconName::BookOpen,
        Some("mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "aiff") => IconName::Play,
        Some("mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v") => IconName::Frame,
        _ => IconName::File,
    }
}

/// The image frame: the selected row's photo if one resolved and is a decodable image, else a
/// muted placeholder icon chosen from the file's extension — same rounded/bordered box either
/// way. Whenever a path resolved at all, two ghost icon buttons overlay the top-right: open in
/// the OS default viewer, reveal in the file manager (plain shell-outs, no panel state touched).
/// Those matter *most* for the icon case, where the OS is the only way to actually see the file.
/// `gpui`'s image cache is keyed by resource path, so switching back to a previously-viewed
/// row's image is served from cache rather than re-decoded from disk.
///
/// Returns `AnyElement` (erased), not `impl IntoElement` — this is called from several `Render`
/// impls (the real panel plus test probes), and gpui's chained builder calls produce deeply
/// nested generic types; propagating that concrete type into every caller overflowed rustc's
/// stack during type-checking instead of just hitting a slow compile.
fn render_image_frame(image_path: Option<PathBuf>, cx: &App) -> AnyElement {
    // Also `img()`'s decode-failure fallback; captures by value because the `'static` fallback
    // closure can't borrow `cx` or `image_path`.
    let placeholder = {
        let color = cx.theme().muted_foreground;
        let icon = placeholder_icon(image_path.as_deref());
        move || {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(color)
                // `IconName` isn't `Copy`, and `with_fallback` wants `Fn` (it may re-render) —
                // so clone per call rather than move the captured icon out.
                .child(Icon::new(icon.clone()).size_6())
                .into_any_element()
        }
    };
    let show_image = image_path.as_deref().is_some_and(is_previewable_image);

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
            Some(path) => frame
                .map(|frame| {
                    if show_image {
                        // gpui's `img` stamps the element with the *image's* aspect ratio, so a
                        // `size_full` img ignores the frame shape and `object_fit` has nothing to
                        // letterbox. Size it by its intrinsic ratio under `max_w/h_full` (where that
                        // aspect logic applies) and center it, so it shrinks to fit — bars and all.
                        frame.child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    img(path.clone())
                                        .max_w_full()
                                        .max_h_full()
                                        .object_fit(ObjectFit::Contain)
                                        .with_fallback(placeholder),
                                ),
                        )
                    } else {
                        frame.child(placeholder())
                    }
                })
                .child(
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
                                        crate::open_image_viewer(path.clone(), cx)
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
            None => frame.child(placeholder()),
        })
        .into_any_element()
}

impl Render for DetailsPanel {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    div()
                        .w(px(110.))
                        .flex_shrink_0()
                        .px_2()
                        .py_1p5()
                        .text_color(cx.theme().muted_foreground)
                        .child(k.clone()),
                )
                // `min_w_0` overrides flex `min-width: auto` so the value wraps instead of overflowing right.
                .child(match open {
                    true => div()
                        .flex_1()
                        .min_w_0()
                        .px_1()
                        .py_0p5()
                        .child(Input::new(&self.editor).xsmall())
                        .into_any_element(),
                    // Plain text rather than `TextView`, which parses markdown/html and mangles
                    // raw metadata. Click opens the editor; copy is Ctrl+C once it's open.
                    false => div()
                        .id(ix)
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .py_1p5()
                        .cursor_text()
                        .on_click(cx.listener({
                            let (k, v) = (k, v.clone());
                            move |this, _, window, cx| this.edit_field(&k, &v, window, cx)
                        }))
                        .child(v)
                        .into_any_element(),
                })
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
                    .child(
                        resizable_panel()
                            .size(px(image_height))
                            .size_range(px(80.)..px(600.))
                            .p_3()
                            .child(render_image_frame(image_path, cx)),
                    )
                    .child(
                        // `pr_2` on the panel insets the scrollbar from the resize edge so dragging it doesn't catch.
                        resizable_panel().pr_2().child(
                            div()
                                .size_full()
                                // `min_h_0` overrides flex `min-height: auto` so this scrolls instead of growing past the panel.
                                .min_h_0()
                                .overflow_y_scrollbar()
                                .px_3()
                                // Pad by the bottom-strip crop (29px closed / 0 open) so it doesn't eat the last field row.
                                .pb(px(12.) + crop)
                                .child(
                                    div()
                                        .rounded(cx.theme().radius)
                                        .overflow_hidden()
                                        .children(rows),
                                ),
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
    use gpui_component::{IconName, IconNamed as _};

    use gpui::VisualTestContext;

    use super::{DetailsPanel, is_previewable_image, placeholder_icon, render_image_frame};

    #[test]
    fn placeholder_icon_varies_by_file_type() {
        // `IconName` is macro-generated and implements neither `PartialEq` nor `Debug`, so
        // compare the embedded asset paths it resolves to instead.
        let icon = |p: &str| placeholder_icon(Some(Path::new(p))).path();
        assert_eq!(icon("/f/scan.PDF"), IconName::BookOpen.path());
        assert_eq!(icon("/f/take.mp3"), IconName::Play.path());
        assert_eq!(icon("/f/clip.mov"), IconName::Frame.path());
        assert_eq!(icon("/f/archive.zip"), IconName::File.path());
        assert_eq!(icon("/f/no-extension"), IconName::File.path());
        assert_eq!(placeholder_icon(None).path(), IconName::File.path());
        // The buckets must actually differ — a regression that collapsed them all to `File`
        // would otherwise still pass every assertion above.
        assert_ne!(icon("/f/scan.pdf"), icon("/f/take.mp3"));
    }

    #[test]
    fn only_decodable_images_get_an_inline_preview() {
        assert!(is_previewable_image(Path::new("/f/a.JPG")));
        assert!(is_previewable_image(Path::new("/f/a.png")));
        // Resolves via the files folder like any other row file, but `img()` can't decode it —
        // it gets an icon instead of a failed load.
        assert!(!is_previewable_image(Path::new("/f/a.pdf")));
        assert!(!is_previewable_image(Path::new("/f/a")));
    }

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
        cx.update(gpui_component::init);
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

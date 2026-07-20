use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    dock::{Panel, PanelControl, PanelEvent},
    resizable::{resizable_panel, v_resizable},
    scroll::ScrollableElement,
    table::TableState,
};
use table::{QrateTableDelegate, Selection, TableChanged, TableStateHandle};

use crate::BottomDockCrop;

/// Project-scoped height of the details panel's image pane, in pixels.
const IMAGE_PANE_HEIGHT_KEY: &str = "details_image_height";

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
}

impl DetailsPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _handle_sub = cx.observe_global::<TableStateHandle>(|this: &mut Self, cx| {
            this.bind(cx);
            cx.notify();
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            state: None,
            _handle_sub,
            _table_sub: None,
            _crop_sub: cx.observe_global::<BottomDockCrop>(|_this: &mut Self, cx| cx.notify()),
        };
        this.bind(cx);
        this
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
                    eprintln!("image action {id} failed: {err}");
                }
            })
    };

    // `h_full`, not a fixed height: the caller sizes the frame (a resizable panel), so dragging
    // the split re-letterboxes the photo instead of stretching or cropping it.
    div()
        .relative()
        .size_full()
        // Flex item in `resizable_panel`: without this its min-content width floors at the
        // photo's intrinsic pixels, so the frame never narrows and `Contain` never re-fits the
        // width. The horizontal twin of the `min_h_0` the fields column relies on below.
        .min_w_0()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .bg(cx.theme().muted)
        .map(|frame| match image_path {
            Some(path) => frame
                .map(|frame| {
                    if show_image {
                        // `rounded` goes on the `img` itself, not just the frame: gpui's
                        // overflow mask is a plain rect (`Style::overflow_mask`), so
                        // `overflow_hidden` above clips square and a full-bleed photo would
                        // paint over the frame's rounded corners. `Img` reads its *own*
                        // corner radii when painting, which is what actually rounds it.
                        frame.child(
                            img(path.clone())
                                .size_full()
                                .rounded(cx.theme().radius)
                                .object_fit(ObjectFit::Contain)
                                .with_fallback(placeholder),
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
                                    .on_click(move |_, window, cx| {
                                        open_image_viewer(path.clone(), window, cx)
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

/// Full-window image overlay contents: the photo (fit-to-window, then zoomable/pannable) with the
/// filename top-left and zoom/close controls top-right. A stateful view because the transform has
/// to survive the dialog's re-renders — the content builder runs every frame, so a plain closure
/// couldn't hold accumulated zoom/offset.
struct ImageViewer {
    path: PathBuf,
    /// 1.0 = fit-to-window (`Contain`); scales up to 8×.
    zoom: f32,
    /// Pan translation from the centered position.
    offset: Point<Pixels>,
    /// Last pointer position while dragging; `None` when not panning.
    drag_from: Option<Point<Pixels>>,
}

impl ImageViewer {
    /// Clamp zoom and recenter once it's back within the frame.
    fn set_zoom(&mut self, zoom: f32) {
        // Down to 0.1 so zoom-out can shrink the image well past the initial fit, up to 8×.
        self.zoom = zoom.clamp(0.1, 8.0);
        // At or below fit the image is no larger than its frame, so there's nothing to pan to.
        if self.zoom <= 1.0 {
            self.offset = Point::default();
        }
    }
}

impl Render for ImageViewer {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (zoom, offset) = (self.zoom, self.offset);
        let name: SharedString = self
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Image")
            .to_string()
            .into();
        let pill = cx.theme().background.opacity(0.75);

        div()
            .id("image-viewer")
            .size_full()
            .relative()
            .overflow_hidden()
            // Any scroll zooms — covers a trackpad's pixel deltas, a wheel's line deltas, and
            // ctrl+scroll alike (drag is what pans, so scroll is free to mean zoom).
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let step = match ev.delta {
                    ScrollDelta::Lines(d) => d.y * 0.1,
                    ScrollDelta::Pixels(d) => f32::from(d.y) * 0.005,
                };
                this.set_zoom(this.zoom * (1.0 + step));
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    this.drag_from = Some(ev.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(last) = this.drag_from {
                    this.offset.x += ev.position.x - last.x;
                    this.offset.y += ev.position.y - last.y;
                    this.drag_from = Some(ev.position);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.drag_from = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        // `flex_shrink_0` is load-bearing: as a flex child the img would otherwise
                        // shrink back to the container on the main axis, cancelling every
                        // `relative(zoom)` past 1 — that was the "zoom does nothing" bug. `relative`
                        // position + `left`/`top` pan it as an offset from the centered position.
                        img(self.path.clone())
                            .flex_shrink_0()
                            .relative()
                            .w(relative(zoom))
                            .h(relative(zoom))
                            .left(offset.x)
                            .top(offset.y)
                            .object_fit(ObjectFit::Contain),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_2()
                    .left_2()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .bg(pill)
                    .child(name),
            )
            .child(
                div()
                    .absolute()
                    .top_2()
                    .right_2()
                    .flex()
                    .gap_1()
                    .p_1()
                    .rounded(cx.theme().radius)
                    .bg(pill)
                    .child(
                        Button::new("zoom-out")
                            .icon(IconName::Minus)
                            .ghost()
                            .small()
                            .tooltip("Zoom out")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_zoom(this.zoom / 1.25);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("zoom-in")
                            .icon(IconName::Plus)
                            .ghost()
                            .small()
                            .tooltip("Zoom in")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_zoom(this.zoom * 1.25);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("close-viewer")
                            .icon(IconName::Close)
                            .ghost()
                            .small()
                            .tooltip("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    }
}

/// Fraction of the viewport the viewer occupies, leaving a ~10% margin all around. The image
/// (`Contain`-fit) never exceeds this, so its larger edge matches the frame with breathing room.
const VIEWER_FRAME: f32 = 0.8;

/// Opens the photo in a transparent dialog overlay (~80% of the viewport, centered) with zoom/pan.
/// Esc or a click on the dimmed backdrop around it dismiss it; the transform lives in the
/// `ImageViewer` entity so it resets per open. The card is stripped to transparent so only the
/// image and its controls show over the backdrop.
fn open_image_viewer(path: PathBuf, window: &mut Window, cx: &mut App) {
    let viewer = cx.new(|_| ImageViewer {
        path,
        zoom: 1.0,
        offset: Point::default(),
        drag_from: None,
    });
    let clear = gpui::hsla(0., 0., 0., 0.);
    window.open_dialog(cx, move |dialog, window, _| {
        let vp = window.viewport_size();
        let viewer = viewer.clone();
        dialog
            .overlay(true)
            .overlay_closable(true)
            .close_button(false)
            .keyboard(true)
            // Transparent card, sized to 80% and centered (the forced `top(margin_top)` plus the
            // library's own horizontal centering give the ~10% margin on every side).
            .p_0()
            .bg(clear)
            .border_color(clear)
            .margin_top(vp.height * (1. - VIEWER_FRAME) / 2.)
            .w(vp.width * VIEWER_FRAME)
            .max_w(vp.width * VIEWER_FRAME)
            .content(move |content, _, _| {
                content
                    .w(vp.width * VIEWER_FRAME)
                    .h(vp.height * VIEWER_FRAME)
                    .child(viewer.clone())
            })
    });
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

        // A bordered two-column grid, not `DescriptionList` — the label/value list read as
        // free-floating text, and the fields *are* tabular. Hand-built rather than a second
        // `DataTable`: that would mean another `TableDelegate` + `TableState` entity to render
        // what is a fixed 2-column, no-sort, no-resize, no-header view of data the center
        // table's delegate already hands over as pairs (`QrateTableDelegate::row_fields`).
        // ponytail: revisit if these fields ever need sorting or inline editing.
        let border = cx.theme().border;
        let rows = fields.into_iter().enumerate().map(|(ix, (k, v))| {
            div()
                .flex()
                // `items_stretch`, not `items_start`: the label cell is one line tall, so with
                // top-align its `border_r` divider only spanned the first line and vanished down
                // the rest of a tall wrapped row. Stretching makes both cells fill the row height.
                .items_stretch()
                .border_b_1()
                .border_color(border)
                .when(ix % 2 == 1, |r| r.bg(cx.theme().muted.opacity(0.4)))
                .child(
                    div()
                        .w(px(110.))
                        .flex_shrink_0()
                        .px_2()
                        .py_1p5()
                        .border_r_1()
                        .border_color(border)
                        .text_color(cx.theme().muted_foreground)
                        .child(k),
                )
                // `min_w_0` lets the value shrink below its longest unbreakable token so the
                // text wraps instead of overflowing the row to the right (the "no line breaks"
                // bug). Without it, a flex item's `min-width: auto` pins it to min-content width.
                // Click-to-copy the whole value: `TextView` (the only selectable text) parses
                // markdown/html, which would mangle raw metadata, so a click beats drag-select.
                .child(
                    div()
                        .id(ix)
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .py_1p5()
                        .cursor_pointer()
                        .on_click({
                            let value = v.clone();
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()))
                            }
                        })
                        .child(v),
                )
        });

        // Split, not one scrolling column: the image lives in its own panel so it stays put
        // while only the fields scroll (the old single `overflow_y_scrollbar` div scrolled the
        // photo away), and the drag handle between them lets the user trade image height for
        // field rows instead of the photo distorting as the dock is resized.
        // `.size()` is the *initial* size only — once the user drags, `ResizableState` owns it,
        // so re-reading the persisted value each render is a restore, not a fight.
        let image_height = cx
            .try_global::<settings::project::CurrentProject>()
            .and_then(|p| p.data.values.get(IMAGE_PANE_HEIGHT_KEY))
            .and_then(|v| v.text().parse::<f32>().ok())
            .unwrap_or(180.);

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
                // `pr_2` on the panel, not the scroll content: it insets the whole scroll area
                // (scrollbar included) from the dock's right resize edge, so grabbing the edge to
                // resize doesn't catch the scrollbar.
                resizable_panel().pr_2().child(
                    div()
                        .size_full()
                        // Scrolls the fields alone. `min_h_0` is load-bearing: a flex child's
                        // default `min-height: auto` refuses to shrink below its content, so
                        // without it this box grows past the panel and the last rows fall off
                        // the bottom with no scrollbar to reach them — the reported bug.
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .px_3()
                        // Normal padding *plus* whatever the workspace's bottom-strip crop is
                        // clipping off this dock right now (29px while the bottom dock is
                        // closed, 0 while it's open) — otherwise the crop eats the last field
                        // row. Padding the scroll content, not the panel, so the extra space
                        // is scrollable-to rather than a dead gap.
                        .pb(px(12.) + crop)
                        .child(
                            div()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(border)
                                .overflow_hidden()
                                .children(rows),
                        ),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // No `use super::*` here: it would chain-glob `gpui::*`, whose `test` proc-macro shadows
    // the built-in `#[test]` that `#[gpui::test]`'s expansion emits — making the macro expand
    // into itself until rustc's recursion limit (and then its stack) blows.
    use std::path::{Path, PathBuf};

    use gpui::{Context, IntoElement, Render, TestAppContext, Window};
    use gpui_component::{IconName, IconNamed as _};

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
}

use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
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

        // Hand-built 2-column grid, not `DescriptionList`/`DataTable`: the fields are fixed, tabular pairs.
        // ponytail: revisit if these fields ever need sorting or inline editing.
        let border = cx.theme().border;
        let rows = fields.into_iter().enumerate().map(|(ix, (k, v))| {
            div()
                .flex()
                // `items_stretch` so the label cell's `border_r` divider spans the full (wrapped) row height.
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
                // `min_w_0` overrides flex `min-width: auto` so the value wraps instead of overflowing right.
                // Click-to-copy rather than drag-select: `TextView` parses markdown/html and mangles raw metadata.
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

        // Split so the image stays put while only the fields scroll, with a drag handle to trade heights.
        // `.size()` is the initial size only — once dragged, `ResizableState` owns it, so re-reading restores.
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
    // No `use super::*`: chain-globbing `gpui::*` shadows the built-in `#[test]` and recurses (see CLAUDE.md).
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

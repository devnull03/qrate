//! An image viewer overlay: the photo fit to whatever area it is mounted in, then zoomable and
//! pannable, with the filename and zoom/close controls.
//!
//! It is *not* a `gpui_component` dialog: a dialog's layer occludes the whole window, so it would
//! cover the custom title bar and steal its window controls. Instead the open viewer is held in
//! the [`ActiveImageViewer`] global and mounted as an overlay by whichever slot its [`Scope`]
//! names. The element itself is `absolute().size_full()` and takes no sizing input beyond its
//! padding, so the same viewer fills the whole workspace or just the centre panel unchanged.

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable as _, IconName, Sizable,
    button::{Button, ButtonVariants},
};

/// Which slot mounts the viewer. Two, because they answer different asks: the Details panel's
/// button means "show me this as big as the window allows", while a gallery card means "show me
/// this instead of the thumbnails" — the side panels stay readable beside it.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Over the whole dock area, below the title bar.
    Workspace,
    /// Over the centre panel only, leaving the docked panels visible.
    Centre,
}

/// The currently-open viewer, if any. Both mount slots observe this.
#[derive(Default)]
pub struct ActiveImageViewer(pub Option<Entity<ImageViewer>>);

impl Global for ActiveImageViewer {}

/// The open viewer, if it belongs in `scope`. `None` tells that slot to mount nothing — which is
/// how one global feeds two slots without ever painting itself twice.
pub fn viewer_in(scope: Scope, cx: &App) -> Option<Entity<ImageViewer>> {
    let viewer = cx.try_global::<ActiveImageViewer>()?.0.clone()?;
    (viewer.read(cx).scope == scope).then_some(viewer)
}

/// Opens `path` in the shared viewer overlay, replacing any viewer already open.
pub fn open_image_viewer(path: PathBuf, scope: Scope, cx: &mut App) {
    // Counted once, here: a PDF has to be opened to be counted, and doing that per frame would
    // re-parse the document on every repaint.
    let pages = preview::page_count(&path);
    let viewer = cx.new(|cx| ImageViewer {
        path,
        scope,
        page: 0,
        pages,
        zoom: 1.0,
        offset: Point::default(),
        drag_from: None,
        focus_handle: cx.focus_handle(),
        focused: false,
    });
    cx.set_global(ActiveImageViewer(Some(viewer)));
}

pub fn close_image_viewer(cx: &mut App) {
    cx.set_global(ActiveImageViewer(None));
}

pub struct ImageViewer {
    path: PathBuf,
    scope: Scope,
    /// Which page is shown, zero-based. Always 0 for anything that isn't a document.
    page: usize,
    /// How many pages there are, so the controls know their bounds. 1 means "not paged".
    pages: usize,
    /// 1.0 = fit-to-area (`Contain`); [`ImageViewer::set_zoom`] clamps it.
    zoom: f32,
    /// Pan translation from the centered position.
    offset: Point<Pixels>,
    /// Last pointer position while dragging; `None` when not panning.
    drag_from: Option<Point<Pixels>>,
    focus_handle: FocusHandle,
    /// Grabs focus on first render so Escape reaches [`Self`]; set once so we don't re-focus.
    focused: bool,
}

impl ImageViewer {
    /// Clamp zoom to [0.1, 8] — below 1 zooms out past the initial fit — and recenter once the
    /// image is no bigger than its frame, where there's nothing to pan to.
    fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.1, 8.0);
        if self.zoom <= 1.0 {
            self.offset = Point::default();
        }
    }

    /// Move `delta` pages, stopping at either end rather than wrapping — a document has a first
    /// and last page, and silently looping past them loses the reader's place.
    ///
    /// Zoom and pan reset with the turn: they were aimed at a detail of the page being left, and
    /// keeping them would land the next page off-screen at 8×.
    fn turn_page(&mut self, delta: isize) {
        let Some(page) = self.page.checked_add_signed(delta) else {
            return;
        };
        if page >= self.pages || page == self.page {
            return;
        }
        self.page = page;
        self.zoom = 1.0;
        self.offset = Point::default();
    }
}

impl Render for ImageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused {
            window.focus(&self.focus_handle, cx);
            self.focused = true;
        }
        let (zoom, offset, page, pages) = (self.zoom, self.offset, self.page, self.pages);
        let name: SharedString = self
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Image")
            .to_string()
            .into();
        let pill = cx.theme().background.opacity(0.8);

        div()
            .track_focus(&self.focus_handle)
            .id("image-viewer")
            // Fills the workspace overlay slot; `absolute` so it stacks over the dock content.
            .absolute()
            .size_full()
            .overflow_hidden()
            // Register an opaque hitbox over the whole overlay so clicks and scrolls land here
            // instead of falling through to the table painted behind it.
            .occlude()
            // Dim what's behind so the photo reads as the focus.
            .bg(cx.theme().background.opacity(0.9))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                match ev.keystroke.key.as_str() {
                    "escape" => close_image_viewer(cx),
                    // The keys anyone reading a document reaches for first. Harmless on a photo,
                    // where there is only ever one page to move between.
                    "left" | "pageup" => {
                        this.turn_page(-1);
                        cx.notify();
                    }
                    "right" | "pagedown" => {
                        this.turn_page(1);
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            // Any scroll zooms — trackpad pixel deltas, wheel line deltas, and ctrl+scroll alike;
            // drag is what pans, so scroll is free to mean zoom.
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
                // Padding is the only sizing input: the image `Contain`-fits the padded content
                // area at zoom 1, so it scales with the window/dock instead of a fixed fraction.
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_8()
                    .child(
                        // `flex_shrink_0` keeps `relative(zoom)` past 1; `relative` + `left`/`top` pan it.
                        //
                        // `FULL`, so zooming in reaches the file's real detail rather than
                        // magnifying a thumbnail — this is the one place that asks for no cap.
                        img(preview::source(&self.path, preview::FULL, page))
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
                    .top_4()
                    .left_4()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .bg(pill)
                    .child(name),
            )
            // Page controls, for a document only — a photo has nothing to page through, and an
            // always-present "1 / 1" would just be noise on every other file.
            .when(pages > 1, |viewer| {
                viewer.child(
                    div()
                        .absolute()
                        .bottom_4()
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .p_1()
                                .rounded(cx.theme().radius)
                                .bg(pill)
                                .child(
                                    Button::new("previous-page")
                                        .icon(IconName::ChevronLeft)
                                        .ghost()
                                        .small()
                                        .disabled(page == 0)
                                        .tooltip("Previous page")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.turn_page(-1);
                                            cx.notify();
                                        })),
                                )
                                // Numbered from one: the page count a reader sees has to match
                                // the one printed on the document.
                                .child(
                                    div()
                                        .px_1()
                                        .text_sm()
                                        .child(format!("{} / {pages}", page + 1)),
                                )
                                .child(
                                    Button::new("next-page")
                                        .icon(IconName::ChevronRight)
                                        .ghost()
                                        .small()
                                        .disabled(page + 1 >= pages)
                                        .tooltip("Next page")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.turn_page(1);
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .absolute()
                    .top_4()
                    .right_4()
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
                            .on_click(cx.listener(|_, _, _, cx| close_image_viewer(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use gpui::TestAppContext;

    use crate::image_viewer::{Scope, close_image_viewer, open_image_viewer, viewer_in};

    /// One global feeds two mount slots — the workspace overlay and the centre panel. Exactly one
    /// may claim it: drop the scope check and the viewer paints twice, once in each slot, with two
    /// sets of live controls over each other.
    #[gpui::test]
    fn only_the_slot_it_was_opened_for_mounts_the_viewer(cx: &mut TestAppContext) {
        let path = std::path::PathBuf::from("/nonexistent/qrate-scope-test.jpg");
        cx.update(|cx| {
            open_image_viewer(path.clone(), Scope::Centre, cx);
            assert!(viewer_in(Scope::Centre, cx).is_some());
            assert!(viewer_in(Scope::Workspace, cx).is_none());

            // Opening in the other scope replaces rather than stacks.
            open_image_viewer(path, Scope::Workspace, cx);
            assert!(viewer_in(Scope::Workspace, cx).is_some());
            assert!(viewer_in(Scope::Centre, cx).is_none());

            close_image_viewer(cx);
            assert!(viewer_in(Scope::Workspace, cx).is_none());
            assert!(viewer_in(Scope::Centre, cx).is_none());
        });
    }

    /// Paging has to stop at both ends. Wrapping past the last page loses the reader's place, and
    /// an underflow on page zero would panic on a `usize` subtraction.
    #[gpui::test]
    fn paging_stops_at_both_ends_and_resets_the_view(cx: &mut TestAppContext) {
        let path = std::path::PathBuf::from("/nonexistent/qrate-paging-test.pdf");
        cx.update(|cx| {
            open_image_viewer(path, Scope::Workspace, cx);
            let viewer = viewer_in(Scope::Workspace, cx).expect("just opened");

            viewer.update(cx, |viewer, _| {
                // A missing file reports one page, so give it a document to page through.
                viewer.pages = 3;

                viewer.turn_page(-1);
                assert_eq!(viewer.page, 0, "cannot go back from the first page");

                viewer.turn_page(1);
                assert_eq!(viewer.page, 1);

                // Zoom and pan belong to the page being left, not to the next one.
                viewer.set_zoom(4.0);
                viewer.offset = gpui::Point {
                    x: gpui::px(30.),
                    y: gpui::px(30.),
                };
                viewer.turn_page(1);
                assert_eq!(viewer.page, 2);
                assert!((viewer.zoom - 1.0).abs() < f32::EPSILON, "zoom reset");
                assert_eq!(viewer.offset, gpui::Point::default(), "pan reset");

                viewer.turn_page(1);
                assert_eq!(viewer.page, 2, "cannot go past the last page");
            });

            close_image_viewer(cx);
        });
    }

    /// A photo is a one-page document, so the controls stay hidden and the arrow keys do nothing.
    #[gpui::test]
    fn a_single_page_file_never_moves(cx: &mut TestAppContext) {
        let path = std::path::PathBuf::from("/nonexistent/qrate-single-page.jpg");
        cx.update(|cx| {
            open_image_viewer(path, Scope::Centre, cx);
            let viewer = viewer_in(Scope::Centre, cx).expect("just opened");
            viewer.update(cx, |viewer, _| {
                assert_eq!(
                    viewer.pages, 1,
                    "anything that isn't a document has one page"
                );
                viewer.turn_page(1);
                viewer.turn_page(-1);
                assert_eq!(viewer.page, 0);
            });
            close_image_viewer(cx);
        });
    }
}

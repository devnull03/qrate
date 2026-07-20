//! A full-screen-ish image viewer overlay: the photo fit to the content area, then zoomable and
//! pannable, with the filename and zoom/close controls.
//!
//! It is *not* a `gpui_component` dialog: a dialog's layer occludes the whole window, so it would
//! cover the custom title bar and steal its window controls. Instead the open viewer is held in
//! the [`ActiveImageViewer`] global and mounted by `Workspace` as an overlay over its own content
//! — which already sits below the title bar — so close/minimize stay reachable and the overlay
//! sizes itself to whatever area the workspace has rather than to fixed viewport fractions.

use std::path::PathBuf;

use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
};

/// The currently-open viewer, if any. `Workspace` observes this and mounts/unmounts the overlay.
#[derive(Default)]
pub struct ActiveImageViewer(pub Option<Entity<ImageViewer>>);

impl Global for ActiveImageViewer {}

/// Opens `path` in the shared viewer overlay, replacing any viewer already open.
pub fn open_image_viewer(path: PathBuf, cx: &mut App) {
    let viewer = cx.new(|cx| ImageViewer {
        path,
        zoom: 1.0,
        offset: Point::default(),
        drag_from: None,
        focus_handle: cx.focus_handle(),
        focused: false,
    });
    cx.set_global(ActiveImageViewer(Some(viewer)));
}

fn close(cx: &mut App) {
    cx.set_global(ActiveImageViewer(None));
}

pub struct ImageViewer {
    path: PathBuf,
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
}

impl Render for ImageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused {
            window.focus(&self.focus_handle, cx);
            self.focused = true;
        }
        let (zoom, offset) = (self.zoom, self.offset);
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
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    close(cx);
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
                        // `flex_shrink_0`: a flex child would otherwise shrink back to the container
                        // on the main axis, cancelling every `relative(zoom)` past 1. `relative`
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
                    .top_4()
                    .left_4()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .bg(pill)
                    .child(name),
            )
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
                            .on_click(cx.listener(|_, _, _, cx| close(cx))),
                    ),
            )
    }
}

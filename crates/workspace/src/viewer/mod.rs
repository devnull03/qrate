//! A fullscreen viewer overlay: whatever a row links to, fit to the area it is mounted in, then
//! zoomable and pannable — with paging, a find panel and on-page highlights for a document.
//!
//! It is *not* a `gpui_component` dialog: a dialog's layer occludes the whole window, so it would
//! cover the custom title bar and steal its window controls. Instead the open viewer is held in
//! the [`ActiveViewer`] global and mounted as an overlay by whichever slot its [`Scope`] names.
//! The element itself is `absolute().size_full()` and takes no sizing input beyond its padding, so
//! the same viewer fills the whole workspace or just the centre panel unchanged.
//!
//! The two neighbours hold the parts with rules in them: [`find`] owns the search state, and
//! [`highlight`] turns a hit's position on the page into a position on the screen.

mod find;
mod highlight;

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable as _, IconName, Selectable as _, Sizable, StyledExt as _,
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState},
    resizable::{ResizableState, h_resizable, resizable_panel},
};

use crate::viewer::find::Find;

/// How wide the find panel opens, and how far it may be dragged either way. The floor keeps a row
/// wide enough to show context around a match; the ceiling stops the panel swallowing the page.
const PANEL: Pixels = px(384.);
const PANEL_RANGE: std::ops::Range<Pixels> = px(240.)..px(720.);

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
pub struct ActiveViewer(pub Option<Entity<Viewer>>);

impl Global for ActiveViewer {}

/// The open viewer, if it belongs in `scope`. `None` tells that slot to mount nothing — which is
/// how one global feeds two slots without ever painting itself twice.
pub fn viewer_in(scope: Scope, cx: &App) -> Option<Entity<Viewer>> {
    let viewer = cx.try_global::<ActiveViewer>()?.0.clone()?;
    (viewer.read(cx).scope == scope).then_some(viewer)
}

/// Opens `path` in the shared viewer overlay, replacing any viewer already open.
pub fn open_viewer(path: PathBuf, scope: Scope, cx: &mut App) {
    // Counted once, here: a PDF has to be opened to be counted, and doing that per frame would
    // re-parse the document on every repaint.
    let pages = preview::page_count(&path);
    let document = preview::has_text(&path);
    let viewer = cx.new(|cx| Viewer {
        path,
        scope,
        page: 0,
        pages,
        document,
        zoom: 1.0,
        offset: Point::default(),
        drag_from: None,
        focus_handle: cx.focus_handle(),
        focused: false,
        find: Find::default(),
        find_open: false,
        split: cx.new(|_| ResizableState::default()),
    });
    cx.set_global(ActiveViewer(Some(viewer)));
}

pub fn close_viewer(cx: &mut App) {
    cx.set_global(ActiveViewer(None));
}

pub struct Viewer {
    path: PathBuf,
    scope: Scope,
    /// Which page is shown, zero-based. Always 0 for anything that isn't a document.
    page: usize,
    /// How many pages there are, so the controls know their bounds. 1 means "not paged".
    pages: usize,
    /// Whether this file is a document at all, which is a different question from whether it has
    /// more than one page — a one-page PDF is still a document, and still says "1 / 1".
    document: bool,
    /// 1.0 = fit-to-area (`Contain`); [`Viewer::set_zoom`] clamps it.
    zoom: f32,
    /// Pan translation from the centered position.
    offset: Point<Pixels>,
    /// Last pointer position while dragging; `None` when not panning.
    drag_from: Option<Point<Pixels>>,
    focus_handle: FocusHandle,
    /// Grabs focus on first render so Escape reaches [`Self`]; set once so we don't re-focus.
    focused: bool,
    find: Find,
    /// Whether the find panel is showing.
    find_open: bool,
    /// The split between the page and the find panel, owned by `gpui_component`'s resizable — it
    /// carries the drag handle, the sizing and the propagation rules, none of which are ours to
    /// reinvent.
    split: Entity<ResizableState>,
}

impl Viewer {
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
        self.show_page(page);
    }

    /// Go to `page`, resetting the view. Split out of [`Self::turn_page`] because a search hit
    /// lands on a page directly rather than by stepping to it, and must reset the same things.
    fn show_page(&mut self, page: usize) {
        self.page = page;
        self.zoom = 1.0;
        self.offset = Point::default();
    }

    /// Show the find panel, building its query box the first time.
    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = true;

        // Asked once, on first open: it opens the document, and the answer cannot change while
        // the viewer is up.
        if self.find.layered.is_none() {
            let path = self.path.clone();
            cx.spawn(async move |this, cx| {
                let layered = cx
                    .background_executor()
                    .spawn(async move { preview::has_text_layer(&path) })
                    .await;
                this.update(cx, |this, cx| {
                    this.find.layered = layered;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }

        let input = match self.find.input.clone() {
            Some(input) => input,
            None => {
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder("Find in document"));
                // Detached rather than held in a field: the subscription lives as long as the
                // input, and the input is owned by this viewer, so it cannot outlive us.
                cx.subscribe(&input, |this, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => this.run_search(cx),
                    InputEvent::PressEnter { shift, .. } => {
                        this.step_match(if *shift { -1 } else { 1 }, cx);
                    }
                    _ => {}
                })
                .detach();
                self.find.input = Some(input.clone());
                input
            }
        };
        window.focus(&input.focus_handle(cx), cx);
        cx.notify();
    }

    /// Search the document for whatever is in the query box, off the main thread.
    ///
    /// PDFium is behind one process-wide lock and a long document takes a noticeable moment, so
    /// running this inline would freeze the window on every keystroke.
    fn run_search(&mut self, cx: &mut Context<Self>) {
        let needle = self
            .find
            .input
            .as_ref()
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if needle == self.find.query {
            return;
        }
        self.find.query = needle.clone();
        self.find.at = 0;

        if needle.is_empty() {
            self.find.hits.clear();
            self.find.searching = false;
            cx.notify();
            return;
        }

        self.find.searching = true;
        let path = self.path.clone();
        cx.spawn(async move |this, cx| {
            let hits = cx
                .background_executor()
                .spawn({
                    let needle = needle.clone();
                    async move { preview::search(&path, &needle) }
                })
                .await;
            this.update(cx, |this, cx| {
                // A slower search for an older query must not overwrite a newer one's results.
                if this.find.query != needle {
                    return;
                }
                this.find.hits = hits;
                this.find.searching = false;
                if let Some(page) = this.find.hits.first().map(|hit| hit.page) {
                    this.show_page(page);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn step_match(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(page) = self.find.step(delta) {
            self.show_page(page);
            cx.notify();
        }
    }
}

impl Render for Viewer {
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
            .unwrap_or("File")
            .to_string()
            .into();
        let pill = cx.theme().background.opacity(0.8);
        let accent = cx.theme().primary.opacity(0.55);
        let marks: Vec<preview::Match> = self.find.on_page(page).cloned().collect();
        // The panel's *live* width, straight off the resizable's state, so the rows re-trim as it
        // is dragged. Empty until the group has laid out once.
        let panel_width = self.split.read(cx).sizes().get(1).copied().unwrap_or(PANEL);

        div()
            .track_focus(&self.focus_handle)
            .id("viewer")
            // Fills whichever overlay slot mounted it; `absolute` so it stacks over that slot's
            // content. `top_0`/`left_0` are load-bearing: with no insets an absolute box takes a
            // *static* position, i.e. after its in-flow siblings — so in the centre slot, whose
            // sibling is the full-width view body, the overlay lands just off its right edge.
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden()
            // Register an opaque hitbox over the whole overlay so clicks and scrolls land here
            // instead of falling through to the table painted behind it.
            .occlude()
            // Dim what's behind so the file reads as the focus.
            .bg(cx.theme().background.opacity(0.9))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                // Paging keys are only ours while the viewer itself holds focus: with the query
                // box focused, left/right belong to its caret.
                let reading = this.focus_handle.is_focused(window);
                match ev.keystroke.key.as_str() {
                    "f" if ev.keystroke.modifiers.secondary() && this.document => {
                        this.open_find(window, cx);
                    }
                    // The panel is the inner layer, so Escape dismisses it before the viewer.
                    "escape" if this.find_open => {
                        this.find_open = false;
                        window.focus(&this.focus_handle, cx);
                        cx.notify();
                    }
                    "escape" => close_viewer(cx),
                    // The keys anyone reading a document reaches for first. Harmless on a photo,
                    // where there is only ever one page to move between.
                    "left" | "pageup" if reading => {
                        this.turn_page(-1);
                        cx.notify();
                    }
                    "right" | "pagedown" if reading => {
                        this.turn_page(1);
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .child(
                h_resizable("viewer-split")
                    .with_state(&self.split)
                    .child(
                        resizable_panel().child(
                            // Padding is the only sizing input: the file `Contain`-fits the padded
                            // content area at zoom 1, so it scales with the window/dock instead of
                            // a fixed fraction.
                            //
                            // Pan and zoom live here rather than on the whole overlay: a press in
                            // the find panel is not the start of a drag across the page, and a
                            // scroll over a list of matches is not a zoom.
                            div()
                                .size_full()
                                .p_8()
                                .on_scroll_wheel(cx.listener(
                                    |this, ev: &ScrollWheelEvent, _, cx| {
                                        // Any scroll zooms — trackpad pixel deltas and wheel line
                                        // deltas alike; drag is what pans, so scroll is free.
                                        let step = match ev.delta {
                                            ScrollDelta::Lines(d) => d.y * 0.1,
                                            ScrollDelta::Pixels(d) => f32::from(d.y) * 0.005,
                                        };
                                        this.set_zoom(this.zoom * (1.0 + step));
                                        cx.notify();
                                    },
                                ))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                        this.drag_from = Some(ev.position);
                                        cx.notify();
                                    }),
                                )
                                .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                                    let Some(last) = this.drag_from else {
                                        return;
                                    };
                                    this.offset.x += ev.position.x - last.x;
                                    this.offset.y += ev.position.y - last.y;
                                    this.drag_from = Some(ev.position);
                                    cx.notify();
                                }))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                                        this.drag_from = None;
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    // The positioned content box. The overlay measures *this*
                                    // element, so the highlight maths never has to know about the
                                    // padding above it.
                                    div()
                                        .relative()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            // `flex_shrink_0` keeps `relative(zoom)` past 1;
                                            // `relative` + `left`/`top` pan it.
                                            //
                                            // `FULL`, so zooming in reaches the file's real detail
                                            // rather than magnifying a thumbnail — this is the one
                                            // place that asks for no cap, and for an image it is
                                            // the original file gpui draws, not our copy.
                                            img(preview::source(&self.path, preview::FULL, page))
                                                .flex_shrink_0()
                                                .relative()
                                                .w(relative(zoom))
                                                .h(relative(zoom))
                                                .left(offset.x)
                                                .top(offset.y)
                                                .object_fit(ObjectFit::Contain),
                                        )
                                        // Hit boxes over the page, for the hits that are on it.
                                        .when(!marks.is_empty(), |area| {
                                            area.child(highlight::overlay(
                                                marks, zoom, offset, accent,
                                            ))
                                        }),
                                ),
                        ),
                    )
                    // `visible` rather than adding and removing the panel: the group tracks its
                    // children by index, so a panel that comes and goes loses its dragged width.
                    .child(
                        resizable_panel()
                            .size(PANEL)
                            .size_range(PANEL_RANGE)
                            .visible(self.find_open)
                            .child(find::panel(&self.find, panel_width, cx)),
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
            // Page controls, for a document — including a one-page one. "1 / 1" is what tells you
            // this is a PDF at all rather than a picture of a page, which is otherwise invisible.
            .when(self.document, |viewer| {
                viewer.child(
                    div()
                        .absolute()
                        .bottom_4()
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            // Loud on purpose. These are the only controls a reader reaches for
                            // constantly, and over a dimmed page a translucent pill of small
                            // ghost buttons reads as decoration.
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .py_1()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .shadow_lg()
                                .child(
                                    Button::new("previous-page")
                                        .icon(IconName::ChevronLeft)
                                        .outline()
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
                                        .font_semibold()
                                        .child(format!("Page {} of {pages}", page + 1)),
                                )
                                .child(
                                    Button::new("next-page")
                                        .icon(IconName::ChevronRight)
                                        .outline()
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
                    // Only for a format that can carry text at all — offered on a photo it would
                    // open a panel that can never find anything.
                    .when(self.document, |group| {
                        group.child(
                            Button::new("toggle-find")
                                .icon(IconName::Search)
                                .ghost()
                                .small()
                                .selected(self.find_open)
                                .tooltip("Find in document (Ctrl+F)")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if this.find_open {
                                        this.find_open = false;
                                        window.focus(&this.focus_handle, cx);
                                        cx.notify();
                                    } else {
                                        this.open_find(window, cx);
                                    }
                                })),
                        )
                    })
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
                            .on_click(cx.listener(|_, _, _, cx| close_viewer(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use gpui::TestAppContext;

    use crate::viewer::{Scope, close_viewer, open_viewer, viewer_in};

    /// One global feeds two mount slots — the workspace overlay and the centre panel. Exactly one
    /// may claim it: drop the scope check and the viewer paints twice, once in each slot, with two
    /// sets of live controls over each other.
    #[gpui::test]
    fn only_the_slot_it_was_opened_for_mounts_the_viewer(cx: &mut TestAppContext) {
        let path = std::path::PathBuf::from("/nonexistent/qrate-scope-test.jpg");
        cx.update(|cx| {
            open_viewer(path.clone(), Scope::Centre, cx);
            assert!(viewer_in(Scope::Centre, cx).is_some());
            assert!(viewer_in(Scope::Workspace, cx).is_none());

            // Opening in the other scope replaces rather than stacks.
            open_viewer(path, Scope::Workspace, cx);
            assert!(viewer_in(Scope::Workspace, cx).is_some());
            assert!(viewer_in(Scope::Centre, cx).is_none());

            close_viewer(cx);
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
            open_viewer(path, Scope::Workspace, cx);
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

            close_viewer(cx);
        });
    }

    /// A photo is a one-page document, so the controls stay hidden and the arrow keys do nothing.
    #[gpui::test]
    fn a_single_page_file_never_moves(cx: &mut TestAppContext) {
        let path = std::path::PathBuf::from("/nonexistent/qrate-single-page.jpg");
        cx.update(|cx| {
            open_viewer(path, Scope::Centre, cx);
            let viewer = viewer_in(Scope::Centre, cx).expect("just opened");
            viewer.update(cx, |viewer, _| {
                assert_eq!(
                    viewer.pages, 1,
                    "anything that isn't a document has one page"
                );
                assert!(!viewer.document, "and a photo is not a document");
                viewer.turn_page(1);
                viewer.turn_page(-1);
                assert_eq!(viewer.page, 0);
            });
            close_viewer(cx);
        });
    }

    /// The page readout is what tells a reader a one-page PDF is a PDF at all — a picture of a
    /// page and a one-page document look identical without it. It keys off "is a document", not
    /// "has more than one page".
    #[gpui::test]
    fn a_one_page_document_still_says_so(cx: &mut TestAppContext) {
        cx.update(|cx| {
            open_viewer("/nonexistent/one-page.pdf".into(), Scope::Workspace, cx);
            let viewer = viewer_in(Scope::Workspace, cx).expect("just opened");
            viewer.update(cx, |viewer, _| {
                assert_eq!(viewer.pages, 1);
                assert!(viewer.document, "a PDF is a document at any length");
            });

            open_viewer("/nonexistent/scan.jpg".into(), Scope::Workspace, cx);
            let photo = viewer_in(Scope::Workspace, cx).expect("just opened");
            photo.update(cx, |viewer, _| assert!(!viewer.document));

            close_viewer(cx);
        });
    }
}

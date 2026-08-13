//! Where a search hit lands on screen.
//!
//! [`preview::Match`] says where a hit sits on its *page*, as fractions. Turning that into pixels
//! means reproducing exactly what the viewer's element tree does to the page image: scale it to
//! `zoom` of the content box, centre it, shift it by the pan offset, then letterbox the page
//! inside that with `ObjectFit::Contain`. Get any step wrong and the highlight sits near its word
//! rather than on it, which is worse than no highlight at all.
//!
//! Kept apart from the render builder because it is arithmetic with no gpui context in it, which
//! is the only reason it can be tested at all.

use gpui::{
    AnyElement, Bounds, Hsla, IntoElement, Pixels, Point, Size, Styled as _, canvas, fill, point,
    size,
};

/// The boxes over the page, painted for every hit on it.
///
/// A canvas rather than absolutely-positioned divs because only paint time knows how big the
/// content box actually came out — which is exactly what the `Contain` letterbox depends on. It
/// measures its own bounds, so it must be laid over the content box itself and nothing wider.
pub fn overlay(
    marks: Vec<preview::Match>,
    zoom: f32,
    offset: Point<Pixels>,
    color: Hsla,
) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            for mark in &marks {
                window.paint_quad(fill(quad(bounds, zoom, offset, mark), color));
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

/// The on-screen box for one hit, given the content area the page is drawn into.
pub fn quad(
    area: Bounds<Pixels>,
    zoom: f32,
    offset: Point<Pixels>,
    hit: &preview::Match,
) -> Bounds<Pixels> {
    let (frame, origin) = drawn(area, zoom, offset, hit.aspect);
    Bounds {
        origin: point(
            origin.x + frame.width * hit.left,
            origin.y + frame.height * hit.top,
        ),
        size: size(frame.width * hit.width, frame.height * hit.height),
    }
}

/// The page image's own rectangle: its size, and where its top-left corner sits.
fn drawn(
    area: Bounds<Pixels>,
    zoom: f32,
    offset: Point<Pixels>,
    aspect: f32,
) -> (Size<Pixels>, Point<Pixels>) {
    // The element is `w(relative(zoom)).h(relative(zoom))` of the content box, centred by the
    // flex parent, then nudged by the pan.
    let box_size = size(area.size.width * zoom, area.size.height * zoom);
    let box_origin = point(
        area.origin.x + (area.size.width - box_size.width) / 2. + offset.x,
        area.origin.y + (area.size.height - box_size.height) / 2. + offset.y,
    );

    // `Contain`: the page keeps its aspect ratio and fits inside that box, so whichever axis runs
    // out first decides the scale and the other one gets the letterbox bars.
    let height_if_wide = box_size.width / aspect;
    let frame = if height_if_wide <= box_size.height {
        size(box_size.width, height_if_wide)
    } else {
        size(box_size.height * aspect, box_size.height)
    };
    let origin = point(
        box_origin.x + (box_size.width - frame.width) / 2.,
        box_origin.y + (box_size.height - frame.height) / 2.,
    );
    (frame, origin)
}

#[cfg(test)]
mod tests {
    // No `use super::*`: this file has no `use gpui::*`, but the sibling modules do and the habit
    // is what keeps the `#[test]` shadowing bug (see CLAUDE.md) out of the crate.
    use gpui::{Bounds, point, px, size};

    use crate::viewer::highlight::quad;

    fn hit(left: f32, top: f32, aspect: f32) -> preview::Match {
        preview::Match {
            page: 0,
            left,
            top,
            width: 0.5,
            height: 0.1,
            aspect,
            line: String::new(),
            at: 0..0,
        }
    }

    /// A square page in a wide area is letterboxed left and right, so the page starts well inside
    /// the area — a highlight that ignored the bars would sit on the wrong words entirely.
    #[test]
    fn a_letterboxed_page_offsets_its_highlights_by_the_bars() {
        let area = Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(400.), px(200.)),
        };
        // Square page in a 400x200 area: drawn 200x200, centred, so it starts 100px in.
        let box_ = quad(area, 1.0, point(px(0.), px(0.)), &hit(0.0, 0.0, 1.0));
        assert_eq!(box_.origin.x, px(100.), "left bar is half the spare width");
        assert_eq!(box_.origin.y, px(0.), "no bar on the limiting axis");
        assert_eq!(box_.size.width, px(100.), "half of a 200px-wide page");
        assert_eq!(box_.size.height, px(20.), "a tenth of a 200px-tall page");

        // A hit halfway down the page lands halfway down the drawn page, not the area.
        let middle = quad(area, 1.0, point(px(0.), px(0.)), &hit(0.0, 0.5, 1.0));
        assert_eq!(middle.origin.y, px(100.));
    }

    /// Zoom scales the page about the centre of the area, and the pan then slides it. Both have to
    /// move the highlight with the page or it drifts off its word the moment anyone looks closer.
    #[test]
    fn zoom_and_pan_carry_the_highlight_with_the_page() {
        let area = Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(200.), px(200.)),
        };
        let at_rest = quad(area, 1.0, point(px(0.), px(0.)), &hit(0.25, 0.25, 1.0));
        assert_eq!(at_rest.origin, point(px(50.), px(50.)));

        // At 2x the page is 400x400 centred on the same 200x200 area, so its top-left is at -100.
        let zoomed = quad(area, 2.0, point(px(0.), px(0.)), &hit(0.25, 0.25, 1.0));
        assert_eq!(zoomed.origin, point(px(0.), px(0.)));
        assert_eq!(zoomed.size.width, px(200.), "the box scales too");

        // Panning is a straight translation on top of that.
        let panned = quad(area, 2.0, point(px(30.), px(-10.)), &hit(0.25, 0.25, 1.0));
        assert_eq!(panned.origin, point(px(30.), px(-10.)));
    }

    /// The area is not always at the window's origin — the centre-panel scope mounts the viewer
    /// partway across the screen, and a highlight that ignored that would be offset by the docks.
    #[test]
    fn highlights_are_placed_relative_to_the_area_not_the_window() {
        let area = Bounds {
            origin: point(px(500.), px(80.)),
            size: size(px(200.), px(200.)),
        };
        let box_ = quad(area, 1.0, point(px(0.), px(0.)), &hit(0.0, 0.0, 1.0));
        assert_eq!(box_.origin, point(px(500.), px(80.)));
    }
}

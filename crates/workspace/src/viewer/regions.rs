//! Where a row's region notes sit on screen, and which region a drag on screen marks.
//!
//! A [`Region`] lives on the upright page in [`Region::SCALE`]ths, and the viewer may show that
//! page turned. Everything here works in fractions of the page *as shown*, converting at the edges,
//! and measures the page with the same letterbox a search hit uses. No gpui context, so it tests.

use diagnostics::Region;
use gpui::{Bounds, Pixels, Point, point, px, size};

use crate::viewer::highlight::drawn;

/// Left, top, right, bottom in fractions of the page as shown. Equal edges are a pin.
pub type Rect = [f32; 4];

/// How near a pin's centre the pointer has to be: half the dot that is drawn.
pub const PIN_RADIUS: f32 = 9.;
/// A badge's height, and the side of a resize handle.
pub const BADGE: f32 = 16.;
pub const HANDLE: f32 = 8.;
/// The smallest box a resize leaves, so a region can never be dragged down to a pin.
const SMALLEST: f32 = 0.005;

fn turn((x, y): (f32, f32), turns: u8) -> (f32, f32) {
    (0..turns % 4).fold((x, y), |(x, y), _| (1. - y, x))
}

pub fn span(a: (f32, f32), b: (f32, f32)) -> Rect {
    [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)]
}

/// Where `region` sits on its page turned `turns` quarter turns clockwise.
pub fn shown(region: &Region, turns: u8) -> Rect {
    let scale = f32::from(Region::SCALE);
    let a = (f32::from(region.x) / scale, f32::from(region.y) / scale);
    let b = (
        a.0 + f32::from(region.w) / scale,
        a.1 + f32::from(region.h) / scale,
    );
    span(turn(a, turns), turn(b, turns))
}

/// The region to store for `rect`, marked on `page` while it was shown at `turns`.
pub fn upright(rect: Rect, turns: u8, page: u32, of: Option<(u32, u32)>) -> Region {
    let back = (4 - turns % 4) % 4;
    let [l, t, r, b] = span(
        turn((rect[0], rect[1]), back),
        turn((rect[2], rect[3]), back),
    );
    let q = |v: f32| (v.clamp(0., 1.) * f32::from(Region::SCALE)).round() as u16;
    Region {
        page,
        x: q(l),
        y: q(t),
        w: q(r) - q(l),
        h: q(b) - q(t),
        of,
    }
}

/// The page as drawn in `area`, the box every region and pointer position is measured against.
pub fn page(area: Bounds<Pixels>, zoom: f32, offset: Point<Pixels>, aspect: f32) -> Bounds<Pixels> {
    let (size, origin) = drawn(area, zoom, offset, aspect);
    Bounds { origin, size }
}

/// `rect` over `page` on screen.
pub fn on_screen(page: Bounds<Pixels>, rect: Rect) -> Bounds<Pixels> {
    let (width, height) = (page.size.width, page.size.height);
    Bounds {
        origin: point(
            page.origin.x + width * rect[0],
            page.origin.y + height * rect[1],
        ),
        size: size(width * (rect[2] - rect[0]), height * (rect[3] - rect[1])),
    }
}

/// The point of `page` under `position`, clamped onto the page.
pub fn at(page: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
    let along = |p: Pixels, from: Pixels, length: Pixels| match length > px(0.) {
        true => (f32::from(p - from) / f32::from(length)).clamp(0., 1.),
        false => 0.,
    };
    (
        along(position.x, page.origin.x, page.size.width),
        along(position.y, page.origin.y, page.size.height),
    )
}

/// The region under `position`: a pin before any box, then the smallest box, so a detail marked
/// inside a larger region stays reachable.
pub fn hit<T: Copy>(boxes: &[(T, Bounds<Pixels>)], position: Point<Pixels>) -> Option<T> {
    let pin = boxes.iter().find(|(_, b)| {
        b.size.width == px(0.)
            && b.size.height == px(0.)
            && f32::from((b.origin - position).x).hypot(f32::from((b.origin - position).y))
                <= PIN_RADIUS
    });
    pin.or_else(|| {
        boxes
            .iter()
            .filter(|(_, b)| b.size.width > px(0.) && b.contains(&position))
            .min_by(|(_, a), (_, b)| {
                let area = |b: &Bounds<Pixels>| f32::from(b.size.width) * f32::from(b.size.height);
                area(a).total_cmp(&area(b))
            })
    })
    .map(|(id, _)| *id)
}

/// One badge or merged pill: where it sits, how wide it is, what it reads and whose it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Badge<T> {
    pub at: Point<Pixels>,
    pub width: Pixels,
    pub label: String,
    pub members: Vec<T>,
}

fn badge_width(label: &str) -> Pixels {
    px((label.len() as f32 * 6. + 8.).max(BADGE))
}

/// A badge at each box's top-left corner, given in number order. One that would cover a badge
/// already placed slides right along its own top edge; out of edge, it joins that badge's pill.
pub fn badges<T: Copy>(boxes: &[(T, usize, Bounds<Pixels>)]) -> Vec<Badge<T>> {
    let mut placed: Vec<Badge<T>> = Vec::new();
    for &(id, number, b) in boxes {
        let label = number.to_string();
        let width = badge_width(&label);
        let y = b.origin.y;
        let mut x = b.origin.x;
        loop {
            let covers = |other: &Badge<T>| {
                x < other.at.x + other.width
                    && other.at.x < x + width
                    && f32::from(y - other.at.y).abs() < BADGE
            };
            let Some(ix) = placed.iter().position(covers) else {
                placed.push(Badge {
                    at: point(x, y),
                    width,
                    label,
                    members: vec![id],
                });
                break;
            };
            let next = placed[ix].at.x + placed[ix].width + px(1.);
            if next + width <= b.origin.x + b.size.width {
                x = next;
                continue;
            }
            let pill = &mut placed[ix];
            pill.label = format!("{} {label}", pill.label);
            pill.width = badge_width(&pill.label);
            pill.members.push(id);
            break;
        }
    }
    placed
}

/// What a drag on a selected region takes hold of: the whole box, or one of its eight handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Move,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Grip {
    pub const HANDLES: [Grip; 8] = [
        Grip::NW,
        Grip::N,
        Grip::NE,
        Grip::E,
        Grip::SE,
        Grip::S,
        Grip::SW,
        Grip::W,
    ];

    /// Where the handle sits along the box's width and height.
    pub fn anchor(self) -> (f32, f32) {
        match self {
            Grip::Move => (0.5, 0.5),
            Grip::N => (0.5, 0.),
            Grip::NE => (1., 0.),
            Grip::E => (1., 0.5),
            Grip::SE => (1., 1.),
            Grip::S => (0.5, 1.),
            Grip::SW => (0., 1.),
            Grip::W => (0., 0.5),
            Grip::NW => (0., 0.),
        }
    }

    /// The handle of `rect` under `position`, if any. Pins have none: they only move.
    pub fn under(rect: Bounds<Pixels>, position: Point<Pixels>) -> Option<Grip> {
        if rect.size.width == px(0.) {
            return None;
        }
        Grip::HANDLES.into_iter().find(|grip| {
            let (u, v) = grip.anchor();
            let centre = point(
                rect.origin.x + rect.size.width * u,
                rect.origin.y + rect.size.height * v,
            );
            f32::from(position.x - centre.x).abs() <= HANDLE / 2. + 2.
                && f32::from(position.y - centre.y).abs() <= HANDLE / 2. + 2.
        })
    }
}

/// `rect` after `grip` was dragged by `delta` fractions of the page. A move keeps its size and
/// stops at the page's edges; a handle stops there too, and never makes a box smaller than a pin.
pub fn drag(rect: Rect, grip: Grip, delta: (f32, f32)) -> Rect {
    let [l, t, r, b] = rect;
    if grip == Grip::Move {
        let dx = delta.0.clamp(-l, 1. - r);
        let dy = delta.1.clamp(-t, 1. - b);
        return [l + dx, t + dy, r + dx, b + dy];
    }
    let (u, v) = grip.anchor();
    let edge = |near: f32, far: f32, along: f32, d: f32| match along {
        0. => ((near + d).clamp(0., (far - SMALLEST).max(0.)), far),
        1. => (near, (far + d).clamp((near + SMALLEST).min(1.), 1.)),
        _ => (near, far),
    };
    let (l, r) = edge(l, r, u, delta.0);
    let (t, b) = edge(t, b, v, delta.1);
    [l, t, r, b]
}

#[cfg(test)]
mod tests {
    use diagnostics::Region;
    use gpui::{Bounds, point, px, size};

    use crate::viewer::regions::{Grip, badges, drag, hit, on_screen, page, shown, upright};

    fn region(x: u16, y: u16, w: u16, h: u16) -> Region {
        Region {
            page: 0,
            x,
            y,
            w,
            h,
            of: None,
        }
    }

    fn square(x: f32, y: f32, w: f32, h: f32) -> Bounds<gpui::Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    /// A region drawn on a turned page is stored upright and comes back where it was drawn, at
    /// every turn: a note must not wander when the archivist presses R.
    #[test]
    fn a_region_survives_every_turn() {
        let stamp = region(7000, 7000, 2000, 1700);
        for turns in 0..4 {
            let on_screen = shown(&stamp, turns);
            assert_eq!(
                upright(on_screen, turns, 0, None),
                stamp,
                "at {turns} turns"
            );
        }
        // A quarter turn clockwise takes the bottom-right stamp to the bottom-left.
        let [l, t, r, b] = shown(&stamp, 1);
        assert!((l - 0.13).abs() < 1e-4 && (t - 0.7).abs() < 1e-4);
        assert!((r - 0.3).abs() < 1e-4 && (b - 0.9).abs() < 1e-4);
    }

    /// A pin turns as a point and stays a pin.
    #[test]
    fn a_pin_turns_as_a_point() {
        let pin = region(800, 9000, 0, 0);
        let [l, t, r, b] = shown(&pin, 1);
        assert_eq!((l, t), (r, b));
        assert!((l - 0.1).abs() < 1e-4 && (t - 0.08).abs() < 1e-4);
        assert_eq!(upright([l, t, r, b], 1, 0, None), pin);
    }

    /// The region sits on the page as drawn, letterbox and all, not on the area around it.
    #[test]
    fn a_region_is_placed_on_the_letterboxed_page() {
        let area = square(0., 0., 400., 200.);
        let drawn = page(area, 1.0, point(px(0.), px(0.)), 1.0);
        assert_eq!(drawn, square(100., 0., 200., 200.));
        let on = on_screen(drawn, shown(&region(5000, 0, 2500, 5000), 0));
        assert_eq!(on, square(200., 0., 50., 100.));
    }

    /// Hover goes to a pin first, then to the smallest box: a face inside a group stays
    /// reachable even though the group's box covers it.
    #[test]
    fn the_smallest_region_under_the_pointer_wins() {
        let boxes = [
            (1, square(0., 0., 300., 200.)),
            (2, square(50., 50., 40., 40.)),
            (3, square(60., 60., 0., 0.)),
        ];
        assert_eq!(hit(&boxes, point(px(70.), px(80.))), Some(2));
        assert_eq!(hit(&boxes, point(px(62.), px(58.))), Some(3));
        assert_eq!(hit(&boxes, point(px(200.), px(150.))), Some(1));
        assert_eq!(hit(&boxes, point(px(320.), px(150.))), None);
    }

    /// A badge that would cover another slides along its own top edge; with no edge left it joins
    /// the other's pill rather than stacking on top of it.
    #[test]
    fn crowded_badges_slide_then_merge() {
        let placed = badges(&[
            (5, 5, square(100., 50., 30., 40.)),
            (6, 6, square(104., 52., 60., 40.)),
            (7, 7, square(108., 54., 20., 40.)),
        ]);
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[1].at.x, px(117.), "#6 slid right of #5");
        assert_eq!(
            placed[0].label, "5 7",
            "#7 had no room to slide, so it joined #5"
        );
        assert_eq!(placed[0].members, vec![5, 7]);
    }

    /// A move stops at the page's edge without squashing the box; a handle stops there too and
    /// cannot turn the box inside out.
    #[test]
    fn dragging_keeps_the_box_on_the_page() {
        let rect = [0.1, 0.1, 0.3, 0.4];
        let moved = drag(rect, Grip::Move, (-0.5, 0.0));
        assert_eq!(moved[0], 0.0, "stopped at the left edge");
        assert!(
            (moved[2] - moved[0] - 0.2).abs() < 1e-6,
            "and kept its width"
        );
        assert_eq!(drag(rect, Grip::E, (2.0, 0.0)), [0.1, 0.1, 1.0, 0.4]);
        let [l, _, r, _] = drag(rect, Grip::W, (0.5, 0.0));
        assert!(r - l > 0.0, "the left edge stops short of the right");
        assert_eq!(
            drag(rect, Grip::N, (0.3, 0.0)),
            rect,
            "N only moves the top"
        );
    }

    #[test]
    fn tiny_regions_at_page_edges_can_be_resized() {
        for rect in [[0., 0., 0.001, 0.002], [0.999, 0.998, 1., 1.]] {
            for grip in Grip::HANDLES {
                for delta in [(-1., -1.), (0., 0.), (1., 1.)] {
                    let [l, t, r, b] = drag(rect, grip, delta);
                    assert!(0. <= l && l < r && r <= 1.);
                    assert!(0. <= t && t < b && b <= 1.);
                }
            }
        }
    }
}

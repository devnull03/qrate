//! Region notes on the viewer: the row's regions drawn over its file, and the card a hovered one
//! opens. Positions come from [`regions`], measured against the content box the page is drawn in.

use std::time::Duration;

use diagnostics::{DATASET_MAIN, Diagnostics, NoteId, Region};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use settings::project::RowId;

use crate::viewer::Viewer;
use crate::viewer::regions::{self, Rect};

/// Whether regions are hidden. App-wide, since stepping to the next row builds a new viewer.
#[derive(Default)]
pub(crate) struct Annotating {
    pub hidden: bool,
}

impl Global for Annotating {}

/// The note whose Notes card the pointer is over, lit in whichever viewer shows it.
#[derive(Default)]
pub(crate) struct Lit(pub Option<NoteId>);

impl Global for Lit {}

/// One of the row's region notes, numbered in the order they were filed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    pub id: NoteId,
    pub number: usize,
    pub region: Region,
}

pub(crate) fn marks(row: RowId, cx: &App) -> Vec<Mark> {
    let mut notes: Vec<(NoteId, Region)> = Diagnostics::all(cx)
        .iter()
        .filter(|d| d.location.dataset == DATASET_MAIN && d.location.row_id == Some(row))
        .filter_map(|d| {
            let note = d.note.as_ref()?;
            Some((note.id, note.region?))
        })
        .collect();
    notes.sort_by_key(|(id, _)| *id);
    notes
        .into_iter()
        .enumerate()
        .map(|(ix, (id, region))| Mark {
            id,
            number: ix + 1,
            region,
        })
        .collect()
}

fn hidden(cx: &App) -> bool {
    cx.try_global::<Annotating>().is_some_and(|a| a.hidden)
}

fn area(b: &Bounds<Pixels>) -> f32 {
    f32::from(b.size.width) * f32::from(b.size.height)
}

impl Viewer {
    pub(super) fn row_marks(&self, cx: &App) -> Vec<Mark> {
        self.row.map_or_else(Vec::new, |row| marks(row, cx))
    }

    /// Whether the page's shape is known, so regions land where they belong.
    fn ready(&self) -> bool {
        self.pixels.is_some()
    }

    fn aspect(&self) -> f32 {
        self.fit()
            .map_or(0.75, |(image, _)| image.width / image.height)
    }

    /// The page in content-box coordinates, which is where every region is drawn.
    pub(super) fn page_box(&self) -> Bounds<Pixels> {
        let area = Bounds {
            origin: Point::default(),
            size: self.frame.get().size,
        };
        regions::page(area, self.zoom, self.offset, self.aspect())
    }

    pub(super) fn local(&self, position: Point<Pixels>) -> Point<Pixels> {
        position - self.frame.get().origin
    }

    /// This page's marks and where each sits on screen, larger first so smaller ones paint over.
    fn placed(&self, marks: &[Mark]) -> Vec<(Mark, Rect, Bounds<Pixels>)> {
        let page = self.page_box();
        let mut placed: Vec<_> = marks
            .iter()
            .filter(|m| m.region.page as usize == self.page)
            .map(|m| {
                let rect = regions::shown(&m.region, self.shown);
                (*m, rect, regions::on_screen(page, rect))
            })
            .collect();
        placed.sort_by(|a, b| area(&b.2).total_cmp(&area(&a.2)));
        placed
    }

    /// Track the region under the pointer, for its fill and its card.
    pub(super) fn hover(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let hit = match self.ready() && !hidden(cx) {
            true => {
                let placed = self.placed(&self.row_marks(cx));
                let boxes: Vec<_> = placed.iter().map(|(m, _, b)| (m.id, *b)).collect();
                regions::hit(&boxes, self.local(position))
            }
            false => None,
        };
        if hit != self.hovered {
            self.hovered = hit;
            cx.notify();
        }
    }

    /// Show `text` at the foot of the stage for a moment.
    pub(super) fn hint(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let gone = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2500))
                .await;
            this.update(cx, |this, cx| {
                this.hint = None;
                cx.notify();
            })
            .ok();
        });
        self.hint = Some((text.into(), gone));
        cx.notify();
    }

    pub(super) fn toggle_hidden(&mut self, cx: &mut Context<Self>) {
        let now = !hidden(cx);
        cx.default_global::<Annotating>().hidden = now;
        let count = self.row_marks(cx).len();
        if now && count > 0 {
            let noun = if count == 1 {
                "annotation"
            } else {
                "annotations"
            };
            self.hint(format!("{count} {noun} hidden. Press N to show them."), cx);
        }
        cx.notify();
    }
}

/// The show/hide toggle for the viewer's toolbar.
pub(super) fn buttons(this: &Viewer, cx: &mut Context<Viewer>) -> AnyElement {
    let hidden = hidden(cx);
    h_flex()
        .gap_1()
        .child(
            Button::new("toggle-annotations")
                .icon(match hidden {
                    true => IconName::EyeOff,
                    false => IconName::Eye,
                })
                .ghost()
                .small()
                .disabled(this.row.is_none())
                .tooltip(match hidden {
                    true => "Show annotations (N)",
                    false => "Hide annotations (N)",
                })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx))),
        )
        .into_any_element()
}

/// Everything drawn over the page: each region, its badge, and the card of the one hovered.
/// Laid over the content box, in its coordinates.
pub(super) fn layer(this: &Viewer, marks: &[Mark], cx: &mut Context<Viewer>) -> AnyElement {
    let frame = this.frame.get().size;
    let shown = !hidden(cx) && this.ready() && frame.width > px(0.);
    let placed = match shown {
        true => this.placed(marks),
        false => Vec::new(),
    };
    let lit = cx.try_global::<Lit>().and_then(|lit| lit.0);
    let focus = this.hovered.or(lit);
    let focus_box = focus.and_then(|id| placed.iter().find(|(m, ..)| m.id == id).map(|p| p.2));
    let crowded = focus_box.is_some_and(|f| {
        placed
            .iter()
            .any(|(m, _, b)| Some(m.id) != focus && f.intersects(b))
    });
    let dim = |id: NoteId| crowded && Some(id) != focus;
    let (white, dark) = (white(), rgb(0x161616));

    let mut boxes: Vec<_> = placed
        .iter()
        .filter(|(_, rect, _)| rect[0] != rect[2] || rect[1] != rect[3])
        .map(|(m, _, b)| (m.id, m.number, *b))
        .collect();
    boxes.sort_by_key(|(_, number, _)| *number);
    let badges = regions::badges(&boxes);

    let card = focus
        .filter(|id| Some(*id) == this.hovered)
        .and_then(|id| {
            Some((
                Diagnostics::note(id, cx)?,
                placed.iter().find(|p| p.0.id == id)?,
            ))
        })
        .map(|(note, (_, _, b))| {
            let pin = b.size.width == px(0.);
            let below = match pin {
                true => b.origin.y + px(14.),
                false => b.origin.y + b.size.height + px(8.),
            };
            let top = match below + px(110.) > frame.height - px(12.) {
                true => b.origin.y - px(118.),
                false => below,
            };
            let left = b
                .origin
                .x
                .min(frame.width - px(diagnostics::note_card::WIDTH + 12.))
                .max(px(12.));
            div()
                .absolute()
                .left(left)
                .top(top)
                .child(diagnostics::note_card::note_card(
                    note,
                    None,
                    Vec::new(),
                    cx,
                ))
        });

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .children(placed.iter().map(|(m, rect, b)| {
            let pin = rect[0] == rect[2] && rect[1] == rect[3];
            let opacity = if dim(m.id) { 0.4 } else { 1. };
            match pin {
                true => div()
                    .absolute()
                    .left(b.origin.x - px(9.))
                    .top(b.origin.y - px(9.))
                    .size(px(18.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(white)
                    .bg(dark)
                    .border(px(1.5))
                    .border_color(white)
                    .shadow_sm()
                    .opacity(opacity)
                    .child(m.number.to_string()),
                false => div()
                    .absolute()
                    .left(b.origin.x)
                    .top(b.origin.y)
                    .w(b.size.width)
                    .h(b.size.height)
                    .rounded(px(1.))
                    .border(px(1.5))
                    .border_color(black().opacity(0.7))
                    .opacity(opacity)
                    .child(
                        div()
                            .size_full()
                            .border(px(1.5))
                            .border_color(white.opacity(0.92))
                            .when(focus == Some(m.id), |inner| inner.bg(white.opacity(0.18))),
                    ),
            }
        }))
        .children(badges.into_iter().map(|badge| {
            let faded = badge.members.iter().all(|id| dim(*id));
            div()
                .absolute()
                .left(badge.at.x - px(1.5))
                .top(badge.at.y - px(1.5))
                .h(px(regions::BADGE))
                .min_w(px(regions::BADGE))
                .px(px(4.))
                .rounded_br(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .whitespace_nowrap()
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(white)
                .bg(black().opacity(0.9))
                .opacity(if faded { 0.4 } else { 1. })
                .child(badge.label)
        }))
        .children(card)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use diagnostics::{DATASET_MAIN, Diagnostics, Location, Region};
    use gpui::TestAppContext;
    use settings::history::Origin;

    use crate::viewer::annotate::marks;

    /// A row's regions are numbered in the order they were filed, and nothing else on the row —
    /// its plain notes, another row's regions — takes a number.
    #[gpui::test]
    fn a_rows_regions_are_numbered_in_filing_order(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let row = |id| Location {
                dataset: DATASET_MAIN.into(),
                row: Some(id as usize - 1),
                row_id: Some(id),
                column: None,
            };
            let region = |x| Region {
                page: 0,
                x,
                y: 0,
                w: 1000,
                h: 1000,
                of: None,
            };
            let file = |at, region, text: &str, cx: &mut gpui::App| {
                Diagnostics::file_note(row(at), region, None, text.into(), Origin::Drawn, cx)
            };
            let first = file(1, Some(region(100)), "stamp", cx);
            file(1, None, "faded", cx);
            file(2, Some(region(200)), "elsewhere", cx);
            let second = file(1, Some(region(300)), "signature", cx);

            let numbered: Vec<_> = marks(1, cx)
                .iter()
                .map(|m| (Some(m.id), m.number))
                .collect();
            assert_eq!(numbered, vec![(first, 1), (second, 2)]);
        });
    }
}

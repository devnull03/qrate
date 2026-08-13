//! Finding text in a document: the query box, the hits PDFium returned, which one is current, and
//! the panel that lists them.
//!
//! The panel is a component in its own right rather than a slice of the viewer's builder, which is
//! why it lives here beside the state it draws. It returns `AnyElement` because gpui's chained
//! builders produce deeply nested generic types, and propagating one out of a function this long
//! overflows rustc's stack at type-check time (see CLAUDE.md).

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable as _, IconName, Sizable, StyledExt as _,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex,
};

use crate::viewer::Viewer;

#[derive(Default)]
pub struct Find {
    /// The query box. `None` until the panel is first opened: `InputState::new` needs a `Window`,
    /// which the viewer has only once it is rendering.
    pub input: Option<Entity<InputState>>,
    /// Every hit in the document, in document order.
    pub hits: Vec<preview::Match>,
    /// Which hit is current, as an index into `hits`.
    pub at: usize,
    /// A search is running. The panel says so rather than showing a momentary "no matches".
    pub searching: bool,
    /// The query the current `hits` answer, so a repeat of the same search is not re-run.
    pub query: String,
    /// Whether the document has any text to search. `None` until checked. A scan that was never
    /// OCR'd finds nothing for every query, and "No matches" would blame the query for it.
    pub layered: Option<bool>,
}

impl Find {
    /// Step `delta` hits and return the page to show, or `None` when there is nothing to step to.
    ///
    /// Wraps, unlike paging: a find that stops dead on the last hit makes the user retype the
    /// query to get back to the first one.
    pub fn step(&mut self, delta: isize) -> Option<usize> {
        let count = self.hits.len();
        if count == 0 {
            return None;
        }
        self.at = (self.at as isize + delta).rem_euclid(count as isize) as usize;
        self.hits.get(self.at).map(|hit| hit.page)
    }

    /// Jump straight to the `index`th hit — what clicking a row does.
    pub fn jump(&mut self, index: usize) -> Option<usize> {
        self.at = index;
        self.hits.get(index).map(|hit| hit.page)
    }

    /// The hits sitting on `page`, which are the only ones the overlay can draw.
    pub fn on_page(&self, page: usize) -> impl Iterator<Item = &preview::Match> {
        self.hits.iter().filter(move |hit| hit.page == page)
    }

    /// What the panel says above the rows.
    fn readout(&self) -> String {
        // Ahead of everything else: on a document with no text layer the query is not the problem,
        // and no wording about matches would be true.
        if self.layered == Some(false) {
            return "This document has no text layer — it was scanned, not typed".to_string();
        }
        match (self.searching, self.query.is_empty(), self.hits.len()) {
            (true, ..) => "Searching…".to_string(),
            (false, true, _) => "Type to search this document".to_string(),
            (false, false, 0) => "No matches".to_string(),
            (false, false, 1) => "1 match".to_string(),
            (false, false, count) => format!("{count} matches"),
        }
    }
}

/// The find panel: the query box, a count, and one row per hit.
///
/// One row per hit rather than a box holding the whole document — a reader wants to see *where* a
/// word turns up, and a wall of extracted text answers a question nobody asked.
pub fn panel(find: &Find, width: Pixels, cx: &mut Context<Viewer>) -> AnyElement {
    let empty = find.hits.is_empty();
    let rows: Vec<_> = find
        .hits
        .iter()
        .enumerate()
        .map(|(index, hit)| (index, hit.page, hit.line.clone(), hit.at.clone()))
        .collect();

    v_flex()
        .size_full()
        .gap_2()
        .p_2()
        // Clears the control cluster pinned to the overlay's top-right corner.
        .pt_12()
        .occlude()
        .bg(cx.theme().background)
        .border_l_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .gap_1()
                .when_some(find.input.clone(), |bar, input| {
                    bar.child(div().flex_1().child(Input::new(&input)))
                })
                .child(
                    Button::new("previous-match")
                        .icon(IconName::ChevronUp)
                        .ghost()
                        .small()
                        .disabled(empty)
                        .tooltip("Previous match")
                        .on_click(cx.listener(|this, _, _, cx| this.step_match(-1, cx))),
                )
                .child(
                    Button::new("next-match")
                        .icon(IconName::ChevronDown)
                        .ghost()
                        .small()
                        .disabled(empty)
                        .tooltip("Next match")
                        .on_click(cx.listener(|this, _, _, cx| this.step_match(1, cx))),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(find.readout()),
        )
        .child(
            v_flex()
                .id("matches")
                .flex_1()
                .min_h_0()
                .gap_1()
                .overflow_y_scroll()
                .children(
                    rows.into_iter()
                        .map(|row| self::row(row, find.at, width, cx).into_any_element()),
                ),
        )
        .into_any_element()
}

/// Roughly how many characters fit across a row of `width`.
///
/// Proportional text has no exact answer and none is needed: this only has to keep a match near
/// the middle of the row as the panel is dragged, so an average advance is enough.
fn budget(width: Pixels) -> usize {
    /// Average advance of the row's `text_xs`, and what the badge, gaps and padding take.
    const ADVANCE: f32 = 6.0;
    const CHROME: f32 = 84.0;

    (((f32::from(width) - CHROME) / ADVANCE).max(8.0)) as usize
}

/// Split `line` at the match, trimming each side so the row is about `budget` characters with the
/// match near the middle. Sliced on character boundaries, so it is safe on any text PDFium hands
/// back.
fn centre(line: &str, span: std::ops::Range<usize>, budget: usize) -> (&str, &str, &str) {
    let word = &line[span.clone()];
    let side = budget.saturating_sub(word.chars().count()) / 2;

    let before = &line[..span.start];
    let head = match side
        .checked_sub(1)
        .and_then(|n| before.char_indices().rev().nth(n))
    {
        Some((at, _)) => &before[at..],
        // Either the row has no room at all, or the context is already shorter than its share.
        None if side == 0 => "",
        None => before,
    };

    let after = &line[span.end..];
    let tail = match after.char_indices().nth(side) {
        Some((at, _)) => &after[..at],
        None => after,
    };
    (head, word, tail)
}

/// One hit: its page, then its line with the matched words picked out.
fn row(
    (index, page, line, span): (usize, usize, String, std::ops::Range<usize>),
    current: usize,
    width: Pixels,
    cx: &mut Context<Viewer>,
) -> impl IntoElement {
    let (head, word, tail) = centre(&line, span, budget(width));
    let muted = cx.theme().muted_foreground;

    h_flex()
        .id(index)
        .w_full()
        .gap_1()
        .px_2()
        .py_1()
        .rounded(cx.theme().radius)
        .overflow_hidden()
        .text_xs()
        .cursor_pointer()
        .when(index == current, |row| row.bg(cx.theme().accent))
        .hover(|row| row.bg(cx.theme().accent))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(page) = this.find.jump(index) {
                this.show_page(page);
                cx.notify();
            }
        }))
        // A badge, not more result text: it is the one part of the row a reader scans down rather
        // than reads, so it gets its own weight and colour instead of the context's.
        .child(
            div()
                .flex_none()
                .px_1p5()
                .rounded(cx.theme().radius)
                .bg(cx.theme().primary.opacity(0.15))
                .text_color(cx.theme().primary)
                .font_semibold()
                .whitespace_nowrap()
                .child(format!("Page {}", page + 1)),
        )
        // One line, never wrapped: a row points at a place in the document, it is not the document.
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(div().flex_none().text_color(muted).child(head.to_string()))
                .child(
                    div()
                        .flex_none()
                        .font_semibold()
                        .text_color(cx.theme().foreground)
                        .child(word.to_string()),
                )
                .child(div().flex_none().text_color(muted).child(tail.to_string())),
        )
}

#[cfg(test)]
mod tests {
    // Never `use super::*` in a tests module in this crate — see CLAUDE.md.
    use crate::viewer::find::Find;

    fn hit(page: usize) -> preview::Match {
        preview::Match {
            page,
            left: 0.,
            top: 0.,
            width: 0.,
            height: 0.,
            aspect: 1.,
            line: String::new(),
            at: 0..0,
        }
    }

    /// Stepping wraps in both directions and reports the page each hit lives on, which is what
    /// moves the viewer. Two hits on one page must not collapse into one stop.
    #[test]
    fn stepping_wraps_and_reports_each_hits_page() {
        let mut find = Find {
            hits: vec![hit(2), hit(2), hit(7)],
            ..Default::default()
        };

        assert_eq!(find.step(1), Some(2), "second hit, still on page 2");
        assert_eq!(find.at, 1);
        assert_eq!(find.step(1), Some(7));
        assert_eq!(
            find.step(1),
            Some(2),
            "past the last hit comes back to the first"
        );
        assert_eq!(find.at, 0);
        assert_eq!(
            find.step(-1),
            Some(7),
            "and back from the first is the last"
        );
        assert_eq!(find.at, 2);
    }

    /// With nothing found, stepping must not move the reader off the page they are on.
    #[test]
    fn stepping_with_no_hits_goes_nowhere() {
        let mut find = Find::default();
        assert_eq!(find.step(1), None);
        assert_eq!(find.step(-1), None);
        assert_eq!(find.at, 0);
    }

    /// The overlay draws only the hits on the page it is over; drawing them all would paint every
    /// other page's boxes onto this one.
    #[test]
    fn only_the_current_pages_hits_are_offered_to_the_overlay() {
        let find = Find {
            hits: vec![hit(0), hit(3), hit(3), hit(9)],
            ..Default::default()
        };
        assert_eq!(find.on_page(3).count(), 2);
        assert_eq!(find.on_page(0).count(), 1);
        assert_eq!(
            find.on_page(5).count(),
            0,
            "a page with no hits draws nothing"
        );
    }

    /// Clicking a row jumps to that hit, and a row index that no longer exists must not panic —
    /// the panel and the hit list are rebuilt on separate frames.
    #[test]
    fn jumping_to_a_row_lands_on_its_page_and_survives_a_stale_index() {
        let mut find = Find {
            hits: vec![hit(4), hit(6)],
            ..Default::default()
        };
        assert_eq!(find.jump(1), Some(6));
        assert_eq!(find.at, 1);
        assert_eq!(find.jump(9), None, "a stale row index is not a panic");
    }

    /// The readout is the only thing distinguishing "still working" from "nothing there", which is
    /// the difference between waiting and retyping the query.
    #[test]
    fn the_readout_tells_searching_apart_from_empty() {
        let searching = Find {
            searching: true,
            query: "aderman".into(),
            ..Default::default()
        };
        assert_eq!(searching.readout(), "Searching…");

        let nothing = Find {
            query: "aderman".into(),
            ..Default::default()
        };
        assert_eq!(nothing.readout(), "No matches");

        let untouched = Find::default();
        assert_eq!(untouched.readout(), "Type to search this document");

        let one = Find {
            query: "a".into(),
            hits: vec![hit(0)],
            ..Default::default()
        };
        assert_eq!(one.readout(), "1 match", "not '1 matches'");

        let many = Find {
            query: "a".into(),
            hits: vec![hit(0), hit(1)],
            ..Default::default()
        };
        assert_eq!(many.readout(), "2 matches");
    }

    /// The match has to stay near the middle of a row as the panel is dragged, which means the
    /// two sides are trimmed together — not the tail alone, which would push the match left until
    /// it fell off a narrow panel.
    #[test]
    fn a_row_keeps_its_match_near_the_middle_at_any_width() {
        use crate::viewer::find::{budget, centre};

        let line = "the quick brown fox jumps over the lazy dog and keeps on running";
        let span = line.find("jumps").expect("present")..line.find("jumps").unwrap() + 5;

        let (head, word, tail) = centre(line, span.clone(), 25);
        assert_eq!(word, "jumps", "the match itself is never trimmed");
        assert_eq!(
            head.chars().count(),
            10,
            "ten either side of a five-char match"
        );
        assert_eq!(tail.chars().count(), 10);
        assert!(
            line.contains(head) && line.contains(tail),
            "both are real slices"
        );

        // A wider panel shows more of both sides, still balanced.
        let (wide_head, _, wide_tail) = centre(line, span.clone(), 45);
        assert!(wide_head.chars().count() > head.chars().count());
        assert!(wide_tail.chars().count() > tail.chars().count());

        // A budget with no room to spare drops the context rather than the match, and must not
        // panic slicing a string it cannot fit.
        let (tight_head, tight_word, tight_tail) = centre(line, span, 3);
        assert_eq!(tight_word, "jumps");
        assert!(tight_head.is_empty() && tight_tail.is_empty());

        // Width drives the budget, and a panel dragged to nothing still asks for a usable row.
        assert!(budget(gpui::px(720.)) > budget(gpui::px(240.)));
        assert!(budget(gpui::px(1.)) >= 8, "never a zero-length row");
    }

    /// Context runs out before the budget does at the start of a page — the head is simply
    /// shorter, and nothing is sliced off a boundary that isn't there.
    #[test]
    fn a_match_with_little_context_either_side_is_left_alone() {
        use crate::viewer::find::centre;

        let (head, word, tail) = centre("hi there", 0..2, 40);
        assert_eq!((head, word, tail), ("", "hi", " there"));

        // Multi-byte characters must not be split mid-character.
        let line = "café société naïve";
        let span = line.find("société").unwrap()..line.find("société").unwrap() + "société".len();
        let (head, word, _) = centre(line, span, 12);
        assert_eq!(word, "société");
        assert!(line.contains(head));
    }

    /// A scan finds nothing for every query, and "No matches" blames the query for it. The
    /// document's own lack of a text layer has to win over every other wording.
    #[test]
    fn a_scan_without_a_text_layer_says_so_instead_of_no_matches() {
        let scan = Find {
            query: "aderman".into(),
            layered: Some(false),
            ..Default::default()
        };
        assert!(scan.readout().contains("no text layer"));

        // Even before anything is typed — the panel should not invite a search that cannot work.
        let untouched = Find {
            layered: Some(false),
            ..Default::default()
        };
        assert!(untouched.readout().contains("no text layer"));

        // A document that does have text is unaffected.
        let typed = Find {
            query: "aderman".into(),
            layered: Some(true),
            ..Default::default()
        };
        assert_eq!(typed.readout(), "No matches");
    }
}

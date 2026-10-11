//! The note hover card: one note whole, then who filed it and when. The viewer shows it on a region
//! and the grid on a cell; each caller only decides where it goes.

use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, IntoElement, ParentElement, SharedString, Styled, div, px};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::{Diagnostic, Severity, severity_color};

pub const WIDTH: f32 = 280.;

/// The muted line under a note: its author or "Unsigned", when it was filed, and "edited" once it
/// has been reworded.
pub fn byline(note: &Diagnostic) -> SharedString {
    let meta = note.note.as_ref();
    let filed = meta.and_then(|n| n.filed.as_ref());
    let author = filed
        .and_then(|f| f.author.clone())
        .unwrap_or_else(|| "Unsigned".into());
    [
        Some(author),
        filed.and_then(|f| f.date.clone()),
        meta.and_then(|n| n.edited.as_ref())
            .map(|_| "edited".into()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<SharedString>>()
    .join(" · ")
    .into()
}

/// `note` over its [`byline`]. What a cell's tooltip used to say — the column's `description` and
/// the validators' `findings` — follows under a divider, so the note always reads first.
pub fn note_card(
    note: &Diagnostic,
    description: Option<SharedString>,
    findings: Vec<(Severity, SharedString)>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let extra = description.is_some() || !findings.is_empty();
    v_flex()
        .w(px(WIDTH))
        .gap_1()
        .px_2p5()
        .py_2()
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .border_1()
        .border_color(theme.border)
        .rounded(px(6.))
        .shadow_md()
        .text_size(px(13.))
        .line_height(px(18.))
        .child(div().whitespace_normal().child(note.message.clone()))
        .child(
            div()
                .text_xs()
                .line_height(px(16.))
                .text_color(theme.muted_foreground)
                .child(byline(note)),
        )
        .when(extra, |card| {
            card.child(
                v_flex()
                    .gap_1()
                    .mt_1()
                    .pt_1p5()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .line_height(px(16.))
                    .children(description.map(|description| {
                        div().text_color(theme.muted_foreground).child(description)
                    }))
                    .children(findings.into_iter().map(|(severity, message)| {
                        h_flex()
                            .items_start()
                            .gap_1p5()
                            .child(
                                Icon::new(match severity {
                                    Severity::Error => IconName::CircleX,
                                    Severity::Warning => IconName::TriangleAlert,
                                    Severity::Note => IconName::Info,
                                })
                                .small()
                                .flex_none()
                                .mt(px(1.))
                                .text_color(severity_color(severity, cx)),
                            )
                            .child(div().flex_1().min_w_0().child(message))
                    })),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui::SharedString;

    use crate::note_card::byline;
    use crate::{DATASET_MAIN, Diagnostic, Filed, Location, NoteMeta, Severity, Source};

    fn note(author: Option<&str>, date: Option<&str>, edited: bool) -> Diagnostic {
        Diagnostic {
            location: Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                row_id: Some(1),
                column: None,
            },
            severity: Severity::Note,
            source: Source::Note,
            message: "Studio stamp".into(),
            group: None,
            note: Some(NoteMeta {
                id: 1,
                filed: Some(Filed {
                    date: date.map(SharedString::from),
                    author: author.map(SharedString::from),
                }),
                region: None,
                kind: None,
                edited: edited.then(|| Filed {
                    date: Some("2026-10-01 16:05".into()),
                    author: None,
                }),
            }),
        }
    }

    /// Who, when, and whether it changed since — an unsigned note says so rather than leaving a
    /// blank where a name would be.
    #[test]
    fn the_byline_names_the_author_or_says_unsigned() {
        assert_eq!(
            byline(&note(Some("Ravi Kapoor"), Some("2026-09-30 10:12"), true)),
            "Ravi Kapoor · 2026-09-30 10:12 · edited"
        );
        assert_eq!(
            byline(&note(None, Some("2026-10-02 09:30"), false)),
            "Unsigned · 2026-10-02 09:30"
        );
    }
}

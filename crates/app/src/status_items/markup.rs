//! Inline markup for plugin-contributed bar items.
//!
//! A status bar is one line, so this is inline-only: no links, no blocks, no lists. `**bold**`,
//! `*italic*`, `~~strike~~`, `__underline__`, and Rich-style `[green]colour[/]` tags. Note the
//! deviation from CommonMark, where `__` is a second spelling of bold — an underline is worth a
//! spelling here and a duplicate is not.
//!
//! A colour name resolves through the theme rather than to a literal, so `[red]` is whatever red
//! the current theme reads as and stays legible when the user switches to a light one. A name the
//! theme has no answer for renders as the text it is.
//!
//! Deliberately not `gpui_component::text::markdown`: that is a block-level renderer with its own
//! layout and selection behaviour, and `workspace::panels::details` already documents it mangling
//! text that has to render literally.

use std::ops::Range;

use gpui::{
    App, FontStyle, FontWeight, HighlightStyle, Hsla, StrikethroughStyle, StyledText, TextStyle,
    UnderlineStyle, px,
};
use gpui_component::ActiveTheme as _;

/// Longest first, so `**` is never read as two `*`.
const MARKERS: [&str; 4] = ["**", "__", "~~", "*"];

pub fn markup(text: &str, base: &TextStyle, cx: &App) -> StyledText {
    let (plain, runs) = parse(text, &|name| color(name, cx));
    StyledText::new(plain).with_default_highlights(base, runs)
}

/// Both the literal colour names a plugin author reaches for first and the semantic ones the theme
/// actually thinks in. Every answer is a theme role — a hardcoded `#ff0000` would be unreadable on
/// half the themes qrate ships.
fn color(name: &str, cx: &App) -> Option<Hsla> {
    let theme = cx.theme();
    Some(match name {
        "red" | "danger" => theme.danger,
        "green" | "success" => theme.success,
        "yellow" | "warning" => theme.warning,
        "blue" | "info" => theme.info,
        "accent" => theme.accent,
        "muted" => theme.muted_foreground,
        _ => return None,
    })
}

/// What one open span will style when it closes.
enum Open<'a> {
    Marker(&'a str),
    Color(Hsla),
}

/// The marker-stripped text, plus one highlight per matched pair.
///
/// Byte offsets index the stripped text, and only ASCII markup is ever removed, so a multi-byte
/// icon in the input cannot land a range mid-character — which `with_default_highlights` asserts.
fn parse(
    text: &str,
    color: &dyn Fn(&str) -> Option<Hsla>,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut plain = String::with_capacity(text.len());
    let mut runs = Vec::new();
    let mut open: Vec<(Open, usize)> = Vec::new();
    let mut rest = text;

    while let Some(next) = rest.chars().next() {
        if let Some(marker) = MARKERS.iter().find(|marker| rest.starts_with(**marker)) {
            let after = &rest[marker.len()..];
            match open
                .iter()
                .position(|(candidate, _)| matches!(candidate, Open::Marker(m) if m == marker))
            {
                Some(ix) => {
                    let (_, start) = open.remove(ix);
                    runs.push((start..plain.len(), marker_style(marker)));
                }
                // An opener with nothing to close it is not markup, it is text a plugin wrote.
                None if after.contains(marker) => open.push((Open::Marker(marker), plain.len())),
                None => plain.push_str(marker),
            }
            rest = after;
            continue;
        }

        if let Some((tag, after)) = tag(rest) {
            // `[/]` and `[/green]` both close the innermost colour — Rich accepts either, and
            // matching the name up would only let a plugin be wrong in a new way.
            if tag.starts_with('/') {
                if let Some(ix) = open
                    .iter()
                    .rposition(|(candidate, _)| matches!(candidate, Open::Color(_)))
                {
                    let (Open::Color(hsla), start) = open.remove(ix) else {
                        unreachable!("rposition matched a colour")
                    };
                    runs.push((
                        start..plain.len(),
                        HighlightStyle {
                            color: Some(hsla),
                            ..Default::default()
                        },
                    ));
                    rest = after;
                    continue;
                }
            } else if let Some(hsla) = color(tag).filter(|_| after.contains("[/")) {
                open.push((Open::Color(hsla), plain.len()));
                rest = after;
                continue;
            }
        }

        plain.push(next);
        rest = &rest[next.len_utf8()..];
    }
    (plain, runs)
}

/// The name inside a leading `[…]`, and what follows it. Anything containing whitespace is prose,
/// not a tag — `[see note]` is something a plugin meant to print.
fn tag(rest: &str) -> Option<(&str, &str)> {
    let body = rest.strip_prefix('[')?;
    let end = body.find(']')?;
    let name = &body[..end];
    (!name.is_empty() && !name.contains(char::is_whitespace)).then_some((name, &body[end + 1..]))
}

fn marker_style(marker: &str) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    match marker {
        "**" => style.font_weight = Some(FontWeight::BOLD),
        "*" => style.font_style = Some(FontStyle::Italic),
        "~~" => {
            style.strikethrough = Some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            })
        }
        _ => {
            style.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: None,
                wavy: false,
            })
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use gpui::{HighlightStyle, Hsla};
    use std::ops::Range;

    const GREEN: Hsla = Hsla {
        h: 0.3,
        s: 1.,
        l: 0.4,
        a: 1.,
    };

    /// The theme lookup the real renderer does, reduced to the one name these tests need.
    fn parse(text: &str) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
        super::parse(text, &|name| (name == "green").then_some(GREEN))
    }

    fn plain(text: &str) -> String {
        parse(text).0
    }

    #[test]
    fn markers_are_stripped_and_their_spans_highlighted() {
        let (text, runs) = parse("⏳ **Islandora** is ~~offline~~");
        assert_eq!(text, "⏳ Islandora is offline");
        // The icon is three bytes wide; a range counting characters would land mid-icon.
        assert_eq!(runs[0].0, 4..13);
        assert_eq!(&text[runs[0].0.clone()], "Islandora");
        assert_eq!(&text[runs[1].0.clone()], "offline");
    }

    #[test]
    fn each_spelling_reaches_its_own_style() {
        let (_, runs) = parse("**b** *i* ~~s~~ __u__");
        let styles: Vec<_> = runs.iter().map(|(_, style)| *style).collect();
        assert!(styles[0].font_weight.is_some());
        assert!(styles[1].font_style.is_some());
        assert!(styles[2].strikethrough.is_some());
        assert!(styles[3].underline.is_some());
    }

    #[test]
    fn a_colour_tag_paints_what_it_wraps() {
        let (text, runs) = parse("[green]✓ up[/] and away");
        assert_eq!(text, "✓ up and away");
        assert_eq!(&text[runs[0].0.clone()], "✓ up");
        assert_eq!(runs[0].1.color, Some(GREEN));
    }

    #[test]
    fn colour_and_weight_compose() {
        let (text, runs) = parse("[green]**Islandora** ✓[/]");
        assert_eq!(text, "Islandora ✓");
        assert!(runs[0].1.font_weight.is_some(), "the inner ** closes first");
        assert_eq!(runs[1].1.color, Some(GREEN));
        assert_eq!(&text[runs[1].0.clone()], "Islandora ✓");
    }

    /// A plugin writing `2 * 3`, `snake_case`, or `[see the docs]` gets what it wrote, not the rest
    /// of the line swallowed by an opener that never closes.
    #[test]
    fn unmatched_and_unknown_markup_is_text() {
        assert_eq!(plain("2 * 3 things"), "2 * 3 things");
        assert_eq!(plain("**bold** and * a stray"), "bold and * a stray");
        assert_eq!(parse("**bold** and * a stray").1.len(), 1);
        assert_eq!(plain("[green]never closed"), "[green]never closed");
        assert_eq!(
            plain("[mauve]no such colour[/]"),
            "[mauve]no such colour[/]"
        );
        assert_eq!(plain("row [3] of [see note]"), "row [3] of [see note]");
    }

    #[test]
    fn nesting_gives_both_runs() {
        let (text, runs) = parse("**bold *and italic* here**");
        assert_eq!(text, "bold and italic here");
        assert_eq!(&text[runs[0].0.clone()], "and italic");
        assert_eq!(&text[runs[1].0.clone()], "bold and italic here");
    }
}

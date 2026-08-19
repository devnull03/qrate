//! Corrections a diagnostic's producer can offer, and the menu that shows them.
//!
//! The generalisation of what [`crate::spelling`] does for one producer. A [`Diagnostic`] still
//! carries nothing but a sentence — a `fixes` field would make every producer serialise its
//! corrections into the store on every run, most of which nobody ever right-clicks. Instead a
//! producer registers a function here and is asked when a menu opens, against the cell's text
//! *now* rather than the text that produced the finding.
//!
//! Keyed by the same string [`Source::label`] returns, so a producer's findings and its fixes
//! meet without either side holding a handle to the other.

use std::collections::BTreeMap;

use gpui::{App, Context, Global, SharedString, Window};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::{Diagnostics, Location};

/// One offered correction: what to call it, and what the cell becomes if it is taken.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fix {
    /// Menu text. Say what the value becomes — the reason it is wrong is already the message.
    pub label: SharedString,
    /// The cell's whole new text, not a fragment, so applying one is a plain write.
    pub replacement: SharedString,
}

/// What a producer is asked when a menu opens: where the finding is, and what the cell says now.
pub type OfferFixes = fn(&Location, &str, &App) -> Vec<Fix>;

/// Which producers can offer corrections, by source name.
#[derive(Default)]
pub struct FixProviders(BTreeMap<SharedString, OfferFixes>);

impl Global for FixProviders {}

impl FixProviders {
    /// Offer corrections for `source`'s findings. Registering again under the same name replaces,
    /// which is what lets a producer that reloads avoid stacking duplicate menus.
    pub fn register(source: &str, offer: OfferFixes, cx: &mut App) {
        cx.default_global::<Self>().0.insert(source.into(), offer);
    }
}

/// Every correction offered for the findings at `location`, in the order their sources sort.
///
/// Public because the cell renderer wants to know whether *any* exist without building a menu.
pub fn at(location: &Location, text: &str, cx: &App) -> Vec<Fix> {
    let Some(providers) = cx.try_global::<FixProviders>() else {
        return Vec::new();
    };
    let mut sources: Vec<SharedString> = Diagnostics::at(
        &location.dataset,
        location.row,
        location.column.as_deref(),
        cx,
    )
    .map(|d| d.source.label())
    .collect();
    // One finding per source is enough to ask it; a column flagged twice by the same rule must
    // not offer its corrections twice.
    sources.sort();
    sources.dedup();

    sources
        .iter()
        .filter_map(|source| providers.0.get(source))
        .flat_map(|offer| offer(location, text, cx))
        .collect()
}

/// Add a `Fixes` submenu for the findings at `location`, or return `menu` untouched when nothing
/// here offers one.
///
/// `apply` takes the whole replacement text, the same contract [`crate::spelling::menu`] uses, so
/// a caller needs nothing but its own way of storing a string.
pub fn menu(
    location: &Location,
    text: &SharedString,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    apply: impl Fn(SharedString, &mut App) + Clone + 'static,
) -> PopupMenu {
    let found = at(location, text, cx);
    if found.is_empty() {
        return menu;
    }

    menu.submenu("Fixes", window, cx, move |sub, _window, _cx| {
        found.iter().fold(sub, |sub, fix| {
            let (apply, replacement) = (apply.clone(), fix.replacement.clone());
            sub.item(
                PopupMenuItem::new(fix.label.clone())
                    .on_click(move |_, _, cx| apply(replacement.clone(), cx)),
            )
        })
    })
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — see the note in `lib.rs`'s test module.
    use crate::fixes::{Fix, FixProviders, at};
    use crate::{DATASET_MAIN, Diagnostic, Diagnostics, Location, Severity, Source};
    use gpui::{App, SharedString, TestAppContext};

    fn location(column: &str) -> Location {
        Location {
            dataset: DATASET_MAIN.into(),
            row: Some(0),
            row_id: None,
            column: Some(column.into()),
        }
    }

    fn publish(source: &str, column: &str, cx: &mut App) {
        Diagnostics::set(
            &Source::Validator(source.into()),
            DATASET_MAIN,
            vec![Diagnostic {
                location: location(column),
                severity: Severity::Error,
                source: Source::Validator(source.into()),
                message: "not a known term".into(),
                filed: None,
            }],
            cx,
        );
    }

    fn offer(_: &Location, text: &str, _: &App) -> Vec<Fix> {
        vec![Fix {
            label: format!("Use “{text}s”").into(),
            replacement: format!("{text}s").into(),
        }]
    }

    /// The whole point: a producer that is not the spell checker can offer a correction, and it is
    /// asked against the text as it stands now rather than the text the finding was built from.
    #[gpui::test]
    fn a_registered_source_offers_its_fixes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            publish("LCSH", "Subject", cx);
            FixProviders::register("LCSH", offer, cx);

            let found = at(&location("Subject"), "Photograph", cx);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].replacement, SharedString::from("Photographs"));
        });
    }

    /// A finding from a producer that registered nothing must not manufacture a menu, and neither
    /// must a location with no findings at all.
    #[gpui::test]
    fn nothing_is_offered_without_a_finding_and_a_provider(cx: &mut TestAppContext) {
        cx.update(|cx| {
            publish("files", "Subject", cx);
            FixProviders::register("LCSH", offer, cx);
            assert!(
                at(&location("Subject"), "Photograph", cx).is_empty(),
                "a source with no provider offers nothing"
            );
            assert!(
                at(&location("Elsewhere"), "Photograph", cx).is_empty(),
                "a clean cell offers nothing"
            );
        });
    }

    /// A column flagged repeatedly by one rule is still one offer of that rule's corrections.
    #[gpui::test]
    fn one_source_is_asked_once_however_often_it_flagged(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let repeated = vec![
                Diagnostic {
                    location: location("Subject"),
                    severity: Severity::Error,
                    source: Source::Validator("LCSH".into()),
                    message: "not a known term".into(),
                    filed: None,
                },
                Diagnostic {
                    location: location("Subject"),
                    severity: Severity::Warning,
                    source: Source::Validator("LCSH".into()),
                    message: "also deprecated".into(),
                    filed: None,
                },
            ];
            Diagnostics::set(
                &Source::Validator("LCSH".into()),
                DATASET_MAIN,
                repeated,
                cx,
            );
            FixProviders::register("LCSH", offer, cx);
            assert_eq!(at(&location("Subject"), "Photograph", cx).len(), 1);
        });
    }
}

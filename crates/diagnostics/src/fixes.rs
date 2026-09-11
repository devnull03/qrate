//! Corrections a diagnostic's producer can offer, and the menu that shows them.
//!
//! The generalisation of what [`crate::spelling`] does for one producer. A [`Diagnostic`] still
//! carries nothing but a sentence — a `fixes` field would make every producer serialise its
//! corrections into the store on every run, most of which nobody ever right-clicks. Instead a
//! producer registers a function here and is asked when a menu opens, against the cell's text
//! *now* rather than the text that produced the finding.
//!
//! Keyed by the stable producer identity in [`Source::key`], so display-name changes cannot
//! disconnect a producer's findings from its fixes.

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui::{App, Context, Global, SharedString, Window};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::{DiagnosticHooks, Diagnostics, Location};

/// One offered correction: what to call it, and what the cell becomes if it is taken.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fix {
    /// Menu text. Say what the value becomes — the reason it is wrong is already the message.
    pub label: SharedString,
    /// The cell's whole new text, not a fragment, so applying one is a plain write.
    pub replacement: SharedString,
}

/// What a producer is asked when a menu opens: where the finding is, and what the cell says now.
pub type OfferFixes = fn(&Location, &str, Option<&str>, &App) -> Vec<Fix>;

pub struct FixTarget<'a> {
    pub source: &'a str,
    pub subject: Option<&'a str>,
}

#[derive(Clone)]
pub struct GroupMember {
    pub location: Location,
    pub text: SharedString,
    pub message: SharedString,
    pub subject: Option<SharedString>,
}

#[derive(Clone)]
pub struct GroupFix {
    pub label: SharedString,
    apply: Rc<dyn Fn(&mut App)>,
}

impl GroupFix {
    pub fn replacements(
        label: impl Into<SharedString>,
        replacements: Vec<(Location, SharedString)>,
    ) -> Self {
        Self {
            label: label.into(),
            apply: Rc::new(move |cx| {
                if let Some(hooks) = cx.try_global::<DiagnosticHooks>().copied() {
                    (hooks.set_texts)(replacements.clone(), cx);
                }
            }),
        }
    }

    pub fn action(label: impl Into<SharedString>, apply: impl Fn(&mut App) + 'static) -> Self {
        Self {
            label: label.into(),
            apply: Rc::new(apply),
        }
    }

    pub fn apply(&self, cx: &mut App) {
        (self.apply)(cx);
    }
}

pub type OfferGroupFixes = fn(&[GroupMember], &App) -> Vec<GroupFix>;

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

#[derive(Default)]
pub struct GroupFixProviders(BTreeMap<SharedString, OfferGroupFixes>);

impl Global for GroupFixProviders {}

impl GroupFixProviders {
    pub fn register(source: &str, offer: OfferGroupFixes, cx: &mut App) {
        cx.default_global::<Self>().0.insert(source.into(), offer);
    }
}

/// Every correction offered for the findings at `location`, in the order their sources sort.
///
/// Public because the cell renderer wants to know whether *any* exist without building a menu.
pub fn at(location: &Location, text: &str, cx: &App) -> Vec<Fix> {
    at_subject(location, text, None, None, cx)
}

fn at_subject(
    location: &Location,
    text: &str,
    source: Option<&str>,
    subject: Option<&str>,
    cx: &App,
) -> Vec<Fix> {
    let Some(providers) = cx.try_global::<FixProviders>() else {
        return Vec::new();
    };
    let mut sources: Vec<SharedString> = Diagnostics::at(
        &location.dataset,
        location.row,
        location.column.as_deref(),
        cx,
    )
    .map(|d| d.source.key())
    .filter(|candidate| source.is_none_or(|source| candidate == source))
    .collect();
    // One finding per source is enough to ask it; a column flagged twice by the same rule must
    // not offer its corrections twice.
    sources.sort();
    sources.dedup();

    let mut fixes: Vec<_> = sources
        .iter()
        .filter_map(|source| providers.0.get(source))
        .flat_map(|offer| offer(location, text, subject, cx))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    fixes.retain(|fix| seen.insert((fix.label.clone(), fix.replacement.clone())));
    fixes
}

pub fn group_menu(
    source: &SharedString,
    members: &[GroupMember],
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(offer) = cx
        .try_global::<GroupFixProviders>()
        .and_then(|providers| providers.0.get(source))
        .copied()
    else {
        return menu;
    };
    let mut fixes = offer(members, cx);
    let mut seen = std::collections::BTreeSet::new();
    fixes.retain(|fix| seen.insert(fix.label.clone()));
    if fixes.is_empty() {
        return menu;
    }
    menu.submenu("Resolve group", window, cx, move |sub, _window, _cx| {
        fixes.iter().fold(sub, |sub, fix| {
            let fix = fix.clone();
            sub.item(PopupMenuItem::new(fix.label.clone()).on_click(move |_, _, cx| fix.apply(cx)))
        })
    })
}

/// Add a `Fixes` submenu for the findings at `location`, or return `menu` untouched when nothing
/// here offers one.
///
/// `apply` takes the whole replacement text, the same contract [`crate::spelling::menu`] uses, so
/// a caller needs nothing but its own way of storing a string.
pub fn menu(
    location: &Location,
    text: &SharedString,
    target: Option<FixTarget<'_>>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    apply: impl Fn(SharedString, &mut App) + Clone + 'static,
) -> PopupMenu {
    let found = at_subject(
        location,
        text,
        target.as_ref().map(|target| target.source),
        target.and_then(|target| target.subject),
        cx,
    );
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
    use crate::fixes::{Fix, FixProviders, GroupFix, at, at_subject};
    use crate::{
        DATASET_MAIN, Diagnostic, DiagnosticHooks, Diagnostics, Location, Severity, Source,
    };
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
                group: None,
                filed: None,
            }],
            cx,
        );
    }

    fn offer(_: &Location, text: &str, _: Option<&str>, _: &App) -> Vec<Fix> {
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

    #[gpui::test]
    fn display_names_do_not_disconnect_fix_providers(cx: &mut TestAppContext) {
        cx.update(|cx| {
            publish("capitalization", "Title", cx);
            publish("LCSH", "Title", cx);
            FixProviders::register("capitalization", offer, cx);
            FixProviders::register("LCSH", offer, cx);

            let found = at(&location("Title"), "alice", cx);
            assert_eq!(found.len(), 1, "identical fixes are deduplicated");
            assert_eq!(found[0].replacement, SharedString::from("alices"));
            assert_eq!(
                at_subject(
                    &location("Title"),
                    "alice",
                    Some("capitalization"),
                    Some("alice"),
                    cx,
                )
                .len(),
                1,
                "a diagnostic row asks only its own producer"
            );
        });
    }

    #[gpui::test]
    fn a_group_replacement_uses_the_bulk_edit_hook(cx: &mut TestAppContext) {
        #[derive(Default)]
        struct Applied(Vec<(Location, SharedString)>);
        impl gpui::Global for Applied {}

        cx.update(|cx| {
            cx.set_global(Applied::default());
            cx.set_global(DiagnosticHooks {
                reveal: |_, _| {},
                text_at: |_, _| None,
                set_text: |_, _, _| {},
                set_texts: |replacements, cx| {
                    use gpui::BorrowAppContext as _;
                    cx.update_global::<Applied, _>(|applied, _| applied.0 = replacements);
                },
                revalidate: |_| {},
            });
            let replacements = vec![
                (location("Title"), "Alice".into()),
                (
                    Location::cell(DATASET_MAIN, 1, None, "Title"),
                    "Alice".into(),
                ),
            ];
            GroupFix::replacements("Use Alice", replacements.clone()).apply(cx);
            assert_eq!(cx.global::<Applied>().0, replacements);
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
                    group: None,
                    filed: None,
                },
                Diagnostic {
                    location: location("Subject"),
                    severity: Severity::Warning,
                    source: Source::Validator("LCSH".into()),
                    message: "also deprecated".into(),
                    group: None,
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

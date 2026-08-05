//! The "did you mean" menu, shared by everything that can show one.
//!
//! Lives here rather than in `table` because the Problems panel offers the same fixes as a cell
//! does, and two copies of a menu is two places to fix it. Neither caller links a dictionary —
//! the words come from [`SpellActions`], and what to do with a correction is the caller's closure.

use gpui::{App, Context, SharedString, Window};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::SpellActions;

/// Add a `Spelling` submenu to `menu` for the misspellings in `text`, or return `menu` untouched
/// when spell checking is off or the text is clean.
///
/// `apply` receives the whole corrected text rather than the word pair: replacing the word is the
/// same everywhere, so doing it here leaves each caller with nothing but its own way of storing a
/// string. Every occurrence is replaced, because a word misspelled twice is misspelled twice and
/// a menu item carries no offset.
pub fn menu(
    text: &SharedString,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    apply: impl Fn(SharedString, &mut App) + Clone + 'static,
) -> PopupMenu {
    let Some(actions) = cx.try_global::<SpellActions>().copied() else {
        return menu;
    };
    let found = (actions.suggest)(text, cx);
    if found.is_empty() {
        return menu;
    }
    let text = text.clone();

    menu.submenu("Spelling", window, cx, move |sub, _window, _cx| {
        let sub = found.iter().fold(sub, |sub, (word, suggestions)| {
            suggestions.iter().fold(sub, |sub, suggestion| {
                let (apply, text) = (apply.clone(), text.clone());
                let (word, suggestion) = (word.clone(), suggestion.clone());
                sub.item(
                    PopupMenuItem::new(format!("{word} → {suggestion}")).on_click(
                        move |_, _, cx| {
                            let fixed = text.replace(word.as_ref(), suggestion.as_ref());
                            apply(fixed.into(), cx);
                        },
                    ),
                )
            })
        });
        found.iter().fold(sub.separator(), |sub, (word, _)| {
            let word = word.clone();
            sub.item(
                PopupMenuItem::new(format!("Add “{word}” to dictionary"))
                    .on_click(move |_, _, cx| (actions.add_word)(&word, cx)),
            )
        })
    })
}

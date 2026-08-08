//! The checks qrate ships with: what a column's values have to look like, and what lists they
//! have to appear on.
//!
//! One crate rather than one per check. They differ in what they need — a date is a grammar, an
//! authority is a server — but they are the same thing to everyone else: a rule a column opts
//! into, publishing findings under its own source and offering fixes through the same menu. The
//! two heavier checks stay where their dependencies are: `spellcheck` because of its dictionaries,
//! `table::file_links` because it needs the files index the Details panel already builds.

pub mod authority;
pub mod dates;

use gpui::App;

pub use authority::LCSH;

/// Register every built-in check. Called by `app`, which is the only place that knows where
/// checks come from.
pub fn init(cx: &mut App) {
    authority::init(cx);
    dates::init(cx);
}

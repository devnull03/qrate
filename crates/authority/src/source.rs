//! What an authority file has to tell qrate, and the list of the ones it knows.
//!
//! Everything that is the same for every authority — when to ask, what to cache, how often to
//! call out, how to turn an answer into a finding — lives in [`crate::check`]. A source supplies
//! only the two things that differ: the URL that looks a term up, and how to read the labels out
//! of what comes back. Adding VIAF or Getty is that pair of methods and a line in [`ALL`].

use crate::lcsh::Lcsh;

/// One authority file: a controlled list of headings a value is supposed to come from.
///
/// One lookup answers both questions this crate asks — "is this a real heading?" and "then what
/// were they reaching for?" — because a near-match list containing the term *is* the term
/// existing. Two endpoints would double the traffic to say the same thing.
pub trait AuthoritySource: Send + Sync + 'static {
    /// What the Problems panel shows, what a column's `authority` setting names, and the key its
    /// findings are replaced by. Stable across runs.
    fn name(&self) -> &'static str;

    /// A sentence for the finding when `term` isn't on the list. Written per source because
    /// "not a Library of Congress subject heading" tells someone where to go and "not found"
    /// does not.
    fn rejection(&self, term: &str) -> String;

    /// Where to ask about `term`.
    fn lookup_url(&self, term: &str) -> String;

    /// The headings the response offers, best first.
    ///
    /// `None` means the answer could not be read at all — a truncated body, an error page, a
    /// shape this build doesn't know. `Some(empty)` means the authority answered and holds
    /// nothing matching, which is the whole point: an empty list is a *rejection*, and confusing
    /// the two is the one mistake this crate must not make. Nothing is cached or reported for a
    /// `None`.
    fn labels(&self, body: &str) -> Option<Vec<String>>;
}

/// Every authority qrate can check against. A column names one of these in its settings.
pub fn all() -> Vec<Box<dyn AuthoritySource>> {
    vec![Box::new(Lcsh)]
}

/// The names, for the column settings picker.
pub fn names() -> Vec<&'static str> {
    all().iter().map(|source| source.name()).collect()
}

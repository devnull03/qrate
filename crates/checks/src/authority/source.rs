//! What an authority file has to tell qrate, and the list of the ones it knows.
//!
//! Everything that is the same for every authority — when to ask, what to cache, how often to
//! call out, how to turn an answer into a finding — lives in [`super::check`]. A source supplies
//! only what differs: the URL that looks a term up, and how to read the labels out of what comes
//! back. Adding one is that pair of methods and a line in [`all`].

use gpui::App;

use crate::authority::{geonames::GeoNames, lcsh::Lcsh, wikidata::Wikidata};

/// One authority file: a controlled list of headings a value is supposed to come from.
///
/// One lookup answers both questions this crate asks — "is this a real heading?" and "then what
/// were they reaching for?" — because a near-match list containing the term *is* the term
/// existing. Two endpoints would double the traffic to say the same thing.
pub trait AuthoritySource: Send + Sync + 'static {
    /// What the Problems panel shows, what a column's `authority` setting names, and the key its
    /// findings are replaced by. Stable across runs.
    fn name(&self) -> &'static str;

    /// One line for the settings picker, saying what this list is *for*. Someone choosing between
    /// them is picking a vocabulary, not a website.
    fn describes(&self) -> &'static str;

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

    /// Why this source cannot run right now, phrased for the person who has to fix it.
    ///
    /// A source needing an account says so here, and the run skips it. The alternative — asking
    /// anyway and getting an error page back — reads as every value being unknown, which sends
    /// someone hunting for a problem in their data that is really a problem in their settings.
    fn unavailable(&self) -> Option<String> {
        None
    }
}

/// Settings the sources need, read once on the UI thread and carried to the background — which is
/// why this is data and not a `&App`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// GeoNames refuses every anonymous call, so its check is off until this is filled in.
    pub geonames_username: String,
}

impl Config {
    pub fn read(cx: &App) -> Self {
        // `effective_text` needs the settings global, which a test app context has no reason to set.
        if !cx.has_global::<settings::AppSettings>() {
            return Self::default();
        }
        Self {
            geonames_username: settings::effective_text(crate::GEONAMES_USERNAME_KEY, cx)
                .trim()
                .to_string(),
        }
    }
}

/// Every authority qrate can check against. A column names one of these in its settings.
pub fn all(config: &Config) -> Vec<Box<dyn AuthoritySource>> {
    vec![
        Box::new(Lcsh),
        Box::new(Wikidata),
        Box::new(GeoNames {
            username: config.geonames_username.clone(),
        }),
    ]
}

/// The names, in the order the settings picker offers them.
///
/// A separate list from [`all`] so the cache and the fix registry — neither of which cares what a
/// source is configured with — do not have to invent a [`Config`] to ask.
pub const NAMES: [&str; 3] = [
    super::lcsh::NAME,
    super::wikidata::NAME,
    super::geonames::NAME,
];

#[cfg(test)]
mod tests {
    use crate::authority::source::{Config, NAMES, all};

    /// The two lists are written by hand and used for different things; they must not drift.
    #[test]
    fn every_source_is_named_once_and_the_lists_agree() {
        let built: Vec<&str> = all(&Config::default())
            .iter()
            .map(|source| source.name())
            .collect();
        assert_eq!(built, NAMES.to_vec());

        let mut unique = NAMES.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), NAMES.len(), "names are the replace-by key");
    }

    /// Availability is about configuration, not the network, so it has to be answerable offline.
    #[test]
    fn only_geonames_needs_configuring() {
        let unconfigured = all(&Config::default());
        let blocked: Vec<&str> = unconfigured
            .iter()
            .filter(|s| s.unavailable().is_some())
            .map(|s| s.name())
            .collect();
        assert_eq!(blocked, vec![super::super::geonames::NAME]);

        let configured = all(&Config {
            geonames_username: "someone".into(),
        });
        assert!(configured.iter().all(|s| s.unavailable().is_none()));
    }
}

//! Wikidata, over the `wbsearchentities` API.
//!
//! The catch-all of the three, and the only one that will have heard of a local photographer, a
//! defunct cannery, or a school that closed in 1974 — which is most of what a regional collection
//! is about. Needs no key and no account.
//!
//! It is also the loosest: it searches every entity Wikidata holds, so a value matching something
//! says only that the string names *a* thing, not that it names a thing of the right kind. Point a
//! column at it when the alternative is checking nothing.

use crate::authority::source::AuthoritySource;

pub struct Wikidata;

/// What a column's `authority` setting names to be checked against this list.
pub const NAME: &str = "Wikidata";

/// Near matches read out of one answer, which is a menu length rather than a network budget.
const SUGGESTIONS: usize = 10;

impl AuthoritySource for Wikidata {
    fn name(&self) -> &'static str {
        NAME
    }

    fn describes(&self) -> &'static str {
        "Anything with a Wikidata entry — people, places, organisations, works. The broadest \
         list, and the loosest: a match means the value names something, not that it names \
         something of the right kind."
    }

    fn rejection(&self, term: &str) -> String {
        format!("“{term}” does not match anything in Wikidata")
    }

    fn lookup_url(&self, term: &str) -> String {
        format!(
            "https://www.wikidata.org/w/api.php?action=wbsearchentities&format=json\
             &language=en&uselang=en&type=item&limit={SUGGESTIONS}&search={}",
            super::encode(term)
        )
    }

    fn labels(&self, body: &str) -> Option<Vec<String>> {
        let parsed = serde_json::from_str::<serde_json::Value>(body).ok()?;
        // The API reports its own failures in an `error` object with no `search` key, so a missing
        // array is a body we could not read rather than a term nobody has heard of.
        let hits = parsed["search"].as_array()?;
        Some(
            hits.iter()
                .filter_map(|hit| hit["label"].as_str().map(str::to_owned))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::authority::source::AuthoritySource;
    use crate::authority::wikidata::Wikidata;

    #[test]
    fn reads_the_entity_labels_out_of_an_answer() {
        let body = r#"{"searchinfo":{"search":"Vancouver"},"search":[
            {"id":"Q24639","label":"Vancouver","description":"largest city in BC"},
            {"id":"Q234053","label":"Vancouver","description":"city in Washington"}
        ],"success":1}"#;
        assert_eq!(
            Wikidata.labels(body).unwrap(),
            vec!["Vancouver", "Vancouver"]
        );
    }

    /// An empty `search` array is Wikidata saying it holds nothing — a rejection, which must
    /// survive as one. An `error` object carries no array at all and is not an answer.
    #[test]
    fn an_empty_search_is_a_rejection_and_an_error_is_not_an_answer() {
        assert_eq!(
            Wikidata.labels(r#"{"search":[],"success":1}"#),
            Some(Vec::new())
        );
        assert_eq!(
            Wikidata.labels(r#"{"error":{"code":"param-missing"}}"#),
            None
        );
        assert_eq!(Wikidata.labels("<html>502</html>"), None);
    }

    #[test]
    fn never_needs_configuring() {
        assert!(Wikidata.unavailable().is_none());
    }
}

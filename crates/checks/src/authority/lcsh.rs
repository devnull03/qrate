//! Library of Congress Subject Headings, over `id.loc.gov`.
//!
//! Uses the `suggest2` endpoint rather than the full search: it is the one built for typing at,
//! it needs no key and no account, and it answers with the authorized label of every near match.
//! That single answer settles both questions — a heading that exists comes back as an exact
//! label, and one that doesn't comes back as the list to offer instead.

use crate::authority::source::AuthoritySource;

pub struct Lcsh;

/// What a column's `authority` setting names to be checked against this list.
pub const NAME: &str = "LCSH";

/// Near matches to read out of one answer. The menu shows these, so it is a menu length, not a
/// network budget — the request costs the same either way.
const SUGGESTIONS: usize = 10;

impl AuthoritySource for Lcsh {
    fn name(&self) -> &'static str {
        NAME
    }

    fn rejection(&self, term: &str) -> String {
        format!("“{term}” is not a Library of Congress subject heading")
    }

    fn lookup_url(&self, term: &str) -> String {
        format!(
            "https://id.loc.gov/authorities/subjects/suggest2?q={}&count={SUGGESTIONS}",
            encode(term)
        )
    }

    fn labels(&self, body: &str) -> Option<Vec<String>> {
        let parsed = serde_json::from_str::<serde_json::Value>(body).ok()?;
        // No `hits` array at all is a body we don't understand, not an empty result — the
        // endpoint always sends one, even when it found nothing.
        let hits = parsed["hits"].as_array()?;
        Some(
            hits.iter()
                .filter_map(|hit| {
                    // `aLabel` is the authorized heading; `suggestLabel` is what the endpoint
                    // shows while typing and can carry a qualifier the heading itself lacks.
                    hit["aLabel"]
                        .as_str()
                        .or_else(|| hit["suggestLabel"].as_str())
                        .map(str::to_owned)
                })
                .collect(),
        )
    }
}

/// Percent-encode a query value. Hand-rolled because this is the only escaping in the crate and
/// the alternative is a dependency to encode a dozen characters.
fn encode(term: &str) -> String {
    term.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::authority::lcsh::{Lcsh, encode};
    use crate::authority::source::AuthoritySource;

    #[test]
    fn reads_the_authorized_labels_out_of_an_answer() {
        let body = r#"{"count":2,"hits":[
            {"aLabel":"Photographs","suggestLabel":"Photographs"},
            {"aLabel":"Photograph collections","suggestLabel":"Photograph collections"}
        ]}"#;
        assert_eq!(
            Lcsh.labels(body).unwrap(),
            vec!["Photographs", "Photograph collections"]
        );
    }

    /// A body that isn't the JSON we expect must read as "no answer", never as "no such heading" —
    /// the difference between a server having a bad day and a cataloguer being wrong.
    #[test]
    fn an_unreadable_answer_is_not_an_answer() {
        assert_eq!(Lcsh.labels("<html>502 Bad Gateway</html>"), None);
        assert_eq!(Lcsh.labels("{}"), None);
        assert_eq!(Lcsh.labels(r#"{"hits":"nonsense"}"#), None);
    }

    /// The distinction the whole crate turns on: the endpoint always sends a `hits` array, so an
    /// empty one is the authority saying "no such heading" and has to survive as a rejection.
    #[test]
    fn an_empty_hit_list_is_a_rejection_not_a_missing_answer() {
        assert_eq!(Lcsh.labels(r#"{"count":0,"hits":[]}"#), Some(Vec::new()));
    }

    #[test]
    fn a_heading_with_punctuation_survives_the_url() {
        assert_eq!(encode("World War, 1939-1945"), "World%20War%2C%201939-1945");
        assert!(
            Lcsh.lookup_url("Bridges--Design")
                .contains("Bridges--Design")
        );
    }
}

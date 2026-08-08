//! GeoNames, over `searchJSON`.
//!
//! The place list the proposal names alongside Getty TGN, and the practical one: it covers
//! populated places, and it will resolve a village that a subject-heading list has never carried.
//!
//! Unlike the other two it refuses anonymous calls — every request needs the username of a free
//! account. So this source reports itself [unavailable](AuthoritySource::unavailable) until one is
//! set, rather than asking anyway and reading the refusal as "no such place".

use crate::authority::source::AuthoritySource;

pub struct GeoNames {
    /// A free geonames.org account name. Empty means the check is off.
    pub username: String,
}

/// What a column's `authority` setting names to be checked against this list.
pub const NAME: &str = "GeoNames";

/// Near matches read out of one answer, which is a menu length rather than a network budget.
const SUGGESTIONS: usize = 10;

impl AuthoritySource for GeoNames {
    fn name(&self) -> &'static str {
        NAME
    }

    fn describes(&self) -> &'static str {
        "Populated places, regions, and physical features worldwide. Needs a free geonames.org \
         account name, set on this page."
    }

    fn rejection(&self, term: &str) -> String {
        format!("“{term}” is not a place GeoNames knows")
    }

    fn lookup_url(&self, term: &str) -> String {
        // `secure.` because the bare host is HTTP only, and the username is a credential of sorts.
        format!(
            "https://secure.geonames.org/searchJSON?maxRows={SUGGESTIONS}&style=SHORT&q={}&username={}",
            super::encode(term),
            super::encode(&self.username)
        )
    }

    fn labels(&self, body: &str) -> Option<Vec<String>> {
        let parsed = serde_json::from_str::<serde_json::Value>(body).ok()?;
        // Every refusal — bad username, spent daily credits, rate limit — comes back as a `status`
        // object with no `geonames` array. Reading that as "no such place" would redden a whole
        // column over an account problem, so it is not an answer.
        let Some(hits) = parsed["geonames"].as_array() else {
            if let Some(message) = parsed["status"]["message"].as_str() {
                log::warn!(
                    "GeoNames refused the lookup, so places are not being checked: {message}"
                );
            }
            return None;
        };
        Some(
            hits.iter()
                .filter_map(|hit| hit["name"].as_str().map(str::to_owned))
                .collect(),
        )
    }

    fn unavailable(&self) -> Option<String> {
        self.username.trim().is_empty().then(|| {
            "GeoNames needs a free account name before it can check anything — Settings ▸ \
             Authorities."
                .to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::authority::geonames::GeoNames;
    use crate::authority::source::AuthoritySource;

    fn configured() -> GeoNames {
        GeoNames {
            username: "someone".into(),
        }
    }

    #[test]
    fn reads_the_place_names_out_of_an_answer() {
        let body = r#"{"totalResultsCount":2,"geonames":[
            {"name":"Vancouver","countryName":"Canada"},
            {"name":"Vancouver Island","countryName":"Canada"}
        ]}"#;
        assert_eq!(
            configured().labels(body).unwrap(),
            vec!["Vancouver", "Vancouver Island"]
        );
    }

    /// The distinction that keeps an account problem from reading as bad data: a refusal carries a
    /// `status` object and no array, so it is not an answer — while a genuine miss is an empty one.
    #[test]
    fn a_refusal_is_not_an_answer_but_an_empty_result_is() {
        let refused = r#"{"status":{"message":"the daily limit of 20000 credits for demo has been exceeded","value":18}}"#;
        assert_eq!(configured().labels(refused), None);
        assert_eq!(
            configured().labels(r#"{"totalResultsCount":0,"geonames":[]}"#),
            Some(Vec::new())
        );
    }

    /// Without an account every call is refused, so the run must skip the source rather than
    /// spend a request per term learning that.
    #[test]
    fn says_so_when_it_has_no_account() {
        let blank = GeoNames {
            username: "  ".into(),
        };
        assert!(blank.unavailable().is_some());
        assert!(configured().unavailable().is_none());
    }

    #[test]
    fn the_username_reaches_the_url_escaped() {
        let odd = GeoNames {
            username: "a b&c".into(),
        };
        assert!(odd.lookup_url("Hope").contains("username=a%20b%26c"));
    }
}

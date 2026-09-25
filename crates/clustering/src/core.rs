//! Pair evidence joined into review clusters. A cluster is advisory until the user resolves it.
use caseless::default_case_fold_str;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// A value's comparable forms. Normalising is most of what clustering a column costs, and almost
/// every value in an edited column is one it already had.
#[derive(Debug)]
pub struct Tokens {
    strict: Vec<String>,
    sorted: Vec<String>,
    loose: Vec<String>,
}

/// Displayed text → its tokens, kept across runs by the caller.
pub type TokenCache = HashMap<String, Arc<Tokens>>;

/// ponytail: past this many distinct values the cache is dropped wholesale, as spelling's is.
const MAX_TOKENIZED: usize = 200_000;

#[derive(Clone, Debug)]
pub struct Value {
    pub displayed: String,
    pub rows: Vec<usize>,
    tokens: Arc<Tokens>,
}

impl Value {
    fn new(displayed: &str, mut rows: Vec<usize>, cache: &mut TokenCache) -> Self {
        rows.sort_unstable();
        rows.dedup();
        let tokens = match cache.get(displayed) {
            Some(tokens) => tokens.clone(),
            None => {
                let strict = tokens(displayed, false);
                let mut sorted = strict.clone();
                sorted.sort();
                let found = Arc::new(Tokens {
                    strict,
                    sorted,
                    loose: tokens(displayed, true),
                });
                cache.insert(displayed.to_owned(), found.clone());
                found
            }
        };
        Self {
            displayed: displayed.to_owned(),
            rows,
            tokens,
        }
    }
}

/// Two distinct values, by index into [`Comparison::values`].
#[derive(Clone, Copy, Debug)]
pub struct Pair {
    pub left: usize,
    pub right: usize,
    pub reason: &'static str,
}

pub struct Comparison {
    /// Every distinct value, in text order.
    pub values: Vec<Value>,
    pub pairs: Vec<Pair>,
    pub candidate_pairs: usize,
}

#[derive(Clone, Debug)]
pub struct Cluster<'a> {
    pub members: Vec<&'a Value>,
    pub reasons: Vec<&'static str>,
    pub key: String,
}

fn tokens(text: &str, strip_marks: bool) -> Vec<String> {
    let normalized: String = text.nfkc().collect();
    let folded = default_case_fold_str(&normalized);
    let comparable = if strip_marks {
        folded.nfkd().filter(|c| !is_combining_mark(*c)).collect()
    } else {
        folded
    };
    comparable
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn one_close_token<'a>(
    left: impl ExactSizeIterator<Item = &'a String>,
    right: &'a [String],
) -> bool {
    if left.len() != right.len() || right.len() < 2 {
        return false;
    }
    let mut changed = left.zip(right).filter(|(a, b)| a != b);
    let (Some((a, b)), None) = (changed.next(), changed.next()) else {
        return false;
    };
    a.chars().all(char::is_alphabetic)
        && b.chars().all(char::is_alphabetic)
        && strsim::normalized_damerau_levenshtein(a, b) >= 0.8
}

fn abbreviated_or_extra(left: &[String], right: &[String]) -> Option<&'static str> {
    let (short, long) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    // ponytail: one extra word at most, so "New York" never swallows "New York Times Magazine".
    if short.len() < 2 || long.len() > short.len() + 1 {
        return None;
    }
    let mut rest: Vec<&str> = long.iter().map(String::as_str).collect();
    let mut unmatched = Vec::new();
    for token in short {
        match rest.iter().position(|other| other == token) {
            Some(ix) => {
                rest.swap_remove(ix);
            }
            None => unmatched.push(token.as_str()),
        }
    }
    if unmatched.len() == short.len() {
        return None;
    }
    let initial = |a: &str, b: &str| {
        a.chars().count() == 1
            && a.chars().all(char::is_alphabetic)
            && b.len() > a.len()
            && b.starts_with(a)
    };
    let abbreviated = !unmatched.is_empty();
    for token in unmatched {
        let ix = rest
            .iter()
            .position(|other| initial(token, other) || initial(other, token))?;
        rest.swap_remove(ix);
    }
    match rest.as_slice() {
        [] if abbreviated => Some("a word is abbreviated"),
        [extra] if extra.chars().all(char::is_alphabetic) => Some(if abbreviated {
            "a word is abbreviated"
        } else {
            "one value has an extra word"
        }),
        _ => None,
    }
}

fn reason(left: &Value, right: &Value) -> Option<&'static str> {
    let (left, right) = (&left.tokens, &right.tokens);
    if left.strict.is_empty() || right.strict.is_empty() {
        return None;
    }
    if left.strict == right.strict {
        return Some("formatting differs");
    }
    if left.sorted == right.sorted {
        return Some("the same words appear in a different order");
    }
    if left.loose == right.loose {
        return Some("accents or diacritics differ");
    }
    if one_close_token(left.strict.iter(), &right.strict)
        || one_close_token(left.strict.iter().rev(), &right.strict)
    {
        return Some("one word has a close spelling");
    }
    abbreviated_or_extra(&left.strict, &right.strict)
}

fn candidate_pairs(values: &[Value]) -> BTreeSet<(usize, usize)> {
    let mut exact: HashMap<&[String], Vec<usize>> = HashMap::new();
    let mut ordered: HashMap<&[String], Vec<usize>> = HashMap::new();
    let mut loose: HashMap<&[String], Vec<usize>> = HashMap::new();
    let mut shared: HashMap<&str, Vec<usize>> = HashMap::new();
    for (ix, value) in values.iter().enumerate() {
        let tokens = &value.tokens;
        if tokens.strict.is_empty() {
            continue;
        }
        exact.entry(&tokens.strict).or_default().push(ix);
        ordered.entry(&tokens.sorted).or_default().push(ix);
        loose.entry(&tokens.loose).or_default().push(ix);
        for token in tokens.strict.iter().collect::<BTreeSet<_>>() {
            shared.entry(token.as_str()).or_default().push(ix);
        }
    }
    let mut pairs = BTreeSet::new();
    for group in exact
        .into_values()
        .chain(ordered.into_values())
        .chain(loose.into_values())
        .chain(shared.into_values().filter(|group| group.len() <= 100))
    {
        for (offset, &left) in group.iter().enumerate() {
            for &right in &group[offset + 1..] {
                pairs.insert((left.min(right), left.max(right)));
            }
        }
    }
    pairs
}

pub fn compare_indexed<'a>(
    values: impl IntoIterator<Item = (usize, &'a str)>,
    cache: &mut TokenCache,
) -> Comparison {
    let mut grouped: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (row, value) in values {
        if !value.trim().is_empty() {
            grouped.entry(value).or_default().push(row);
        }
    }
    if cache.len() > MAX_TOKENIZED {
        cache.clear();
    }
    let distinct: Vec<_> = grouped
        .into_iter()
        .map(|(text, rows)| Value::new(text, rows, cache))
        .collect();
    let candidates = candidate_pairs(&distinct);
    let candidate_count = candidates.len();
    let pairs = candidates
        .into_iter()
        .filter_map(|(left, right)| {
            Some(Pair {
                reason: reason(&distinct[left], &distinct[right])?,
                left,
                right,
            })
        })
        .collect();
    Comparison {
        values: distinct,
        pairs,
        candidate_pairs: candidate_count,
    }
}

/// Connected pairs, each cluster's members in text order.
pub fn clusters<'a>(values: &'a [Value], pairs: &[Pair]) -> Vec<Cluster<'a>> {
    let mut links: HashMap<usize, Vec<(usize, &'static str)>> = HashMap::new();
    for pair in pairs {
        links
            .entry(pair.left)
            .or_default()
            .push((pair.right, pair.reason));
        links
            .entry(pair.right)
            .or_default()
            .push((pair.left, pair.reason));
    }

    let mut unseen: BTreeSet<usize> = links.keys().copied().collect();
    let mut clusters = Vec::new();
    while let Some(start) = unseen.pop_first() {
        let mut members = BTreeSet::from([start]);
        let mut reasons = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(ix) = stack.pop() {
            for &(neighbor, reason) in links.get(&ix).into_iter().flatten() {
                reasons.insert(reason);
                if unseen.remove(&neighbor) {
                    members.insert(neighbor);
                    stack.push(neighbor);
                }
            }
        }
        let members: Vec<&Value> = members.into_iter().map(|ix| &values[ix]).collect();
        let names: BTreeSet<&str> = members.iter().map(|v| v.displayed.as_str()).collect();
        clusters.push(Cluster {
            key: format!("{names:?}"),
            members,
            reasons: reasons.into_iter().collect(),
        });
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::{Comparison, TokenCache, Value, candidate_pairs, clusters, compare_indexed};

    fn compare<'a>(values: impl IntoIterator<Item = &'a str>) -> Comparison {
        compare_indexed(values.into_iter().enumerate(), &mut TokenCache::default())
    }

    fn shown(found: &Comparison, ix: usize) -> &str {
        &found.values[ix].displayed
    }

    #[test]
    fn evidence_and_frequencies_preserve_exact_text() {
        let found = compare([
            "Agnès Varda",
            "Varda, Agnès",
            "Agnes Varda",
            "Agnès Varad",
            "Werner Herzog",
        ]);
        for why in ["different order", "diacritics differ", "close spelling"] {
            assert!(found.pairs.iter().any(|p| p.reason.contains(why)));
        }
        assert!(
            found
                .pairs
                .iter()
                .all(|p| !shown(&found, p.left).contains("Werner")
                    && !shown(&found, p.right).contains("Werner"))
        );
        assert!(compare(["Agnès Varda", "Agnès Varda"]).pairs.is_empty());
        let found = compare(["Alice", " Alice ", "Alice"]);
        let pair = found.pairs[0];
        assert_eq!(shown(&found, pair.left), " Alice ");
        assert_eq!(found.values[pair.right].rows, [0, 2]);
    }
    #[test]
    fn abbreviations_and_extra_words_join_one_cluster() {
        let compared = compare([
            "Bains, Satwinder",
            "Satwinder, Bains",
            "Dr. Satwinder Bains",
            "Satinder Bains",
            "Satwinder B.",
            "Satwinder Singh Bains Kaur",
            "Satwinder",
        ]);
        let found = clusters(&compared.values, &compared.pairs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members.len(), 5);
        assert!(
            compare(["New York", "New York Times Magazine"])
                .pairs
                .is_empty()
        );
        assert!(compare(["Bains", "Dr. Bains"]).pairs.is_empty());
    }

    #[test]
    fn blocking_bounds_two_thousand_values() {
        let mut cache = TokenCache::default();
        let values: Vec<_> = (0..2000)
            .map(|ix| {
                Value::new(
                    &format!("group{} value{}", ix / 100, ix % 100),
                    vec![ix],
                    &mut cache,
                )
            })
            .collect();
        assert!(candidate_pairs(&values).len() <= 120_000);
    }

    #[test]
    fn unicode_normalization_does_not_change_displayed_values() {
        for (left, right) in [
            ("Straße", "STRASSE"),
            ("Ａｌｉｃｅ", "Alice"),
            ("Cafe\u{301}", "Café"),
        ] {
            let found = compare([left, right]);
            assert_eq!(found.pairs.len(), 1);
            assert_eq!(found.pairs[0].reason, "formatting differs");
            assert!([left, right].contains(&shown(&found, found.pairs[0].left)));
            assert!([left, right].contains(&shown(&found, found.pairs[0].right)));
        }
        assert!(
            compare(["Film 12345", "Film 12346", "...", "---"])
                .pairs
                .is_empty()
        );
    }

    #[test]
    fn pair_evidence_keeps_the_observed_edges() {
        assert_eq!(
            compare(["Alice abcde", "Alice abcdf", "Alice abcdg"])
                .pairs
                .len(),
            3
        );
        let found = compare(["Alice abcde", "Alice abcdf", "Alice abccf"]);
        assert_eq!(found.pairs.len(), 2);
        assert!(
            found
                .pairs
                .iter()
                .all(|p| !(shown(&found, p.left) == "Alice abccf"
                    && shown(&found, p.right) == "Alice abcde"))
        );
    }

    #[test]
    fn connected_pair_evidence_forms_one_review_cluster() {
        let found = compare(["Alice abcde", "Alice abcdf", "Alice abccf"]);
        let clusters = clusters(&found.values, &found.pairs);
        assert_eq!(clusters.len(), 1);
        assert_eq!(
            clusters[0]
                .members
                .iter()
                .map(|member| member.displayed.as_str())
                .collect::<Vec<_>>(),
            ["Alice abccf", "Alice abcde", "Alice abcdf"]
        );
        assert_eq!(clusters[0].reasons, ["one word has a close spelling"]);
    }

    /// Each distinct value is normalised once; a later run over the same text reuses the result.
    #[test]
    fn a_second_comparison_reuses_the_tokenized_values() {
        let mut cache = TokenCache::default();
        let first = compare_indexed(
            ["Agnès Varda", "Varda, Agnès"].into_iter().enumerate(),
            &mut cache,
        );
        assert_eq!(cache.len(), 2);
        let second = compare_indexed(
            ["Agnès Varda", "Agnes Varda"].into_iter().enumerate(),
            &mut cache,
        );
        assert_eq!(cache.len(), 3, "only the new value was tokenized");
        assert!(std::sync::Arc::ptr_eq(
            &first.values[0].tokens,
            &second.values[1].tokens
        ));
        assert_eq!(second.pairs.len(), 1);
    }
}

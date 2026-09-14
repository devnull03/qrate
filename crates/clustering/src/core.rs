//! Pair evidence joined into review clusters. A cluster is advisory until the user resolves it.
use caseless::default_case_fold_str;
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

#[derive(Clone, Debug)]
pub struct Value {
    pub displayed: String,
    pub rows: Vec<usize>,
    strict: Vec<String>,
    sorted: Vec<String>,
    loose: Vec<String>,
}

impl Value {
    fn new(displayed: String, mut rows: Vec<usize>) -> Self {
        rows.sort_unstable();
        rows.dedup();
        let strict = tokens(&displayed, false);
        let mut sorted = strict.clone();
        sorted.sort();
        let loose = tokens(&displayed, true);
        Self {
            displayed,
            rows,
            strict,
            sorted,
            loose,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub left: Value,
    pub right: Value,
    pub reason: &'static str,
    pub key: String,
}

pub struct Comparison {
    pub pairs: Vec<Pair>,
    pub distinct_values: usize,
    pub candidate_pairs: usize,
}

#[derive(Clone, Debug)]
pub struct Cluster {
    pub members: Vec<Value>,
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

fn one_close_token(left: &[String], right: &[String]) -> bool {
    if left.len() != right.len() || left.is_empty() {
        return false;
    }
    let changed: Vec<_> = left.iter().zip(right).filter(|(a, b)| a != b).collect();
    changed.len() == 1
        && left.len() >= 2
        && changed[0].0.chars().all(char::is_alphabetic)
        && changed[0].1.chars().all(char::is_alphabetic)
        && strsim::normalized_damerau_levenshtein(changed[0].0, changed[0].1) >= 0.8
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
    if one_close_token(&left.strict, &right.strict)
        || one_close_token(
            &left.strict.iter().rev().cloned().collect::<Vec<_>>(),
            &right.strict,
        )
    {
        return Some("one word has a close spelling");
    }
    abbreviated_or_extra(&left.strict, &right.strict)
}

fn candidate_pairs(values: &[Value]) -> BTreeSet<(usize, usize)> {
    let mut exact: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    let mut ordered: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    let mut loose: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    let mut shared: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (ix, value) in values.iter().enumerate() {
        if value.strict.is_empty() {
            continue;
        }
        exact.entry(value.strict.clone()).or_default().push(ix);
        ordered.entry(value.sorted.clone()).or_default().push(ix);
        loose.entry(value.loose.clone()).or_default().push(ix);
        for token in value.strict.iter().collect::<BTreeSet<_>>() {
            shared.entry(token.clone()).or_default().push(ix);
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

pub fn compare<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<Pair> {
    let comparison = compare_indexed(values.into_iter().enumerate());
    comparison.pairs
}

pub fn compare_indexed<'a>(values: impl IntoIterator<Item = (usize, &'a str)>) -> Comparison {
    let mut grouped: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (row, value) in values {
        if !value.trim().is_empty() {
            grouped.entry(value.to_owned()).or_default().push(row);
        }
    }
    let distinct: Vec<_> = grouped
        .into_iter()
        .map(|(text, rows)| Value::new(text, rows))
        .collect();
    let candidates = candidate_pairs(&distinct);
    let candidate_count = candidates.len();
    let pairs = candidates
        .into_iter()
        .filter_map(|(left, right)| {
            let (left, right) = (&distinct[left], &distinct[right]);
            Some(Pair {
                reason: reason(left, right)?,
                key: format!("{:?}", (&left.displayed, &right.displayed)),
                left: left.clone(),
                right: right.clone(),
            })
        })
        .collect();
    Comparison {
        pairs,
        distinct_values: distinct.len(),
        candidate_pairs: candidate_count,
    }
}

pub fn clusters(pairs: Vec<Pair>) -> Vec<Cluster> {
    let mut values = BTreeMap::new();
    let mut links: BTreeMap<String, Vec<(String, &'static str)>> = BTreeMap::new();
    for pair in pairs {
        let (left, right) = (pair.left.displayed.clone(), pair.right.displayed.clone());
        values.entry(left.clone()).or_insert(pair.left);
        values.entry(right.clone()).or_insert(pair.right);
        links
            .entry(left.clone())
            .or_default()
            .push((right.clone(), pair.reason));
        links.entry(right).or_default().push((left, pair.reason));
    }

    let mut unseen: BTreeSet<_> = values.keys().cloned().collect();
    let mut clusters = Vec::new();
    while let Some(start) = unseen.pop_first() {
        let mut names = BTreeSet::from([start.clone()]);
        let mut reasons = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(name) = stack.pop() {
            for (neighbor, reason) in links.get(&name).into_iter().flatten() {
                reasons.insert(*reason);
                if unseen.remove(neighbor) {
                    names.insert(neighbor.clone());
                    stack.push(neighbor.clone());
                }
            }
        }
        let members = names
            .iter()
            .filter_map(|name| values.remove(name))
            .collect();
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
    use super::{Value, candidate_pairs, clusters, compare};
    #[test]
    fn evidence_and_frequencies_preserve_exact_text() {
        let pairs = compare([
            "Agnès Varda",
            "Varda, Agnès",
            "Agnes Varda",
            "Agnès Varad",
            "Werner Herzog",
        ]);
        for why in ["different order", "diacritics differ", "close spelling"] {
            assert!(pairs.iter().any(|p| p.reason.contains(why)));
        }
        assert!(
            pairs
                .iter()
                .all(|p| !p.left.displayed.contains("Werner")
                    && !p.right.displayed.contains("Werner"))
        );
        assert!(compare(["Agnès Varda", "Agnès Varda"]).is_empty());
        let pair = compare(["Alice", " Alice ", "Alice"]).remove(0);
        assert_eq!(pair.left.displayed, " Alice ");
        assert_eq!(pair.right.rows, [0, 2]);
        assert_eq!(pair.key, compare(["Alice", " Alice "])[0].key);
    }
    #[test]
    fn abbreviations_and_extra_words_join_one_cluster() {
        let found = clusters(compare([
            "Bains, Satwinder",
            "Satwinder, Bains",
            "Dr. Satwinder Bains",
            "Satinder Bains",
            "Satwinder B.",
            "Satwinder Singh Bains Kaur",
            "Satwinder",
        ]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members.len(), 5);
        assert!(compare(["New York", "New York Times Magazine"]).is_empty());
        assert!(compare(["Bains", "Dr. Bains"]).is_empty());
    }

    #[test]
    fn blocking_bounds_two_thousand_values() {
        let values: Vec<_> = (0..2000)
            .map(|ix| Value::new(format!("group{} value{}", ix / 100, ix % 100), vec![ix]))
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
            let pairs = compare([left, right]);
            assert_eq!(pairs.len(), 1);
            assert_eq!(pairs[0].reason, "formatting differs");
            assert!([left, right].contains(&pairs[0].left.displayed.as_str()));
            assert!([left, right].contains(&pairs[0].right.displayed.as_str()));
        }
        assert!(compare(["Film 12345", "Film 12346", "...", "---"]).is_empty());
    }

    #[test]
    fn pair_evidence_keeps_the_observed_edges() {
        let pairs = compare(["Alice abcde", "Alice abcdf", "Alice abcdg"]);
        assert_eq!(pairs.len(), 3);
        let pairs = compare(["Alice abcde", "Alice abcdf", "Alice abccf"]);
        assert_eq!(pairs.len(), 2);
        assert!(pairs.iter().all(|p| !(p.left.displayed == "Alice abccf" && p.right.displayed == "Alice abcde")));
    }

    #[test]
    fn connected_pair_evidence_forms_one_review_cluster() {
        let clusters = clusters(compare(["Alice abcde", "Alice abcdf", "Alice abccf"]));
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
}

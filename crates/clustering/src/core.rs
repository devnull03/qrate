//! Pair evidence for review, not transitive entity identity.
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
    fn new(displayed: String, rows: Vec<usize>) -> Self {
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
    None
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
    compare_indexed(values.into_iter().enumerate())
}

pub fn compare_indexed<'a>(values: impl IntoIterator<Item = (usize, &'a str)>) -> Vec<Pair> {
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
    candidate_pairs(&distinct)
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
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Value, candidate_pairs, compare};
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
    fn pair_evidence_never_closes_a_transitive_chain() {
        let pairs = compare(["Alice abcde", "Alice abcdf", "Alice abcdg"]);
        assert_eq!(pairs.len(), 3);
        let pairs = compare(["Alice abcde", "Alice abcdf", "Alice abccf"]);
        assert_eq!(pairs.len(), 2);
        assert!(pairs.iter().all(|p| !(p.left.displayed == "Alice abccf" && p.right.displayed == "Alice abcde")));
    }
}

//! Opt-in review of inconsistent displayed forms within one column.
//!
//! Similar text is evidence for review, not identity. This validator never merges values and keeps
//! the observed spellings intact so the cataloguer, not an algorithm, chooses a replacement.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use caseless::default_case_fold_str;
use diagnostics::{ColumnInfo, ColumnValidator, Fix, Location, Severity};
use gpui::{App, Global, SharedString};
use settings::columns::ColumnType;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub const VALUE_VARIANTS_NAME: &str = "value variants";

type CandidateMap = BTreeMap<(String, usize), Vec<SharedString>>;

#[derive(Clone, Default)]
pub struct ValueVariants {
    candidates: Arc<RwLock<CandidateMap>>,
}

impl Global for ValueVariants {}

#[derive(Clone)]
struct Value {
    displayed: SharedString,
    rows: Vec<usize>,
    strict: Vec<String>,
    sorted: Vec<String>,
    loose: Vec<String>,
}

impl Value {
    fn new(displayed: SharedString, rows: Vec<usize>) -> Self {
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

fn tokens(text: &str, strip_marks: bool) -> Vec<String> {
    let normalized: String = text.nfkc().collect();
    let folded = default_case_fold_str(&normalized);
    let comparable: String = if strip_marks {
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
    let pairs: Vec<(&str, &str)> = left
        .iter()
        .zip(right)
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let changed: Vec<_> = pairs.iter().filter(|(a, b)| a != b).collect();
    let stable = pairs.len() - changed.len();
    changed.len() == 1
        && stable >= 1
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
        exact.entry(value.strict.clone()).or_default().push(ix);
        ordered.entry(value.sorted.clone()).or_default().push(ix);
        loose.entry(value.loose.clone()).or_default().push(ix);
        for token in value.strict.iter().collect::<BTreeSet<_>>() {
            shared.entry(token.clone()).or_default().push(ix);
        }
    }

    let mut pairs = BTreeSet::new();
    let add = |groups: Vec<Vec<usize>>, pairs: &mut BTreeSet<(usize, usize)>| {
        for group in groups {
            for (offset, &left) in group.iter().enumerate() {
                for &right in &group[offset + 1..] {
                    pairs.insert((left.min(right), left.max(right)));
                }
            }
        }
    };
    add(
        exact
            .into_values()
            .chain(ordered.into_values())
            .chain(loose.into_values())
            .collect(),
        &mut pairs,
    );
    // Ubiquitous tokens are not useful blocking evidence.
    add(
        shared
            .into_values()
            .filter(|group| group.len() <= 100)
            .collect(),
        &mut pairs,
    );
    pairs
}

impl ColumnValidator for ValueVariants {
    fn name(&self) -> SharedString {
        VALUE_VARIANTS_NAME.into()
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: &[SharedString],
    ) -> Vec<(usize, Severity, SharedString)> {
        let Ok(mut candidates) = self.candidates.write() else {
            return Vec::new();
        };
        candidates.retain(|(name, _), _| name != column.name);
        if !column.settings.variant_review
            || !ColumnType::from_declared(column.data_type).is_prose()
        {
            return Vec::new();
        }

        let mut grouped: BTreeMap<SharedString, Vec<usize>> = BTreeMap::new();
        for (row, value) in values.iter().enumerate() {
            let displayed = value.trim();
            if !displayed.is_empty() {
                grouped.entry(displayed.into()).or_default().push(row);
            }
        }
        let distinct: Vec<Value> = grouped
            .into_iter()
            .map(|(displayed, rows)| Value::new(displayed, rows))
            .collect();
        let mut messages: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();

        for (left, right) in candidate_pairs(&distinct) {
            let (left, right) = (&distinct[left], &distinct[right]);
            let Some(why) = reason(left, right) else {
                continue;
            };
            for (value, alternative) in [(left, right), (right, left)] {
                for &row in &value.rows {
                    candidates
                        .entry((column.name.to_string(), row))
                        .or_default()
                        .push(alternative.displayed.clone());
                    messages.entry(row).or_default().insert(format!(
                        "“{}” may be a variant of “{}”: {why}",
                        value.displayed, alternative.displayed
                    ));
                }
            }
        }

        messages
            .into_iter()
            .map(|(row, messages)| {
                (
                    row,
                    Severity::Note,
                    messages.into_iter().collect::<Vec<_>>().join("; ").into(),
                )
            })
            .collect()
    }
}

pub fn variant_fixes(location: &Location, text: &str, cx: &App) -> Vec<Fix> {
    let (Some(row), Some(column), Some(variants)) = (
        location.row,
        location.column.as_deref(),
        cx.try_global::<ValueVariants>(),
    ) else {
        return Vec::new();
    };
    let Ok(candidates) = variants.candidates.read() else {
        return Vec::new();
    };
    candidates
        .get(&(column.to_string(), row))
        .into_iter()
        .flatten()
        .filter(|candidate| candidate.as_ref() != text)
        .map(|candidate| Fix {
            label: format!("Use “{candidate}”").into(),
            replacement: candidate.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use diagnostics::{ColumnInfo, ColumnValidator, DATASET_MAIN, Location, Severity};
    use gpui::{SharedString, TestAppContext};
    use settings::columns::ColumnSettings;

    use crate::variants::{Value, candidate_pairs};
    use crate::{ValueVariants, variant_fixes};

    fn findings(values: &[&str]) -> Vec<(usize, Severity, SharedString)> {
        let values: Vec<SharedString> = values.iter().map(|value| (*value).into()).collect();
        ValueVariants::default().validate(
            &ColumnInfo {
                name: "Creator",
                data_type: "Text",
                settings: &ColumnSettings {
                    variant_review: true,
                    ..Default::default()
                },
            },
            &values,
        )
    }

    #[test]
    fn review_is_opt_in() {
        let values = ["Agnès Varda", "Varda, Agnès"].map(SharedString::from);
        assert!(
            ValueVariants::default()
                .validate(
                    &ColumnInfo {
                        name: "Creator",
                        data_type: "Text",
                        settings: &ColumnSettings::default(),
                    },
                    &values,
                )
                .is_empty()
        );
    }

    #[test]
    fn finds_order_diacritic_and_close_spelling_variants() {
        let found = findings(&[
            "Agnès Varda",
            "Varda, Agnès",
            "Agnes Varda",
            "Agnès Varad",
            "Werner Herzog",
        ]);
        assert!(found.iter().any(|item| item.2.contains("different order")));
        assert!(
            found
                .iter()
                .any(|item| item.2.contains("diacritics differ"))
        );
        assert!(found.iter().any(|item| item.2.contains("close spelling")));
        assert!(
            found
                .iter()
                .all(|item| !item.2.contains("Werner") && !item.2.contains("Herzog"))
        );
    }

    #[test]
    fn repeated_identical_values_are_not_variants() {
        assert!(findings(&["Agnès Varda", "Agnès Varda"]).is_empty());
    }

    #[test]
    fn blocking_bounds_a_two_thousand_value_column() {
        let values: Vec<Value> = (0..2_000)
            .map(|ix| {
                Value::new(
                    format!("group{} value{}", ix / 100, ix % 100).into(),
                    vec![ix],
                )
            })
            .collect();
        assert!(
            candidate_pairs(&values).len() <= 120_000,
            "blocking must not fall back to all 1,999,000 pairs"
        );
    }

    #[gpui::test]
    fn a_fix_uses_an_observed_form_without_changing_other_rows(cx: &mut TestAppContext) {
        let variants = ValueVariants::default();
        let settings = ColumnSettings {
            variant_review: true,
            ..Default::default()
        };
        variants.validate(
            &ColumnInfo {
                name: "Creator",
                data_type: "Text",
                settings: &settings,
            },
            &["Agnès Varda".into(), "Varda, Agnès".into()],
        );
        cx.update(|cx| {
            cx.set_global(variants);
            let fixes = variant_fixes(
                &Location {
                    dataset: DATASET_MAIN.into(),
                    row: Some(0),
                    row_id: None,
                    column: Some("Creator".into()),
                },
                "Agnès Varda",
                cx,
            );
            assert_eq!(fixes.len(), 1);
            assert_eq!(fixes[0].replacement, "Varda, Agnès");
        });
    }
}

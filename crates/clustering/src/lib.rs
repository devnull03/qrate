//! Advisory value comparisons, independently registered from spelling.
pub mod core;

use diagnostics::{
    ColumnFinding, ColumnInfo, ColumnValidator, ColumnValues, DiagnosticGroup, DiagnosticHooks,
    Fix, GroupFix, GroupMember, Location, Severity,
};
use gpui::{App, Global, SharedString};
use settings::columns::ColumnType;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub const VALUE_VARIANTS_NAME: &str = "value variants";
const SLOW_CLUSTERING: Duration = Duration::from_millis(250);
type Candidates = BTreeMap<(String, usize), (SharedString, Vec<SharedString>)>;

#[derive(Clone, Default)]
pub struct ValueVariants {
    candidates: Arc<RwLock<Candidates>>,
}
impl Global for ValueVariants {}

impl ColumnValidator for ValueVariants {
    fn name(&self) -> SharedString {
        VALUE_VARIANTS_NAME.into()
    }

    fn begin_run(&self) {
        if let Ok(mut candidates) = self.candidates.write() {
            candidates.clear();
        }
    }

    fn validate(&self, column: &ColumnInfo, values: ColumnValues<'_>) -> Vec<ColumnFinding> {
        let started = Instant::now();
        let Ok(mut candidates) = self.candidates.write() else {
            return Vec::new();
        };
        candidates.retain(|(name, _), _| name != column.name);
        if !column.settings.variant_review
            || !ColumnType::from_declared(column.data_type).is_prose()
        {
            return Vec::new();
        }
        let mut findings = Vec::new();
        let mut logical_values = 0;
        let logical = values
            .iter()
            .flat_map(|cell| cell.parts().map(move |value| (cell.row, value)))
            .inspect(|_| logical_values += 1);
        let comparison = core::compare_indexed(logical);
        let pair_count = comparison.pairs.len();
        for pair in comparison.pairs {
            let distinct = ordered_pair(&pair.left.displayed, &pair.right.displayed);
            if column.settings.distinct_variants.contains(&distinct) {
                continue;
            }
            let group = DiagnosticGroup {
                key: format!("{:?}", (column.name, &pair.key)).into(),
                summary: format!(
                    "“{}” and “{}” may be variants: {}",
                    pair.left.displayed, pair.right.displayed, pair.reason
                )
                .into(),
            };
            for (value, alternative) in [(&pair.left, &pair.right), (&pair.right, &pair.left)] {
                for &row in &value.rows {
                    let Some(replacement) =
                        values.replace_part(row, &value.displayed, &alternative.displayed)
                    else {
                        continue;
                    };
                    candidates
                        .entry((column.name.to_owned(), row))
                        .or_insert_with(|| (values.raw()[row].clone(), Vec::new()))
                        .1
                        .push(replacement);
                    findings.push(ColumnFinding {
                        row: Some(row),
                        severity: Severity::Warning,
                        message: value.displayed.clone().into(),
                        group: Some(group.clone()),
                    });
                }
            }
        }
        let elapsed = started.elapsed();
        log::debug!(
            "clustered column {:?}: {} rows, {logical_values} logical values, {} distinct values, {} candidate pairs, {pair_count} matches, {} findings in {elapsed:?}",
            column.name,
            values.raw().len(),
            comparison.distinct_values,
            comparison.candidate_pairs,
            findings.len(),
        );
        if elapsed >= SLOW_CLUSTERING {
            log::warn!(
                "slow value clustering in column {:?}: {} rows, {logical_values} logical values, {} distinct values, {} candidate pairs, {pair_count} matches, {} findings in {elapsed:?}",
                column.name,
                values.raw().len(),
                comparison.distinct_values,
                comparison.candidate_pairs,
                findings.len(),
            );
        }
        findings
    }
}

fn ordered_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_owned(), right.to_owned())
    } else {
        (right.to_owned(), left.to_owned())
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
    if location.dataset != diagnostics::DATASET_MAIN {
        return Vec::new();
    }
    let Ok(candidates) = variants.candidates.read() else {
        return Vec::new();
    };
    let Some((expected, alternatives)) = candidates.get(&(column.to_owned(), row)) else {
        return Vec::new();
    };
    if expected.as_ref() != text {
        return Vec::new();
    }
    alternatives
        .iter()
        .filter(|candidate| candidate.as_ref() != text)
        .map(|candidate| Fix {
            label: format!("Use “{candidate}”").into(),
            replacement: candidate.clone(),
        })
        .collect()
}

pub fn variant_group_fixes(members: &[GroupMember], cx: &App) -> Vec<GroupFix> {
    let mut forms: Vec<_> = members
        .iter()
        .map(|member| member.message.to_string())
        .collect();
    forms.sort();
    forms.dedup();
    let [left, right] = forms.as_slice() else {
        return Vec::new();
    };
    let delimiter = settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx);
    let mut fixes = [left, right]
        .into_iter()
        .filter_map(|target| {
            let replacements = members
                .iter()
                .filter(|member| member.message.as_ref() != target.as_str())
                .map(|member| {
                    let raw = [member.text.clone()];
                    ColumnValues::new(&raw, &delimiter)
                        .replace_part(0, &member.message, target)
                        .map(|replacement| (member.location.clone(), replacement))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(GroupFix::replacements(
                format!("Change all to “{target}”"),
                replacements,
            ))
        })
        .collect::<Vec<_>>();
    let Some(column) = members
        .first()
        .and_then(|member| member.location.column.clone())
    else {
        return fixes;
    };
    let distinct = ordered_pair(left, right);
    fixes.push(GroupFix::action(
        "These are distinct — ignore this pair",
        move |cx| {
            settings::columns::update(
                &column,
                |settings| {
                    settings.distinct_variants.insert(distinct.clone());
                },
                cx,
            );
            if let Some(hooks) = cx.try_global::<DiagnosticHooks>().copied() {
                (hooks.revalidate)(cx);
            }
        },
    ));
    fixes
}

#[cfg(test)]
mod tests {
    use crate::{ValueVariants, ordered_pair, variant_fixes, variant_group_fixes};
    use diagnostics::{
        ColumnInfo, ColumnValidator, ColumnValues, DATASET_MAIN, GroupMember, Location, Severity,
    };
    use gpui::TestAppContext;
    use settings::columns::ColumnSettings;

    #[gpui::test]
    fn opt_in_grouping_and_stale_fixes(cx: &mut TestAppContext) {
        let variants = ValueVariants::default();
        let values = [
            "Agnès Varda".into(),
            "Varda, Agnès".into(),
            "Agnès Varda".into(),
        ];
        let mut settings = ColumnSettings::default();
        assert!(
            variants
                .validate(
                    &ColumnInfo {
                        name: "Creator",
                        data_type: "Text",
                        settings: &settings,
                    },
                    ColumnValues::new(&values, ""),
                )
                .is_empty()
        );
        settings.variant_review = true;
        let found = variants.validate(
            &ColumnInfo {
                name: "Creator",
                data_type: "Text",
                settings: &settings,
            },
            ColumnValues::new(&values, ""),
        );
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|f| f.group == found[0].group));
        assert_eq!(
            found
                .iter()
                .map(|finding| finding.message.as_ref())
                .collect::<Vec<_>>(),
            ["Agnès Varda", "Agnès Varda", "Varda, Agnès"]
        );
        cx.update(|cx| {
            cx.set_global(variants);
            let location = Location::cell(DATASET_MAIN, 0, None, "Creator");
            assert_eq!(
                variant_fixes(&location, "Agnès Varda", cx)[0].replacement,
                "Varda, Agnès"
            );
            assert!(variant_fixes(&location, "changed", cx).is_empty());
            cx.global::<ValueVariants>().begin_run();
            assert!(variant_fixes(&location, "Agnès Varda", cx).is_empty());
        });
    }

    #[test]
    fn subdelimited_cells_compare_each_value_and_keep_whole_cell_fixes() {
        let variants = ValueVariants::default();
        let settings = ColumnSettings {
            variant_review: true,
            ..Default::default()
        };
        let values = [
            "Busson, Carl W.|Dhillon, Baltej Singh".into(),
            "Carl W. Busson|Baltej Singh Dhillon".into(),
        ];
        let found = variants.validate(
            &ColumnInfo {
                name: "Creator",
                data_type: "Text",
                settings: &settings,
            },
            ColumnValues::new(&values, "|"),
        );
        assert_eq!(found.len(), 4);
        assert!(found.iter().all(|finding| {
            finding.severity == Severity::Warning && !finding.message.contains('|')
        }));
        let candidates = variants.candidates.read().unwrap();
        let (expected, replacements) = &candidates[&("Creator".to_owned(), 0)];
        assert_eq!(expected, "Busson, Carl W.|Dhillon, Baltej Singh");
        assert!(replacements.contains(&"Carl W. Busson|Dhillon, Baltej Singh".into()));
        assert!(replacements.contains(&"Busson, Carl W.|Baltej Singh Dhillon".into()));
    }

    #[test]
    fn confirmed_distinct_pairs_are_not_reported() {
        let variants = ValueVariants::default();
        let mut settings = ColumnSettings {
            variant_review: true,
            ..Default::default()
        };
        settings
            .distinct_variants
            .insert(ordered_pair("Agnès Varda", "Agnes Varda"));
        let values = ["Agnès Varda".into(), "Agnes Varda".into()];
        assert!(
            variants
                .validate(
                    &ColumnInfo {
                        name: "Creator",
                        data_type: "Text",
                        settings: &settings,
                    },
                    ColumnValues::new(&values, ""),
                )
                .is_empty()
        );
    }

    #[gpui::test]
    fn group_resolver_offers_both_forms_and_distinct(cx: &mut TestAppContext) {
        #[derive(Default)]
        struct Revalidations(usize);
        impl gpui::Global for Revalidations {}
        #[derive(Default)]
        struct Applied(Vec<(Location, gpui::SharedString)>);
        impl gpui::Global for Applied {}

        cx.update(|cx| {
            let mut settings = settings::AppSettings::default();
            settings.values.insert(
                settings::FILTER_SUBDELIMITER_KEY.into(),
                settings::Val::Text("|".into()),
            );
            cx.set_global(settings);
            cx.set_global(Revalidations::default());
            cx.set_global(Applied::default());
            cx.set_global(settings::project::CurrentProject {
                file: std::env::temp_dir().join("qrate-clustering-ignore.qrate"),
                data: settings::project::ProjectData {
                    name: "Test".into(),
                    columns: Vec::new(),
                    headers: vec!["Creator".into()],
                    rows: Vec::new(),
                    row_ids: Vec::new(),
                    values: Default::default(),
                },
            });
            cx.set_global(diagnostics::DiagnosticHooks {
                reveal: |_, _| {},
                text_at: |_, _| None,
                set_text: |_, _, _| {},
                set_texts: |replacements, cx| {
                    use gpui::BorrowAppContext as _;
                    cx.update_global::<Applied, _>(|applied, _| applied.0 = replacements);
                },
                revalidate: |cx| {
                    use gpui::BorrowAppContext as _;
                    cx.update_global::<Revalidations, _>(|count, _| count.0 += 1);
                },
            });
            let members = [
                GroupMember {
                    location: Location::cell(DATASET_MAIN, 0, None, "Creator"),
                    text: "Agnès Varda|Director".into(),
                    message: "Agnès Varda".into(),
                },
                GroupMember {
                    location: Location::cell(DATASET_MAIN, 1, None, "Creator"),
                    text: "Agnes Varda|Producer".into(),
                    message: "Agnes Varda".into(),
                },
            ];
            let fixes = variant_group_fixes(&members, cx);
            assert_eq!(
                fixes
                    .iter()
                    .map(|fix| fix.label.as_ref())
                    .collect::<Vec<_>>(),
                [
                    "Change all to “Agnes Varda”",
                    "Change all to “Agnès Varda”",
                    "These are distinct — ignore this pair",
                ]
            );
            fixes[0].apply(cx);
            assert_eq!(cx.global::<Applied>().0.len(), 1);
            assert_eq!(
                cx.global::<Applied>().0[0].1,
                "Agnes Varda|Director",
                "the other logical value remains unchanged"
            );
            fixes[2].apply(cx);
            assert!(
                settings::columns::get("Creator", cx)
                    .distinct_variants
                    .contains(&ordered_pair("Agnès Varda", "Agnes Varda"))
            );
            assert_eq!(cx.global::<Revalidations>().0, 1);
        });
    }
}

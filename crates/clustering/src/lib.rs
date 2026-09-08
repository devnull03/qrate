//! Advisory value comparisons, independently registered from spelling.
pub mod core;

use diagnostics::{
    ColumnFinding, ColumnInfo, ColumnValidator, DiagnosticGroup, Fix, Location, Severity,
};
use gpui::{App, Global, SharedString};
use settings::columns::ColumnType;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

pub const VALUE_VARIANTS_NAME: &str = "value variants";
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

    fn validate(&self, column: &ColumnInfo, values: &[SharedString]) -> Vec<ColumnFinding> {
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
        for pair in core::compare(values.iter().map(|value| value.as_ref())) {
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
                    candidates
                        .entry((column.name.to_owned(), row))
                        .or_insert_with(|| (value.displayed.clone().into(), Vec::new()))
                        .1
                        .push(alternative.displayed.clone().into());
                    findings.push(ColumnFinding {
                        row: Some(row),
                        severity: Severity::Note,
                        message: group.summary.clone(),
                        group: Some(group.clone()),
                    });
                }
            }
        }
        findings
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

#[cfg(test)]
mod tests {
    use crate::{ValueVariants, variant_fixes};
    use diagnostics::{ColumnInfo, ColumnValidator, DATASET_MAIN, Location};
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
                        settings: &settings
                    },
                    &values
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
            &values,
        );
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|f| f.group == found[0].group));
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
}

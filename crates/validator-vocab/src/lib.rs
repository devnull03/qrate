//! Controlled-vocabulary validation: flags any cell whose value is not on its column's allowed
//! list.
//!
//! Its own crate because that is the unit a validator ships as — this one needs nothing but a
//! `HashSet`, but the next needs a dictionary, and the boundary has to be the same either way.
//! Nothing here knows about datasets, projects, or the table; the registry addresses whatever
//! [`Vocab::validate`] returns.

use std::collections::HashSet;

use diagnostics::{ColumnInfo, ColumnValidator, Severity};
use gpui::SharedString;

pub struct Vocab;

impl ColumnValidator for Vocab {
    fn name(&self) -> SharedString {
        "vocabulary".into()
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: &[SharedString],
    ) -> Vec<(usize, Severity, SharedString)> {
        // An empty list is how a column stays unrestricted, so this is the opt-out for every
        // column the user has not deliberately restricted.
        if column.settings.allowed_values.is_empty() {
            return Vec::new();
        }
        let allowed: HashSet<&str> = column
            .settings
            .allowed_values
            .iter()
            .map(String::as_str)
            .collect();

        values
            .iter()
            .enumerate()
            // A blank cell is missing data, not a vocabulary breach — that is a different check.
            .filter(|(_, v)| !v.trim().is_empty() && !allowed.contains(v.trim()))
            .map(|(row, v)| {
                (
                    row,
                    Severity::Error,
                    format!("\"{}\" is not an allowed value for {}", v, column.name).into(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::Vocab;
    use diagnostics::{ColumnInfo, ColumnValidator, Severity};
    use gpui::SharedString;
    use settings::columns::ColumnSettings;

    fn check(allowed: &[&str], values: &[&str]) -> Vec<(usize, Severity, SharedString)> {
        let settings = ColumnSettings {
            filter_enabled: false,
            allowed_values: allowed.iter().map(|s| s.to_string()).collect(),
        };
        let column = ColumnInfo {
            name: "Format",
            data_type: "Text",
            settings: &settings,
        };
        let values: Vec<SharedString> = values.iter().map(|v| SharedString::from(*v)).collect();
        Vocab.validate(&column, &values)
    }

    #[test]
    fn an_unrestricted_column_is_never_flagged() {
        assert!(check(&[], &["anything", "at", "all"]).is_empty());
    }

    #[test]
    fn only_values_off_the_list_are_flagged() {
        let found = check(&["Film", "Video"], &["Film", "Fllm", "Video", ""]);
        assert_eq!(found.len(), 1);
        let (row, severity, message) = &found[0];
        assert_eq!(*row, 1, "the row index is the position in the column");
        assert_eq!(*severity, Severity::Error);
        assert!(message.contains("Fllm") && message.contains("Format"));
    }

    /// Blanks are missing data, and surrounding whitespace is a formatting artefact of the import
    /// rather than a different term — neither is a vocabulary error.
    #[test]
    fn blank_and_padded_values_pass() {
        assert!(check(&["Film"], &["", "   ", " Film ", "Film"]).is_empty());
    }
}

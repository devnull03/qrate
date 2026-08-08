//! Checks a `Date` column against EDTF, and offers the obvious rewrites when a value misses.
//!
//! EDTF (ISO 8601-2) is what Islandora and MODS want in a date field, and it is more forgiving
//! than it looks: `1987`, `1987-05`, `1987?` (uncertain), `1987~` (approximate), `1987/1989`
//! (interval), and `198X` (unspecified digit) are all valid. So a value this rejects is usually
//! prose — `circa 1987`, `May 1987`, `1987-5-3` — which is exactly what an ingest would choke on.

use diagnostics::{ColumnInfo, ColumnValidator, Fix, FixProviders, Location, Severity};
use gpui::{App, SharedString};
use settings::columns::ColumnType;

/// What the Problems panel shows, and the key this validator's output is replaced by.
pub const SOURCE: &str = "date";

pub struct DateCheck;

/// Whether `value` is something EDTF accepts. Level 1 rather than level 0 because uncertainty and
/// approximation are the whole reason an archive wants EDTF over a plain date.
fn valid(value: &str) -> bool {
    edtf::level_1::Edtf::parse(value).is_ok()
}

impl ColumnValidator for DateCheck {
    fn name(&self) -> SharedString {
        SOURCE.into()
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: &[SharedString],
    ) -> Vec<(usize, Severity, SharedString)> {
        if ColumnType::from_declared(column.data_type) != ColumnType::Date {
            return Vec::new();
        }
        values
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.trim().is_empty())
            .filter(|(_, value)| !valid(value.trim()))
            .map(|(row, value)| {
                (
                    row,
                    Severity::Error,
                    format!("“{}” is not an EDTF date", value.trim()).into(),
                )
            })
            .collect()
    }
}

/// Rewrites worth offering for a value EDTF rejected, each kept only if it actually parses.
///
/// Deliberately narrow: these are transcription habits (a slash instead of a hyphen, an unpadded
/// month, the word "circa"), not attempts to guess what a date means. Anything cleverer would be
/// proposing a fact about the collection, which is the archivist's call and not a menu's.
fn rewrites(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let lower = trimmed.to_lowercase();

    // "circa 1987" / "c. 1987" / "ca. 1987" → the EDTF approximate marker.
    let approximate = ["circa ", "ca. ", "ca ", "c. "]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))
        .map(|rest| format!("{}~", rest.trim()));

    // 1987/05/03 and 1987.05.03 are the same date written the way a spreadsheet offered.
    let hyphenated = trimmed.replace(['/', '.'], "-");

    // 1987-5-3 → 1987-05-03. EDTF wants two digits, and a missing zero is a typing habit.
    let padded = {
        let parts: Vec<&str> = hyphenated.split('-').collect();
        (parts.len() == 3 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())))
            .then(|| format!("{}-{:0>2}-{:0>2}", parts[0], parts[1], parts[2]))
    };

    let mut offered: Vec<String> = [approximate, Some(hyphenated), padded]
        .into_iter()
        .flatten()
        .filter(|candidate| candidate != trimmed && valid(candidate))
        .collect();
    offered.dedup();
    offered
}

fn offer(_: &Location, text: &str, _: &App) -> Vec<Fix> {
    if valid(text.trim()) {
        return Vec::new();
    }
    rewrites(text)
        .into_iter()
        .map(|fixed| Fix {
            label: format!("Use {fixed}").into(),
            replacement: fixed.into(),
        })
        .collect()
}

pub fn init(cx: &mut App) {
    diagnostics::Validators::register(Box::new(DateCheck), cx);
    FixProviders::register(SOURCE, offer, cx);
}

#[cfg(test)]
mod tests {
    use crate::dates::{rewrites, valid};

    /// The shapes an archive actually needs EDTF for — uncertainty and imprecision — must pass,
    /// or the validator is just a stricter date field nobody wants.
    #[test]
    fn edtf_accepts_the_imprecision_an_archive_needs() {
        for value in [
            "1987",
            "1987-05",
            "1987-05-03",
            "1987?",
            "1987~",
            "1987-05?",
            "1987/1989",
            "198X",
        ] {
            assert!(valid(value), "{value} is valid EDTF");
        }
    }

    #[test]
    fn prose_and_spreadsheet_habits_are_rejected() {
        for value in ["circa 1987", "May 1987", "1987-5-3", "sometime in the 80s"] {
            assert!(!valid(value), "{value} is not EDTF");
        }
    }

    /// Each offer has to parse — suggesting a fix that is still invalid is worse than none.
    #[test]
    fn every_rewrite_offered_is_itself_valid() {
        for value in ["circa 1987", "1987/05/03", "1987-5-3", "1987.05.03"] {
            let offered = rewrites(value);
            assert!(!offered.is_empty(), "{value} gets an offer");
            for candidate in offered {
                assert!(valid(&candidate), "{value} offered invalid {candidate}");
            }
        }
    }

    #[test]
    fn a_value_nothing_sensible_can_be_done_with_gets_no_offer() {
        assert!(rewrites("sometime in the 80s").is_empty());
        assert!(rewrites("May 1987").is_empty());
    }

    /// `1987/1989` is an EDTF interval already — the slash rewrite must not "fix" it into
    /// something else, and a valid value must never be offered a rewrite at all.
    #[test]
    fn a_valid_value_is_left_alone() {
        assert!(valid("1987/1989"));
        assert!(rewrites("1987").is_empty());
    }
}

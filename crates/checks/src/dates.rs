//! Checks a `Date` column against EDTF, and offers the obvious rewrites when a value misses.
//!
//! EDTF (ISO 8601-2) is what Islandora and MODS want in a date field, and it is more forgiving
//! than it looks: `1987`, `1987-05`, `1987?` (uncertain), `1987~` (approximate), `1987/1989`
//! (interval), `198X` (unspecified digit), and `[1667,1668]` (one of these) are all valid. So a
//! value this rejects is usually prose — `circa 1987`, `May 1987`, `1987-5-3` — which is exactly
//! what an ingest would choke on.

use std::sync::atomic::{AtomicU8, Ordering};

use diagnostics::{
    ColumnFinding, ColumnInfo, ColumnValidator, ColumnValues, DiagnosticGroup, Fix, FixProviders,
    Location, Severity,
};
use gpui::{App, SharedString};
use settings::columns::ColumnType;

/// What the Problems panel shows, and the key this validator's output is replaced by.
pub const SOURCE: &str = "date";

/// Settings key (either scope) for what a `Date` column accepts: EDTF (unset), `iso`, `lenient`.
pub const DATE_FORMAT_KEY: &str = "date_format";

/// What Settings offers for [`DATE_FORMAT_KEY`].
pub const DATE_FORMATS: &[(&str, &str)] = &[
    ("", "EDTF (default)"),
    ("iso", "ISO 8601 only"),
    ("lenient", "Lenient"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Mode {
    Edtf,
    Iso,
    Lenient,
}

/// The mode in force, mirrored out of the settings stores because validation runs off the UI
/// thread with no `App` to read them from.
static MODE: AtomicU8 = AtomicU8::new(Mode::Edtf as u8);

fn mode() -> Mode {
    match MODE.load(Ordering::SeqCst) {
        1 => Mode::Iso,
        2 => Mode::Lenient,
        _ => Mode::Edtf,
    }
}

/// Copy [`DATE_FORMAT_KEY`] into the validator. Call before revalidating after a change to it.
pub fn sync_mode(cx: &mut App) {
    if !cx.has_global::<settings::AppSettings>() {
        return;
    }
    let mode = match settings::effective_text(DATE_FORMAT_KEY, cx).as_ref() {
        "iso" => Mode::Iso,
        "lenient" => Mode::Lenient,
        _ => Mode::Edtf,
    };
    MODE.store(mode as u8, Ordering::SeqCst);
}

pub struct DateCheck;

fn valid(value: &str, mode: Mode) -> bool {
    match mode {
        Mode::Edtf => edtf(value),
        Mode::Iso => match value.split_once('/') {
            Some((start, end)) => iso(start) && iso(end),
            None => iso(value),
        },
        Mode::Lenient => edtf(value) || approximate(value),
    }
}

/// Whether `value` is something EDTF accepts. Level 1 rather than level 0 because uncertainty and
/// approximation are the whole reason an archive wants EDTF over a plain date, plus the level 2
/// sets below — `edtf`'s own `level_2` parses nothing but scientific years.
fn edtf(value: &str) -> bool {
    match members(value) {
        Some(members) => members.iter().all(|member| valid_member(member)),
        None => edtf::level_1::Edtf::parse(value).is_ok(),
    }
}

/// A calendar date as `YYYY`, `YYYY-MM`, or `YYYY-MM-DD`, with no EDTF qualifiers.
fn iso(value: &str) -> bool {
    let shaped = matches!(value.len(), 4 | 7 | 10)
        && value.char_indices().all(|(at, c)| match at {
            4 | 7 => c == '-',
            _ => c.is_ascii_digit(),
        });
    shaped && edtf::level_1::Edtf::parse(value).is_ok()
}

/// The ways a catalogue writes "about this date" that EDTF has its own syntax for: `circa 1920`,
/// `ca. 1920`, `c. 1920`, a decade as `1920s`, and a bracketed guess such as `[1920?]`.
fn approximate(value: &str) -> bool {
    let lower = value.trim().to_lowercase();
    if let Some(inner) = lower.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return edtf(inner.trim()) || approximate(inner);
    }
    if let Some(rest) = ["circa ", "ca. ", "ca.", "ca ", "c. ", "c."]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))
    {
        return edtf(rest.trim());
    }
    let decade = lower
        .strip_suffix("'s")
        .or_else(|| lower.strip_suffix('s'))
        .unwrap_or_default();
    decade.len() == 4 && decade.ends_with('0') && decade.chars().all(|c| c.is_ascii_digit())
}

/// What is inside an EDTF set (`[1667,1668]`, one of these) or list (`{1960,1961-12}`, all of
/// these), or `None` when the value is neither. Members are trimmed: the spec writes them without
/// spaces, but a space after a comma is a typing habit and not a different date.
fn members(value: &str) -> Option<Vec<&str>> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .or_else(|| value.strip_prefix('{').and_then(|v| v.strip_suffix('}')))?;
    Some(inner.split(',').map(str::trim).collect())
}

/// One member of a set: a date, or a range between two of them — `1670..1672`, and the open-ended
/// `..1672` and `1670..`. A range inside a set is written with `..`, so the slash interval and
/// anything else level 1 accepts as a whole value is rejected here.
fn valid_member(member: &str) -> bool {
    fn date(value: &str) -> bool {
        matches!(
            edtf::level_1::Edtf::parse(value),
            Ok(edtf::level_1::Edtf::Date(_) | edtf::level_1::Edtf::YYear(_))
        )
    }
    match member.split_once("..") {
        Some(("", end)) => date(end),
        Some((start, "")) => date(start),
        Some((start, end)) => date(start) && date(end),
        None => date(member),
    }
}

impl ColumnValidator for DateCheck {
    fn name(&self) -> SharedString {
        SOURCE.into()
    }

    fn revision(&self) -> u64 {
        mode() as u64
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: ColumnValues<'_>,
    ) -> Vec<diagnostics::ColumnFinding> {
        if ColumnType::from_declared(column.data_type) != ColumnType::Date {
            return Vec::new();
        }
        let mode = mode();
        let expected = match mode {
            Mode::Edtf => "an EDTF date",
            Mode::Iso => "an ISO 8601 date",
            Mode::Lenient => "a date qrate recognises",
        };
        values
            .iter()
            .flat_map(|cell| {
                cell.parts()
                    .filter(move |value| !valid(value, mode))
                    .map(move |value| {
                        let message: SharedString = format!("“{value}” is not {expected}").into();
                        ColumnFinding {
                            row: Some(cell.row),
                            severity: Severity::Error,
                            message: message.clone(),
                            group: Some(DiagnosticGroup {
                                key: value.to_owned().into(),
                                summary: message,
                                subject: Some(value.to_owned().into()),
                            }),
                        }
                    })
            })
            .collect()
    }
}

/// Rewrites worth offering for a value EDTF rejected, each kept only if it actually parses.
///
/// Deliberately narrow: these are transcription habits (a slash instead of a hyphen, an unpadded
/// month, the word "circa"), not attempts to guess what a date means. Anything cleverer would be
/// proposing a fact about the collection, which is the archivist's call and not a menu's.
fn rewrites(value: &str, mode: Mode) -> Vec<String> {
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
        .filter(|candidate| candidate != trimmed && valid(candidate, mode))
        .collect();
    offered.dedup();
    offered
}

fn offer(_: &Location, text: &str, subject: Option<&str>, _: &App) -> Vec<Fix> {
    let value = subject.unwrap_or(text);
    let mode = mode();
    if valid(value.trim(), mode) {
        return Vec::new();
    }
    rewrites(value, mode)
        .into_iter()
        .map(|fixed| Fix {
            label: format!("Use {fixed}").into(),
            replacement: text.replacen(value, &fixed, 1).into(),
        })
        .collect()
}

pub fn init(cx: &mut App) {
    sync_mode(cx);
    cx.observe_global::<settings::AppSettings>(sync_mode)
        .detach();
    cx.observe_global::<settings::project::CurrentProject>(sync_mode)
        .detach();
    diagnostics::Validators::register(Box::new(DateCheck), cx);
    FixProviders::register(SOURCE, offer, cx);
}

#[cfg(test)]
mod tests {
    use crate::dates::{DateCheck, Mode, approximate, offer, rewrites, valid};
    use diagnostics::{ColumnInfo, ColumnValidator, ColumnValues, DATASET_MAIN, Location};
    use gpui::SharedString;
    use settings::columns::ColumnSettings;

    #[gpui::test]
    fn the_same_bad_date_groups_and_fixes_only_its_own_value(cx: &mut gpui::TestAppContext) {
        let settings = ColumnSettings::default();
        let column = ColumnInfo {
            name: "Date",
            data_type: "Date",
            settings: &settings,
        };
        let raw: Vec<SharedString> = vec!["circa 1987".into(), "1990 | circa 1987".into()];
        let found = DateCheck.validate(&column, ColumnValues::new(&raw, "|"));
        let groups: Vec<_> = found.iter().map(|f| f.group.clone().unwrap()).collect();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], groups[1]);
        assert_eq!(groups[0].subject.as_deref(), Some("circa 1987"));

        let location = Location {
            dataset: DATASET_MAIN.into(),
            row: Some(1),
            row_id: None,
            column: Some("Date".into()),
        };
        let fixes = cx.update(|cx| offer(&location, &raw[1], Some("circa 1987"), cx));
        assert_eq!(fixes[0].replacement.as_ref(), "1990 | 1987~");
    }

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
            assert!(valid(value, Mode::Edtf), "{value} is valid EDTF");
        }
    }

    /// Level 2 sets and lists — a slide catalogued as "1857 or 1858" is written `[1857,1858]`, and
    /// `edtf`'s own level 2 parses none of this, so it is peeled apart here.
    #[test]
    fn edtf_accepts_sets_and_lists() {
        for value in [
            "[1857,1858]",
            "[1857, 1858]",
            "{1960,1961-12}",
            "[1667]",
            "[1670..1672]",
            "[..1672]",
            "[1670..]",
            "[1667,1670..1672,198X]",
        ] {
            assert!(valid(value, Mode::Edtf), "{value} is valid EDTF");
        }
    }

    /// A set is only as valid as its members, and half-written brackets are a transcription slip
    /// worth reporting rather than waving through.
    #[test]
    fn a_set_with_a_bad_member_or_a_missing_bracket_is_rejected() {
        for value in [
            "[]",
            "[1857,]",
            "[May 1987]",
            "[1857",
            "1857]",
            "[1857,1858}",
            // A range inside a set is written `..`; the slash interval belongs at the top level.
            "[1667/1668]",
        ] {
            assert!(!valid(value, Mode::Edtf), "{value} is not EDTF");
        }
    }

    #[test]
    fn prose_and_spreadsheet_habits_are_rejected() {
        for value in ["circa 1987", "May 1987", "1987-5-3", "sometime in the 80s"] {
            assert!(!valid(value, Mode::Edtf), "{value} is not EDTF");
        }
    }

    /// Each offer has to parse — suggesting a fix that is still invalid is worse than none.
    #[test]
    fn every_rewrite_offered_is_itself_valid() {
        for value in ["circa 1987", "1987/05/03", "1987-5-3", "1987.05.03"] {
            let offered = rewrites(value, Mode::Edtf);
            assert!(!offered.is_empty(), "{value} gets an offer");
            for candidate in offered {
                assert!(
                    valid(&candidate, Mode::Edtf),
                    "{value} offered invalid {candidate}"
                );
            }
        }
    }

    #[test]
    fn a_value_nothing_sensible_can_be_done_with_gets_no_offer() {
        assert!(rewrites("sometime in the 80s", Mode::Edtf).is_empty());
        assert!(rewrites("May 1987", Mode::Edtf).is_empty());
    }

    /// `1987/1989` is an EDTF interval already — the slash rewrite must not "fix" it into
    /// something else, and a valid value must never be offered a rewrite at all.
    #[test]
    fn a_valid_value_is_left_alone() {
        assert!(valid("1987/1989", Mode::Edtf));
        assert!(rewrites("1987", Mode::Edtf).is_empty());
    }

    /// ISO 8601 only is plain calendar dates and intervals between them: EDTF's qualifiers, sets
    /// and unspecified digits are all refused, and so is a date the calendar does not have.
    #[test]
    fn iso_mode_accepts_calendar_dates_and_nothing_else() {
        for value in [
            "1987",
            "1987-05",
            "1987-05-03",
            "1987/1989",
            "1987-05/1988-01-31",
        ] {
            assert!(valid(value, Mode::Iso), "{value} is ISO 8601");
        }
        for value in [
            "1987?",
            "1987~",
            "198X",
            "[1857,1858]",
            "1987-13",
            "1987-02-30",
            "87",
            "1987-5-3",
            "circa 1987",
        ] {
            assert!(!valid(value, Mode::Iso), "{value} is not ISO 8601");
        }
    }

    /// An ISO rewrite still has to be ISO: "circa" becoming `1987~` is an EDTF answer.
    #[test]
    fn iso_mode_only_offers_iso_rewrites() {
        assert_eq!(
            rewrites("1987/5/3", Mode::Iso),
            vec!["1987-05-03".to_string()]
        );
        assert!(rewrites("circa 1987", Mode::Iso).is_empty());
    }

    #[test]
    fn lenient_mode_accepts_approximate_forms_as_well_as_edtf() {
        for value in [
            "circa 1920",
            "ca. 1920",
            "ca 1920",
            "c. 1920",
            "c.1920",
            "Circa 1920",
            "1920s",
            "1920's",
            "[1920?]",
            "[ca. 1920]",
            "1920~",
            "1987-05-03",
            "[1857,1858]",
        ] {
            assert!(valid(value, Mode::Lenient), "{value} is accepted leniently");
        }
        for value in [
            "sometime in the 80s",
            "May 1987",
            "1925s",
            "c. May",
            "[1920",
        ] {
            assert!(!valid(value, Mode::Lenient), "{value} is still refused");
        }
        assert!(
            !approximate("1987"),
            "a plain date is EDTF's, not an approximation"
        );
    }
}

//! Spell checking as a [`ColumnValidator`], plus the suggestion and add-a-word actions the
//! table's right-click menu reaches through [`diagnostics::SpellActions`].
//!
//! A leaf crate that only `app` depends on, which is the whole reason `ColumnValidator` is `dyn`:
//! the two half-megabyte English dictionaries embedded below must never enter `table`'s graph.
//!
//! Canadian and American English ship compiled in; every other language is a download away. That
//! is the only difference between one language and the next — no code knows what English is.

pub mod catalogue;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use diagnostics::{
    ColumnInfo, ColumnValidator, ColumnValues, Fix, Location, Misspelling, Severity,
};
use gpui::{App, Global, SharedString};
use settings::columns::ColumnType;
use spellbook::Dictionary;

/// Setting key (either scope) for the spell-check master switch. Absent means on, so the feature
/// works without first visiting Settings — the same convention `COLUMN_FILTERS_ENABLED_KEY` uses.
pub const SPELLCHECK_ENABLED_KEY: &str = "spellcheck_enabled";

/// Setting key holding the dictionary language, a `catalogue` code such as `en-GB`. Absent means [`DEFAULT_LANGUAGE`].
pub const SPELLCHECK_LANGUAGE_KEY: &str = "spellcheck_language";

/// Setting key for skipping capitalized words. Absent means on: a catalogue is mostly people,
/// places, studios, and titles, and no word list will ever hold all of them.
pub const SPELLCHECK_NAMES_KEY: &str = "spellcheck_ignore_capitalized";

/// The language used when nothing is configured. Canadian rather than American because the two
/// SCOWL word lists carry one spelling each, and `catalogue` is not a word en-US accepts.
pub const DEFAULT_LANGUAGE: &str = "en-CA";

static EN_CA_AFF: &str = include_str!("../dictionaries/en-CA.aff");
static EN_CA_DIC: &str = include_str!("../dictionaries/en-CA.dic");
static EN_AFF: &str = include_str!("../dictionaries/en.aff");
static EN_DIC: &str = include_str!("../dictionaries/en.dic");

/// The name the Problems panel shows, and the key this validator's output is replaced by.
const VALIDATOR_NAME: &str = "spell";

/// Capitalization is separate from spelling: known words in the wrong case are quieter and offer a
/// direct replacement instead of "Add to dictionary".
pub const CAPITALIZATION_VALIDATOR_NAME: &str = "capitalization";

/// Shortest token worth checking. Below this the dictionary answers "yes" for almost anything.
const MIN_WORD_LEN: usize = 3;

/// An all-caps token this long or shorter is read as an acronym — `MP4`, `VHS`, `NTSC`, `JPEG` —
/// and skipped. Longer all-caps text is checked normally, because Hunspell already accepts an
/// upper-cased form of a known word, so an ALL CAPS TITLE still gets its typos found.
const ACRONYM_LEN: usize = 4;

/// Suggestions offered per misspelling. Past a handful the menu is a wall of near-identical words.
const MAX_SUGGESTIONS: usize = 5;

/// Misspellings [`misspellings`] will suggest for. Suggesting is ~1000x the cost of checking, so
/// this is a latency budget, not a layout choice: a menu opens on a click and cannot go and think
/// about a cell with forty typos in it. The diagnostic's message still names every one.
const MAX_SUGGESTED_WORDS: usize = 3;

/// The loaded dictionary, shared between the registered validator and the menu actions.
///
/// `RwLock` because [`ColumnValidator::validate`] takes `&self` while adding a word needs `&mut` —
/// the alternative is rebuilding the validator on every "Add to dictionary", which would mean
/// re-parsing half a megabyte to learn one word.
#[derive(Clone)]
pub struct SpellCheck {
    dictionaries: Arc<RwLock<DictionarySet>>,
    /// Read once at load, so changing it takes a restart — the same deal the language gets, and
    /// for the same reason: [`ColumnValidator::validate`] is handed no context to re-read from.
    ignore_capitalized: bool,
}

impl Global for SpellCheck {}

struct LoadedDictionary {
    code: String,
    dictionary: Dictionary,
}

struct DictionarySet {
    loaded: Vec<LoadedDictionary>,
    preferred: String,
}

impl DictionarySet {
    fn base(code: &str) -> &str {
        code.split('-').next().unwrap_or(code)
    }

    /// Pick one installed base language only when the dictionaries provide enough evidence.
    fn for_text(&self, text: &str) -> Option<&LoadedDictionary> {
        let tokens: Vec<&str> = words(text, false)
            .filter(|word| {
                !word.starts_with(char::is_uppercase)
                    || self
                        .loaded
                        .iter()
                        .any(|loaded| known_in_any_case(&loaded.dictionary, word).is_some())
            })
            .collect();
        let mut bases: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for (ix, loaded) in self.loaded.iter().enumerate() {
            let accepted = tokens
                .iter()
                .filter(|word| known_in_any_case(&loaded.dictionary, word).is_some())
                .count();
            let base = Self::base(&loaded.code);
            let entry = bases.entry(base).or_insert((accepted, ix));
            let configured_variant =
                Self::base(&self.preferred) == base && loaded.code == self.preferred;
            let current_is_configured = self.loaded[entry.1].code == self.preferred;
            if configured_variant || !current_is_configured && accepted > entry.0 {
                entry.1 = ix;
            }
            entry.0 = entry.0.max(accepted);
        }

        if bases.len() == 1 {
            return bases.into_values().next().map(|(_, ix)| &self.loaded[ix]);
        }
        if tokens.len() < 2 {
            return None;
        }

        let mut ranked: Vec<(usize, usize)> = bases.into_values().collect();
        ranked.sort_by_key(|score| std::cmp::Reverse(score.0));
        let winner = ranked[0];
        let runner_up = ranked.get(1).map_or(0, |score| score.0);
        let enough_coverage = winner.0 * 5 >= tokens.len() * 3;
        let clear_lead = (winner.0 - runner_up) * 5 >= tokens.len();
        (enough_coverage && clear_lead).then(|| &self.loaded[winner.1])
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FindingKind {
    Spelling,
    Capitalization,
}

enum WordOutcome {
    Clean,
    Capitalization(String),
    ProperNoun,
    Misspelled,
}

/// Whether spell checking is on. Compared against the text a stored bool renders as, so that
/// *unset* reads as on the way `AUTOSAVE_KEY != "off"` does — `effective_bool` cannot express
/// that, since it answers `false` for a key nobody has written yet.
pub fn enabled(cx: &App) -> bool {
    // `AppSettings::get` panics without the global, which a test app context has no reason to set.
    !cx.has_global::<settings::AppSettings>()
        || settings::effective_text(SPELLCHECK_ENABLED_KEY, cx) != "false"
}

/// Whether capitalized words are treated as names and skipped. Unset reads as on.
pub fn ignore_capitalized(cx: &App) -> bool {
    !cx.has_global::<settings::AppSettings>()
        || settings::effective_text(SPELLCHECK_NAMES_KEY, cx) != "false"
}

/// Dictionaries being fetched right now, so the settings list can say so rather than looking like
/// a click that did nothing. A download takes seconds and there is no progress to report from a
/// blocking request, so "in flight or not" is the whole state there is.
#[derive(Default)]
pub struct Downloading(pub std::collections::HashSet<String>);

impl Global for Downloading {}

pub fn is_downloading(code: &str, cx: &App) -> bool {
    cx.try_global::<Downloading>()
        .is_some_and(|d| d.0.contains(code))
}

/// Fetch `code` in the background and switch to it once it lands, the way tapping an uninstalled
/// language on a phone both downloads and selects it. Failures are logged rather than surfaced:
/// the row simply goes back to offering the download.
pub fn start_download(code: SharedString, cx: &mut App) {
    if is_downloading(&code, cx) {
        return;
    }
    cx.default_global::<Downloading>()
        .0
        .insert(code.to_string());
    let fetching = code.clone();
    let fetch = cx
        .background_executor()
        .spawn(async move { catalogue::download(&fetching) });
    cx.spawn(async move |cx| {
        let result = fetch.await;
        cx.update(|cx| {
            cx.default_global::<Downloading>().0.remove(code.as_ref());
            match result {
                Ok(()) => {
                    log::info!("downloaded the {} dictionary", catalogue::name_of(&code));
                    settings::set_scoped_text(SPELLCHECK_LANGUAGE_KEY, code, cx);
                }
                Err(err) => log::error!(
                    "could not download the {} dictionary, so it was not installed: {err}",
                    catalogue::name_of(&code)
                ),
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

/// The configured language, or [`DEFAULT_LANGUAGE`] when unset.
pub fn language(cx: &App) -> SharedString {
    let configured = settings::effective_text(SPELLCHECK_LANGUAGE_KEY, cx);
    if configured.is_empty() {
        DEFAULT_LANGUAGE.into()
    } else {
        configured
    }
}

impl SpellCheck {
    /// Parse every installed dictionary and replay the user's custom words over each. Slow enough
    /// that the caller must do this off the UI thread.
    pub fn load(language: &str, ignore_capitalized: bool) -> Option<Self> {
        let custom = custom_words();
        let mut codes: Vec<&str> = catalogue::BUILT_IN.to_vec();
        codes.extend(
            catalogue::listing()
                .into_iter()
                .filter(|(_, state)| *state == catalogue::State::Installed)
                .map(|(entry, _)| entry.0),
        );
        codes.sort_unstable();
        codes.dedup();

        let mut loaded = Vec::new();
        for code in codes {
            let Some((aff, dic)) = source_exact(code) else {
                continue;
            };
            let mut dictionary = match Dictionary::new(&aff, &dic) {
                Ok(dictionary) => dictionary,
                Err(err) => {
                    log::warn!("the {code} dictionary is unreadable and was skipped: {err}");
                    continue;
                }
            };
            for word in &custom {
                if let Err(err) = dictionary.add(word) {
                    log::warn!("skipped \"{word}\" from the {code} custom dictionary: {err}");
                }
            }
            loaded.push(LoadedDictionary {
                code: code.to_string(),
                dictionary,
            });
        }
        if loaded.is_empty() {
            log::error!("no installed dictionary was readable, so spell checking is off");
            return None;
        }
        let preferred = if loaded.iter().any(|d| d.code == language) {
            language
        } else {
            DEFAULT_LANGUAGE
        };
        log::info!(
            "spell checking with {} installed dictionaries, preferring {preferred}, and {} of your own words",
            loaded.len(),
            custom.len()
        );
        Some(Self {
            dictionaries: Arc::new(RwLock::new(DictionarySet {
                loaded,
                preferred: preferred.to_string(),
            })),
            ignore_capitalized,
        })
    }
}

/// The `.aff`/`.dic` pair for an installed language. Never substitute English for a missing file:
/// treating a failed French download as English would manufacture the false positives this module
/// is meant to prevent.
fn source_exact(language: &str) -> Option<(Cow<'static, str>, Cow<'static, str>)> {
    match language {
        DEFAULT_LANGUAGE => Some((Cow::Borrowed(EN_CA_AFF), Cow::Borrowed(EN_CA_DIC))),
        "en" => Some((Cow::Borrowed(EN_AFF), Cow::Borrowed(EN_DIC))),
        _ => {
            let (aff, dic) = catalogue::paths(language)?;
            Some((
                std::fs::read_to_string(aff).ok()?.into(),
                std::fs::read_to_string(dic).ok()?.into(),
            ))
        }
    }
}

/// The user's own dictionary: one word per line, `#` starts a comment. A plain text file rather
/// than a settings blob so it can be edited, backed up, and diffed like any other list.
pub fn custom_dictionary_path() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("dictionary.txt"))
}

fn custom_words() -> Vec<String> {
    let Some(path) = custom_dictionary_path() else {
        return Vec::new();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Not having one yet is the normal case, so only a real read failure is worth a line.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            log::warn!(
                "could not read your custom dictionary at {}, so its words will be flagged: {err}",
                path.display()
            );
            return Vec::new();
        }
    };
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Teach the dictionary `word` and append it to the user's file. Returns whether anything
/// changed, so the caller knows whether a revalidation is worth running.
pub fn add_word(word: &str, cx: &mut App) -> bool {
    let Some(this) = cx.try_global::<SpellCheck>().cloned() else {
        return false;
    };
    let Ok(mut dictionaries) = this.dictionaries.write() else {
        log::error!("the dictionary is poisoned, so \"{word}\" was not added");
        return false;
    };
    let mut changed = false;
    for loaded in &mut dictionaries.loaded {
        match loaded.dictionary.add(word) {
            Ok(()) => changed = true,
            Err(err) => log::warn!(
                "could not add \"{word}\" to the {} dictionary: {err}",
                loaded.code
            ),
        }
    }
    if !changed {
        return false;
    }
    drop(dictionaries);

    if let Some(path) = custom_dictionary_path() {
        let appended = std::fs::create_dir_all(path.parent().unwrap_or(&path)).and_then(|()| {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            writeln!(file, "{word}")
        });
        if let Err(err) = appended {
            log::warn!(
                "\"{word}\" was accepted for this session but could not be saved to {}, so it will be flagged again after a restart: {err}",
                path.display()
            );
        }
    }
    true
}

/// Every misspelled word in `text`, each with its ranked suggestions, in first-seen order.
/// Called when the right-click menu opens rather than stored per cell, so a diagnostic's message
/// stays a sentence for a human instead of a format the table has to parse back.
pub fn misspellings(text: &str, cx: &App) -> Vec<Misspelling> {
    let Some(this) = cx.try_global::<SpellCheck>() else {
        return Vec::new();
    };
    let Ok(dictionaries) = this.dictionaries.read() else {
        return Vec::new();
    };
    let Some(loaded) = dictionaries.for_text(text) else {
        return Vec::new();
    };
    let dictionary = &loaded.dictionary;
    let mut found: Vec<(SharedString, Vec<SharedString>)> = Vec::new();
    for word in words(text, false) {
        if found.len() == MAX_SUGGESTED_WORDS {
            break;
        }
        if !matches!(
            classify_word(dictionary, word, this.ignore_capitalized),
            WordOutcome::Misspelled
        ) || found.iter().any(|(seen, _)| seen == word)
        {
            continue;
        }
        // Measured on the shipped dictionary: ngram suggestion costs 25-40ms a word against well under
        // one, and only earns its keep on a word too mangled for the edit-distance pass to reach
        // ("documentaire" → "documentary"). So it runs as a fallback, not as the first answer.
        let mut suggestions = Vec::new();
        dictionary
            .suggester()
            .with_ngram_suggestions(false)
            .suggest(word, &mut suggestions);
        if suggestions.is_empty() {
            dictionary.suggest(word, &mut suggestions);
        }
        suggestions.truncate(MAX_SUGGESTIONS);
        found.push((
            word.into(),
            suggestions.into_iter().map(SharedString::from).collect(),
        ));
    }
    found
}

/// Capitalization corrections for the general fix-provider registry.
pub fn capitalization_fixes(_: &Location, text: &str, cx: &App) -> Vec<Fix> {
    let Some(this) = cx.try_global::<SpellCheck>() else {
        return Vec::new();
    };
    let Ok(dictionaries) = this.dictionaries.read() else {
        return Vec::new();
    };
    let Some(loaded) = dictionaries.for_text(text) else {
        return Vec::new();
    };
    words(text, false)
        .filter_map(
            |word| match classify_word(&loaded.dictionary, word, false) {
                WordOutcome::Capitalization(canonical) => Some(Fix {
                    label: format!("Use “{canonical}”").into(),
                    replacement: text.replace(word, &canonical).into(),
                }),
                _ => None,
            },
        )
        .collect()
}

/// Split `text` into the tokens worth checking. Straight and typographic apostrophes stay inside
/// words, so contractions survive as one token regardless of how they were entered.
fn words(text: &str, ignore_capitalized: bool) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .map(|token| token.trim_matches(['\'', '’']))
        .filter(move |token| checkable(token, ignore_capitalized))
}

/// Whether a token is prose rather than data. This, not the column's declared type, is what keeps
/// `AV-2019-0043` and `MP4` out of the Problems panel.
///
/// `ignore_capitalized` is the names rule, and it is blunt on purpose: a cell is a field, not a
/// sentence, so there is no sentence start to exempt and `Varda` alone in a Director column has to
/// pass. The cost is a typo that happens to be capitalized, which is why it is a setting.
fn checkable(token: &str, ignore_capitalized: bool) -> bool {
    let len = token.chars().count();
    if len < MIN_WORD_LEN || token.chars().any(char::is_numeric) {
        return false;
    }
    if len <= ACRONYM_LEN && token.chars().all(char::is_uppercase) {
        return false;
    }
    !ignore_capitalized || !token.starts_with(char::is_uppercase)
}

fn titlecase(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first
        .to_uppercase()
        .chain(chars.flat_map(char::to_lowercase))
        .collect()
}

/// Return the dictionary's canonical casing, including the exact input when it is already valid.
fn known_in_any_case(dictionary: &Dictionary, word: &str) -> Option<String> {
    let dictionary_word = word.replace('’', "'");
    if dictionary.check(&dictionary_word) {
        return Some(word.to_string());
    }
    let lower = dictionary_word.to_lowercase();
    let title = titlecase(&dictionary_word);
    let upper = dictionary_word.to_uppercase();
    [lower, title, upper]
        .into_iter()
        .find(|candidate| candidate != word && dictionary.check(candidate))
}

fn classify_word(dictionary: &Dictionary, word: &str, ignore_capitalized: bool) -> WordOutcome {
    match known_in_any_case(dictionary, word) {
        Some(canonical) if canonical == word => WordOutcome::Clean,
        Some(canonical) => WordOutcome::Capitalization(canonical),
        None if ignore_capitalized && word.starts_with(char::is_uppercase) => {
            WordOutcome::ProperNoun
        }
        None => WordOutcome::Misspelled,
    }
}

impl SpellCheck {
    fn findings(
        &self,
        column: &ColumnInfo,
        values: ColumnValues<'_>,
        kind: FindingKind,
    ) -> Vec<diagnostics::ColumnFinding> {
        if !column.settings.spellcheck || !ColumnType::from_declared(column.data_type).is_prose() {
            return Vec::new();
        }
        let Ok(dictionaries) = self.dictionaries.read() else {
            return Vec::new();
        };
        values
            .iter()
            .flat_map(|cell| {
                let mut found = Vec::new();
                let mut seen = std::collections::BTreeSet::new();
                for value in cell.parts() {
                    let Some(loaded) = dictionaries.for_text(value) else {
                        continue;
                    };
                    for word in words(value, false) {
                        if !seen.insert(word) {
                            continue;
                        }
                        let outcome =
                            classify_word(&loaded.dictionary, word, self.ignore_capitalized);
                        let (severity, message, target) = match (kind, outcome) {
                            (FindingKind::Spelling, WordOutcome::Misspelled) => (
                                Severity::Warning,
                                format!("misspelled: {word}"),
                                String::new(),
                            ),
                            (
                                FindingKind::Capitalization,
                                WordOutcome::Capitalization(canonical),
                            ) => (
                                Severity::Warning,
                                format!("capitalization: “{word}” should be “{canonical}”"),
                                canonical,
                            ),
                            _ => continue,
                        };
                        found.push(diagnostics::ColumnFinding {
                            row: Some(cell.row),
                            severity,
                            group: Some(diagnostics::DiagnosticGroup {
                                key: format!("{:?}", (&loaded.code, word, target)).into(),
                                summary: message.clone().into(),
                            }),
                            message: message.into(),
                        });
                    }
                }
                found
            })
            .collect()
    }
}

impl ColumnValidator for SpellCheck {
    fn name(&self) -> SharedString {
        VALIDATOR_NAME.into()
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: ColumnValues<'_>,
    ) -> Vec<diagnostics::ColumnFinding> {
        self.findings(column, values, FindingKind::Spelling)
    }
}

pub struct CapitalizationCheck(pub SpellCheck);

impl ColumnValidator for CapitalizationCheck {
    fn name(&self) -> SharedString {
        CAPITALIZATION_VALIDATOR_NAME.into()
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: ColumnValues<'_>,
    ) -> Vec<diagnostics::ColumnFinding> {
        self.0.findings(column, values, FindingKind::Capitalization)
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the chained glob would let gpui's `test` macro shadow the
    // `#[test]` its own expansion emits. See the note in `table`'s `note.rs` test module.
    use crate::{
        CapitalizationCheck, DictionarySet, EN_CA_AFF, EN_CA_DIC, LoadedDictionary, SpellCheck,
        checkable, words,
    };
    use diagnostics::{ColumnInfo, ColumnValidator, Severity};
    use gpui::SharedString;
    use settings::columns::ColumnSettings;
    use spellbook::Dictionary;

    fn dictionary_with_names_ignored(ignore_capitalized: bool) -> SpellCheck {
        SpellCheck {
            dictionaries: std::sync::Arc::new(std::sync::RwLock::new(DictionarySet {
                loaded: vec![LoadedDictionary {
                    code: "en-CA".to_string(),
                    dictionary: Dictionary::new(EN_CA_AFF, EN_CA_DIC)
                        .expect("the embedded en-CA dictionary parses"),
                }],
                preferred: "en-CA".to_string(),
            })),
            ignore_capitalized,
        }
    }

    fn dictionary() -> SpellCheck {
        dictionary_with_names_ignored(false)
    }

    fn findings(
        spell: &SpellCheck,
        data_type: &str,
        settings: &ColumnSettings,
        values: &[&str],
    ) -> Vec<diagnostics::ColumnFinding> {
        let values: Vec<SharedString> = values.iter().map(|v| SharedString::from(*v)).collect();
        spell.validate(
            &ColumnInfo {
                name: "Title",
                data_type,
                settings,
            },
            diagnostics::ColumnValues::new(&values, ""),
        )
    }

    fn small_dictionary(code: &str, entries: &[&str]) -> LoadedDictionary {
        let dic = format!("{}\n{}\n", entries.len(), entries.join("\n"));
        LoadedDictionary {
            code: code.to_string(),
            dictionary: Dictionary::new("SET UTF-8\n", &dic).expect("test dictionary parses"),
        }
    }

    #[test]
    fn the_embedded_dictionary_knows_english() {
        let spell = dictionary();
        let found = findings(
            &spell,
            "Text",
            &ColumnSettings::default(),
            &["I did receive the reel", "I did recieve the reel"],
        );
        assert_eq!(found.len(), 1, "only the misspelled row is reported");
        assert_eq!(found[0].row, Some(1));
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].message, "misspelled: recieve");
    }

    #[test]
    fn known_words_in_the_wrong_case_are_not_misspellings() {
        let spell = dictionary();
        let defaults = ColumnSettings::default();
        assert!(findings(&spell, "", &defaults, &["alice visited Canada"]).is_empty());
        let found = CapitalizationCheck(spell).validate(
            &ColumnInfo {
                name: "Description",
                data_type: "Text",
                settings: &defaults,
            },
            diagnostics::ColumnValues::new(&["alice visited Canada".into()], ""),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(
            found[0].message,
            "capitalization: “alice” should be “Alice”"
        );
    }

    #[test]
    fn dictionary_selection_requires_coverage_and_a_clear_lead() {
        let dictionaries = DictionarySet {
            loaded: vec![
                small_dictionary("en-CA", &["film", "history", "the"]),
                small_dictionary("fr", &["film", "histoire", "le"]),
            ],
            preferred: "en-CA".to_string(),
        };
        assert_eq!(
            dictionaries.for_text("the film history").map(|d| &*d.code),
            Some("en-CA")
        );
        assert_eq!(
            dictionaries.for_text("le film histoire").map(|d| &*d.code),
            Some("fr")
        );
        assert_eq!(
            dictionaries
                .for_text("Agnès Varda the film history")
                .map(|d| &*d.code),
            Some("en-CA"),
            "unknown title-cased names do not dilute the language evidence"
        );
        assert!(dictionaries.for_text("film").is_none());
        assert!(dictionaries.for_text("unknown tokens").is_none());
    }

    /// Which variant ships is a real choice, not a default: SCOWL size 60 carries one spelling per
    /// word, so en_CA and en_US disagree on most of these. `catalogue` is the one that decides it
    /// for a cataloguing tool — en_US rejects it.
    #[test]
    fn canadian_spellings_are_the_ones_that_pass() {
        let spell = dictionary();
        let defaults = ColumnSettings::default();
        let flags = |cell: &str| !findings(&spell, "", &defaults, &[cell]).is_empty();
        for word in [
            "catalogue",
            "colour",
            "centre",
            "theatre",
            "labour",
            "licence",
        ] {
            assert!(!flags(word), "{word} is how it is spelled here");
        }
        for word in ["catalog", "color", "theater"] {
            assert!(
                flags(word),
                "{word} is the American spelling and should be flagged"
            );
        }
    }

    /// The rules that keep an archive's identifiers and format codes out of the Problems panel.
    #[test]
    fn data_shaped_tokens_are_never_checked() {
        for token in ["AV-2019-0043", "MP4", "1080i", "a", "of", "TIFF", "NTSC"] {
            assert!(
                !checkable(token, false),
                "{token} should not reach the dictionary"
            );
        }
        for token in ["receive", "Toronto", "don't", "BROADCAST"] {
            assert!(checkable(token, false), "{token} should be checked");
        }
    }

    #[test]
    fn an_apostrophe_stays_inside_its_word() {
        assert_eq!(
            words("don't stop", false).collect::<Vec<_>>(),
            vec!["don't", "stop"]
        );
        assert_eq!(
            words("don’t stop", false).collect::<Vec<_>>(),
            vec!["don’t", "stop"]
        );
        assert_eq!(
            words("'quoted' word", false).collect::<Vec<_>>(),
            vec!["quoted", "word"]
        );
    }

    /// The two opt-outs, one per level.
    #[test]
    fn a_column_can_opt_out_by_setting_or_by_type() {
        let spell = dictionary();
        let off = ColumnSettings {
            spellcheck: false,
            ..Default::default()
        };
        assert!(findings(&spell, "Text", &off, &["recieve"]).is_empty());
        assert!(
            findings(&spell, "Date", &ColumnSettings::default(), &["recieve"]).is_empty(),
            "a declared Date column holds no prose"
        );
        assert!(
            findings(
                &spell,
                "  DATETIME ",
                &ColumnSettings::default(),
                &["recieve"]
            )
            .is_empty(),
            "a synonym spelled any way still reads as a date"
        );
        assert!(
            !findings(&spell, "", &ColumnSettings::default(), &["recieve"]).is_empty(),
            "an unconfigured column is still checked"
        );
    }

    /// What "Add to dictionary" has to achieve. Exercised on the dictionary directly because the
    /// public `add_word` also writes the user's file, which a test must not touch.
    #[test]
    fn a_learned_word_stops_being_reported() {
        let spell = dictionary();
        let defaults = ColumnSettings::default();
        let cell = ["shot on betacam"];
        assert_eq!(
            findings(&spell, "", &defaults, &cell)[0].message,
            "misspelled: betacam"
        );
        let mut dictionaries = spell.dictionaries.write().expect("uncontended");
        for loaded in &mut dictionaries.loaded {
            loaded
                .dictionary
                .add("betacam")
                .expect("a bare word is a valid .dic line");
        }
        drop(dictionaries);
        assert!(findings(&spell, "", &defaults, &cell).is_empty());
    }

    /// The names rule. No word list holds every filmmaker, town, and studio, so the only general
    /// answer is to stop treating a capital letter as a claim about spelling.
    #[test]
    fn capitalized_words_are_names_when_the_rule_is_on() {
        let names = dictionary_with_names_ignored(true);
        let defaults = ColumnSettings::default();
        // Varda and Anansi are absent from SCOWL, so both flag with the rule off.
        assert_eq!(
            findings(&dictionary(), "", &defaults, &["Agnès Varda"]).len(),
            2
        );
        assert!(findings(&names, "", &defaults, &["Agnès Varda"]).is_empty());
        assert!(findings(&names, "", &defaults, &["Anansi the Spider"]).is_empty());
        // A lowercase typo still gets caught, which is the whole point of scoping it to capitals.
        assert_eq!(
            findings(&names, "", &defaults, &["Varda recieved it"])[0].message,
            "misspelled: recieved"
        );
        assert!(checkable("recieve", true));
        assert!(!checkable("Recieve", true));
    }

    #[test]
    fn every_misspelling_in_a_row_is_named_once() {
        let spell = dictionary();
        let found = findings(
            &spell,
            "",
            &ColumnSettings::default(),
            &["teh recieve teh reel"],
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].message, "misspelled: teh");
        assert_eq!(found[1].message, "misspelled: recieve");
    }

    #[test]
    fn repeated_words_share_groups_but_not_locations() {
        let found = findings(
            &dictionary(),
            "",
            &ColumnSettings::default(),
            &["recieve recieve", "recieve"],
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].group, found[1].group);
        assert_eq!(found[0].row, Some(0));
        assert_eq!(found[1].row, Some(1));
    }

    #[test]
    fn a_hundred_cells_emit_one_word_group() {
        let found = findings(
            &dictionary(),
            "",
            &ColumnSettings::default(),
            &["recieve"; 100],
        );
        assert_eq!(found.len(), 100);
        assert!(found.iter().all(|finding| finding.group == found[0].group));
    }

    #[test]
    fn subdelimited_names_are_checked_as_separate_values() {
        let spell = dictionary();
        let values = ["receive the reel|recieve the film".into()];
        let found = spell.validate(
            &ColumnInfo {
                name: "Title",
                data_type: "Text",
                settings: &ColumnSettings::default(),
            },
            diagnostics::ColumnValues::new(&values, "|"),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "misspelled: recieve");
    }
}

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
use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use diagnostics::{
    ColumnInfo, ColumnValidator, ColumnValues, GroupFix, GroupMember, Misspelling, Severity,
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
pub const SPELLING_VALIDATOR_NAME: &str = "spell";

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
    /// Words accepted while a run held the dictionaries, taught at the start of the next run so
    /// the UI thread never waits on a column being checked.
    learned: Arc<Mutex<Vec<String>>>,
    /// Bumped whenever a word is taught, since that changes answers for unchanged columns.
    revision: Arc<AtomicU64>,
}

impl Global for SpellCheck {}

struct LoadedDictionary {
    code: String,
    dictionary: Dictionary,
}

struct DictionarySet {
    loaded: Vec<LoadedDictionary>,
    preferred: String,
    /// Each distinct cell value’s verdict, kept across runs — checking the sheet is seconds, and
    /// almost none of it changes between runs. Only [`Self::learn`] changes an answer, so only it
    /// clears this.
    checked: Mutex<HashMap<String, Checked>>,
}

/// The dictionary chosen for one value and its flagged words, or `None` when no language fits.
type Checked = Option<(String, Vec<(String, WordOutcome)>)>;

/// ponytail: past this many distinct values the cache is dropped wholesale; evict by age if edits
/// ever churn through it in a session.
const MAX_CHECKED: usize = 200_000;

impl DictionarySet {
    fn base(code: &str) -> &str {
        code.split('-').next().unwrap_or(code)
    }

    /// Teach every loaded dictionary `word`. Returns whether any accepted it.
    fn learn(&mut self, word: &str) -> bool {
        let mut changed = false;
        for loaded in &mut self.loaded {
            match loaded.dictionary.add(word) {
                Ok(()) => changed = true,
                Err(err) => log::warn!(
                    "could not add \"{word}\" to the {} dictionary: {err}",
                    loaded.code
                ),
            }
        }
        if changed && let Ok(checked) = self.checked.get_mut() {
            let folded = |text: &str| text.replace('’', "'").to_lowercase();
            let word = folded(word);
            checked.retain(|value, _| !folded(value).contains(&word));
        }
        changed
    }

    fn check(&self, value: &str, ignore_capitalized: bool) -> Checked {
        let loaded = self.for_text(value)?;
        let english = Self::base(&loaded.code) == "en";
        let mut seen = std::collections::BTreeSet::new();
        let flagged = words(value, false)
            .filter(|word| !(english && non_english_elision(word)) && seen.insert(*word))
            .filter_map(
                |word| match classify_word(&loaded.dictionary, word, ignore_capitalized) {
                    outcome @ (WordOutcome::Misspelled | WordOutcome::Capitalization(_)) => {
                        Some((word.to_owned(), outcome))
                    }
                    WordOutcome::Clean | WordOutcome::ProperNoun => None,
                },
            )
            .collect();
        Some((loaded.code.clone(), flagged))
    }

    /// Pick one installed base language only when the dictionaries provide enough evidence.
    fn for_text(&self, text: &str) -> Option<&LoadedDictionary> {
        let english_only = self
            .loaded
            .iter()
            .all(|loaded| Self::base(&loaded.code) == "en");
        let enough_text = text.chars().filter(|c| c.is_alphabetic()).count() >= 20;
        if english_only
            && enough_text
            && whatlang::detect(text).is_some_and(|info| {
                info.lang() != whatlang::Lang::Eng
                    && (info.is_reliable() || info.confidence() >= 0.45)
            })
        {
            return None;
        }
        let tokens: Vec<&str> = words(text, false)
            .filter(|word| {
                !starts_uppercase(word)
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

#[derive(Clone)]
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
pub struct Downloading {
    active: std::collections::HashSet<String>,
    wanted: std::collections::HashSet<String>,
}

impl Global for Downloading {}

pub fn is_downloading(code: &str, cx: &App) -> bool {
    cx.try_global::<Downloading>()
        .is_some_and(|d| d.active.contains(code))
}

pub fn retain_wanted_downloads(selected: &[SharedString], cx: &mut App) {
    let downloading = cx.default_global::<Downloading>();
    downloading
        .wanted
        .retain(|code| selected.iter().any(|selected| selected == code));
}

/// Fetch `code` in the background and add it to the checked languages once it lands, the way
/// tapping an uninstalled language on a phone both downloads and selects it. `then` runs after the
/// selection changes, so the caller can reload the checker. Failures are logged rather than
/// surfaced: the row simply goes back to offering the download.
pub fn start_download(code: SharedString, then: fn(&mut App), cx: &mut App) {
    if is_downloading(&code, cx) {
        return;
    }
    let downloading = cx.default_global::<Downloading>();
    downloading.active.insert(code.to_string());
    downloading.wanted.insert(code.to_string());
    let fetching = code.clone();
    let fetch = cx
        .background_executor()
        .spawn(async move { catalogue::download(&fetching) });
    cx.spawn(async move |cx| {
        let result = fetch.await;
        cx.update(|cx| {
            let downloading = cx.default_global::<Downloading>();
            downloading.active.remove(code.as_ref());
            let wanted = downloading.wanted.remove(code.as_ref());
            match result {
                Ok(()) if wanted => {
                    log::info!("downloaded the {} dictionary", catalogue::name_of(&code));
                    let mut codes = languages(cx);
                    codes.push(code.to_string());
                    set_languages(&codes, cx);
                    then(cx);
                }
                Ok(()) => log::info!(
                    "downloaded the {} dictionary without selecting it",
                    catalogue::name_of(&code)
                ),
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

/// The languages to check with, in the order they were picked, or [`DEFAULT_LANGUAGE`] when none
/// are. Stored comma-separated, so a setting written before multi-select still reads as one.
pub fn languages(cx: &App) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for code in settings::effective_text(SPELLCHECK_LANGUAGE_KEY, cx).split(',') {
        let code = code.trim();
        if !code.is_empty() && !codes.iter().any(|c| c == code) {
            codes.push(code.to_owned());
        }
    }
    if codes.is_empty() {
        codes.push(DEFAULT_LANGUAGE.to_owned());
    }
    codes
}

pub fn set_languages(codes: &[String], cx: &mut App) {
    settings::set_user_text(SPELLCHECK_LANGUAGE_KEY, codes.join(",").into(), cx);
}

impl SpellCheck {
    /// Parse the chosen dictionaries and replay the user's custom words over each. Slow enough
    /// that the caller must do this off the UI thread.
    pub fn load(languages: &[String], ignore_capitalized: bool) -> Option<Self> {
        let custom = custom_words();
        let mut loaded = Vec::new();
        for code in languages {
            let Some((aff, dic)) = source_exact(code) else {
                log::warn!("the {code} dictionary is not installed, so it was skipped");
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
                code: code.clone(),
                dictionary,
            });
        }
        if loaded.is_empty() {
            log::error!("none of the chosen dictionaries was readable, so spell checking is off");
            return None;
        }
        let preferred = loaded[0].code.clone();
        log::info!(
            "spell checking with {}, preferring {preferred}, and {} of your own words",
            loaded
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            custom.len()
        );
        Some(Self {
            dictionaries: Arc::new(RwLock::new(DictionarySet {
                loaded,
                preferred: preferred.to_string(),
                checked: Default::default(),
            })),
            ignore_capitalized,
            learned: Arc::default(),
            revision: Arc::default(),
        })
    }

    /// Teach `word` now if no run is reading the dictionaries, else queue it for the next run.
    /// Returns whether it was accepted, which a queued word is assumed to be.
    fn learn(&self, word: &str) -> bool {
        match self.dictionaries.try_write() {
            Ok(mut dictionaries) => {
                let changed = dictionaries.learn(word);
                if changed {
                    self.revision.fetch_add(1, Ordering::SeqCst);
                }
                changed
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                self.learned
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(word.to_owned());
                true
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                log::error!("the dictionary is poisoned, so \"{word}\" was not added");
                false
            }
        }
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
    let Some(this) = cx.try_global::<SpellCheck>() else {
        return false;
    };
    if !this.learn(word) {
        return false;
    }

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
        if DictionarySet::base(&loaded.code) == "en" && non_english_elision(word) {
            continue;
        }
        if found.len() == MAX_SUGGESTED_WORDS {
            break;
        }
        if found.iter().any(|(seen, _)| seen == word) {
            continue;
        }
        if let Some(corrections) = corrections(dictionary, word, this.ignore_capitalized) {
            found.push((word.into(), corrections));
        }
    }
    found
}

/// What a flagged word could become: the dictionary's casing for a capitalization slip, else
/// ranked suggestions. `None` for a word nothing flags.
fn corrections(
    dictionary: &Dictionary,
    word: &str,
    ignore_capitalized: bool,
) -> Option<Vec<SharedString>> {
    match classify_word(dictionary, word, ignore_capitalized) {
        WordOutcome::Misspelled => Some(suggestions(dictionary, word)),
        WordOutcome::Capitalization(canonical) => Some(vec![canonical.into()]),
        WordOutcome::Clean | WordOutcome::ProperNoun => None,
    }
}

fn suggestions(dictionary: &Dictionary, word: &str) -> Vec<SharedString> {
    // N-gram suggestions cost 25-40ms per word, so use them only when edit distance finds nothing.
    let mut suggestions = Vec::new();
    dictionary
        .suggester()
        .with_ngram_suggestions(false)
        .suggest(word, &mut suggestions);
    if suggestions.is_empty() {
        dictionary.suggest(word, &mut suggestions);
    }
    suggestions.truncate(MAX_SUGGESTIONS);
    suggestions.into_iter().map(SharedString::from).collect()
}

pub fn spelling_group_fixes(members: &[GroupMember], cx: &App) -> Vec<GroupFix> {
    let Some(first) = members.first() else {
        return Vec::new();
    };
    let Some(word) = first.subject.as_deref() else {
        return Vec::new();
    };
    let Some(this) = cx.try_global::<SpellCheck>() else {
        return Vec::new();
    };
    let Ok(dictionaries) = this.dictionaries.read() else {
        return Vec::new();
    };
    let Some(loaded) = dictionaries.for_text(&first.text) else {
        return Vec::new();
    };
    let Some(corrections) = corrections(&loaded.dictionary, word, this.ignore_capitalized) else {
        return Vec::new();
    };
    let learned = word.to_owned();
    let add = GroupFix::action(format!("Add “{word}” to dictionary"), move |cx| {
        if add_word(&learned, cx)
            && let Some(hooks) = cx.try_global::<diagnostics::DiagnosticHooks>().copied()
        {
            (hooks.revalidate)(cx);
        }
    });
    corrections
        .into_iter()
        .filter_map(|suggestion| {
            let replacements = members
                .iter()
                .map(|member| {
                    (member.subject.as_deref() == Some(word)).then(|| {
                        (
                            member.location.clone(),
                            member.text.replace(word, suggestion.as_ref()).into(),
                        )
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(GroupFix::replacements(
                format!("Change all “{word}” to “{suggestion}”"),
                replacements,
            ))
        })
        .chain([add])
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
    !ignore_capitalized || !starts_uppercase(token)
}

fn non_english_elision(word: &str) -> bool {
    let Some(split) = word.find(['\'', '’']) else {
        return false;
    };
    matches!(
        word[..split].to_lowercase().as_str(),
        "c" | "d" | "j" | "l" | "m" | "n" | "qu" | "s" | "t"
    )
}

fn starts_uppercase(value: &str) -> bool {
    value.chars().next().is_some_and(char::is_uppercase)
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
        None if ignore_capitalized && starts_uppercase(word) => WordOutcome::ProperNoun,
        None => WordOutcome::Misspelled,
    }
}

impl ColumnValidator for SpellCheck {
    fn name(&self) -> SharedString {
        SPELLING_VALIDATOR_NAME.into()
    }

    fn begin_run(&self, _columns: &[SharedString]) {
        let learned =
            std::mem::take(&mut *self.learned.lock().unwrap_or_else(PoisonError::into_inner));
        if learned.is_empty() {
            return;
        }
        let Ok(mut dictionaries) = self.dictionaries.write() else {
            return;
        };
        let mut changed = false;
        for word in &learned {
            changed |= dictionaries.learn(word);
        }
        if changed {
            self.revision.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    fn validate(
        &self,
        column: &ColumnInfo,
        values: ColumnValues<'_>,
    ) -> Vec<diagnostics::ColumnFinding> {
        if !column.settings.spellcheck || !ColumnType::from_declared(column.data_type).is_prose() {
            return Vec::new();
        }
        let Ok(dictionaries) = self.dictionaries.read() else {
            return Vec::new();
        };
        let Ok(mut checked) = dictionaries.checked.lock() else {
            return Vec::new();
        };
        if checked.len() > MAX_CHECKED {
            checked.clear();
        }
        values
            .iter()
            .flat_map(|cell| {
                let mut found = Vec::new();
                let mut seen = std::collections::BTreeSet::new();
                for value in cell.parts() {
                    if !checked.contains_key(value) {
                        let verdict = dictionaries.check(value, self.ignore_capitalized);
                        checked.insert(value.to_owned(), verdict);
                    }
                    let Some((code, flagged)) = &checked[value] else {
                        continue;
                    };
                    for (word, outcome) in flagged {
                        if !seen.insert(word.clone()) {
                            continue;
                        }
                        let (message, target) = match outcome {
                            WordOutcome::Misspelled => (format!("misspelled: {word}"), ""),
                            WordOutcome::Capitalization(canonical) => (
                                format!("capitalization: “{word}” should be “{canonical}”"),
                                canonical.as_str(),
                            ),
                            WordOutcome::Clean | WordOutcome::ProperNoun => continue,
                        };
                        found.push(diagnostics::ColumnFinding {
                            row: Some(cell.row),
                            severity: Severity::Warning,
                            group: Some(diagnostics::DiagnosticGroup {
                                key: format!("{:?}", (code, word, target)).into(),
                                summary: message.clone().into(),
                                subject: Some(word.clone().into()),
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

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the chained glob would let gpui's `test` macro shadow the
    // `#[test]` its own expansion emits. See the note in `table`'s `note.rs` test module.
    use crate::{
        DictionarySet, Downloading, EN_CA_AFF, EN_CA_DIC, LoadedDictionary, SpellCheck, checkable,
        retain_wanted_downloads, spelling_group_fixes, words,
    };
    use diagnostics::{ColumnInfo, ColumnValidator, DATASET_MAIN, GroupMember, Location, Severity};
    use gpui::{SharedString, TestAppContext};
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
                checked: Default::default(),
            })),
            ignore_capitalized,
            learned: Default::default(),
            revision: Default::default(),
        }
    }

    fn dictionary() -> SpellCheck {
        dictionary_with_names_ignored(false)
    }

    #[gpui::test]
    fn deselecting_an_in_flight_download_keeps_it_unselected(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mut downloading = Downloading::default();
            downloading.active.insert("fr".into());
            downloading.wanted.insert("fr".into());
            cx.set_global(downloading);
            retain_wanted_downloads(&[], cx);
            let downloading = cx.global::<Downloading>();
            assert!(downloading.active.contains("fr"));
            assert!(!downloading.wanted.contains("fr"));
        });
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
    fn known_words_in_the_wrong_case_are_capitalization_not_misspellings() {
        let found = findings(
            &dictionary(),
            "",
            &ColumnSettings::default(),
            &["alice visited Canada"],
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
            checked: Default::default(),
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

    #[test]
    fn unsupported_non_english_text_is_not_checked_as_english() {
        let spell = dictionary();
        let defaults = ColumnSettings::default();
        let french = "bonjour, je suis heureux de vous rencontrer";
        assert!(
            findings(&spell, "", &defaults, &["l’enfant"]).is_empty(),
            "a common non-English elision is not an English misspelling"
        );
        assert!(
            findings(&spell, "", &defaults, &[french]).is_empty(),
            "reliably identified French text is not checked with an English dictionary: {:?}",
            whatlang::detect(french)
        );
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
        assert!(
            spell
                .dictionaries
                .write()
                .expect("uncontended")
                .learn("betacam")
        );
        assert!(findings(&spell, "", &defaults, &cell).is_empty());
    }

    /// A run holds the dictionaries for a whole column, so a word accepted meanwhile waits for the
    /// next run instead of blocking the UI thread — and that run's answers change with it.
    #[test]
    fn a_word_learned_during_a_run_is_taught_by_the_next_one() {
        let spell = dictionary();
        let defaults = ColumnSettings::default();
        let cell = ["shot on betacam"];
        let running = spell.dictionaries.read().expect("uncontended");
        assert!(spell.learn("betacam"), "queued rather than refused");
        drop(running);
        assert_eq!(findings(&spell, "", &defaults, &cell).len(), 1);

        let before = spell.revision();
        spell.begin_run(&[]);
        assert!(spell.revision() > before, "cached answers are stale");
        assert!(findings(&spell, "", &defaults, &cell).is_empty());
    }

    /// Learning a word forgets only the cached verdicts that could mention it.
    #[test]
    fn learning_a_word_keeps_unrelated_cached_verdicts() {
        let spell = dictionary();
        findings(
            &spell,
            "",
            &ColumnSettings::default(),
            &["shot on Betacam", "I did recieve the reel"],
        );
        assert!(spell.learn("betacam"));
        let dictionaries = spell.dictionaries.read().expect("uncontended");
        let checked = dictionaries.checked.lock().expect("uncontended");
        assert!(!checked.contains_key("shot on Betacam"));
        assert!(checked.contains_key("I did recieve the reel"));
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

    #[gpui::test]
    fn grouped_words_offer_one_resolution_for_every_cell(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(dictionary());
            let members = |text: &str, message: &str| {
                [0, 1].map(|row| GroupMember {
                    location: Location::cell(DATASET_MAIN, row, None, "Title"),
                    text: text.into(),
                    message: message.into(),
                    subject: Some(message.split_whitespace().last().unwrap_or_default().into()),
                })
            };
            assert!(
                spelling_group_fixes(&members("recieve it", "misspelled: recieve"), cx)
                    .iter()
                    .any(|fix| fix.label == "Change all “recieve” to “receive”")
            );
            let capitalization = spelling_group_fixes(
                &members("alice visited Canada", "capitalization: alice"),
                cx,
            );
            assert_eq!(
                capitalization
                    .iter()
                    .map(|fix| fix.label.as_ref())
                    .collect::<Vec<_>>(),
                ["Change all “alice” to “Alice”", "Add “alice” to dictionary"]
            );
        });
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

#![allow(unused)]

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, RwLock};
use symspell_rs::{AsciiStringStrategy, SymSpell, SymSpellBuilder, Verbosity};
use tokio::task;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellError {
    pub start: usize,
    pub end: usize,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellCheckResult {
    pub errors: Vec<SpellError>,
    pub words_checked: usize,
    pub fragment_start: usize,
    pub fragment_end: usize,
}

struct CompositeDictionary {
    engine: SymSpell<AsciiStringStrategy>,
    ignored_words: HashSet<String>,
    user_words: HashSet<String>,
}

impl CompositeDictionary {
    fn new() -> Self {
        let engine = SymSpellBuilder::default()
            .max_dictionary_edit_distance(2)
            .prefix_length(7)
            .build()
            .expect("failed to initialize SymSpell");

        Self {
            engine,
            ignored_words: HashSet::new(),
            user_words: HashSet::new(),
        }
    }

    fn load_frequency_dictionary_from_reader<R: BufRead>(
        &mut self,
        reader: R,
    ) -> Result<(), String> {
        for line in reader.lines() {
            let raw = line.map_err(|e| format!("failed to read dictionary line: {e}"))?;
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some((term, freq)) = parse_dictionary_line(trimmed) {
                self.engine.load_dictionary_line(&term, freq, 0, " ");
            }
        }

        Ok(())
    }

    fn check_fragment(&self, text: &str, fragment_start: usize) -> SpellCheckResult {
        let spans = extract_words(text, fragment_start);
        let mut errors = Vec::new();

        for span in spans.iter() {
            if self.is_known(&span.token) {
                continue;
            }

            let suggestions = self.lookup_suggestions(&span.token, 5);
            errors.push(SpellError {
                start: span.start,
                end: span.end,
                suggestions,
            });
        }

        SpellCheckResult {
            words_checked: spans.len(),
            fragment_start,
            fragment_end: fragment_start + text.len(),
            errors,
        }
    }

    fn is_known(&self, token: &str) -> bool {
        if token.is_empty()
            || self.ignored_words.contains(token)
            || self.user_words.contains(token)
            || token.len() <= 1
            || token.chars().any(|c| c.is_numeric())
        {
            return true;
        }

        self.engine
            .lookup(token, Verbosity::Top, 0)
            .into_iter()
            .next()
            .map(|suggestion| suggestion.term == token)
            .unwrap_or(false)
    }

    fn lookup_suggestions(&self, token: &str, cap: usize) -> Vec<String> {
        self.engine
            .lookup(token, Verbosity::Closest, 2)
            .into_iter()
            .take(cap)
            .map(|s| s.term)
            .collect()
    }

    fn add_user_word(&mut self, word: &str) {
        if let Some(normalized) = normalize_token(word) {
            self.user_words.insert(normalized.clone());
            self.engine
                .load_dictionary_line(&normalized, 1_000_000, 0, " ");
        }
    }

    fn add_ignored_word(&mut self, word: &str) {
        if let Some(normalized) = normalize_token(word) {
            self.ignored_words.insert(normalized);
        }
    }
}

fn parse_dictionary_line(line: &str) -> Option<(String, i64)> {
    let mut parts = line.split_whitespace();
    let term = parts.next()?.to_lowercase();
    let freq = parts.next().unwrap_or("1").parse::<i64>().ok()?;
    Some((term, freq))
}

fn normalize_token(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim_matches(|c: char| !(c.is_alphabetic() || c == '\''))
        .to_lowercase();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

struct WordSpan {
    start: usize,
    end: usize,
    token: String,
}

fn extract_words(text: &str, base_offset: usize) -> Vec<WordSpan> {
    let mut spans = Vec::new();
    let mut word_start: Option<usize> = None;

    for (idx, ch) in text.char_indices() {
        if ch.is_alphabetic() || ch == '\'' {
            word_start.get_or_insert(idx);
        } else if let Some(start) = word_start.take() {
            push_token(text, base_offset, start, idx, &mut spans);
        }
    }

    if let Some(start) = word_start.take() {
        push_token(text, base_offset, start, text.len(), &mut spans);
    }

    spans
}

fn push_token(text: &str, base_offset: usize, start: usize, end: usize, spans: &mut Vec<WordSpan>) {
    if start >= end {
        return;
    }

    let snippet = &text[start..end];
    let trimmed = snippet.trim_matches('\'');
    if trimmed.is_empty() {
        return;
    }

    let trim_offset = snippet.find(trimmed).unwrap_or(0);
    let token_start = base_offset + start + trim_offset;
    let token_end = token_start + trimmed.len();

    spans.push(WordSpan {
        start: token_start,
        end: token_end,
        token: trimmed.to_lowercase(),
    });
}

#[derive(Clone)]
pub struct SpellCheckState {
    dictionary: Arc<RwLock<CompositeDictionary>>,
}

impl SpellCheckState {
    pub fn new() -> Self {
        Self {
            dictionary: Arc::new(RwLock::new(CompositeDictionary::new())),
        }
    }

    pub fn load_frequency_dictionary_from_path<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), String> {
        let file = File::open(path.as_ref())
            .map_err(|e| format!("failed to open dictionary {:?}: {e}", path.as_ref()))?;
        let reader = BufReader::new(file);
        let mut guard = self
            .dictionary
            .write()
            .map_err(|e| format!("failed to acquire dictionary write lock: {e}"))?;
        guard.load_frequency_dictionary_from_reader(reader)
    }
}

impl Default for SpellCheckState {
    fn default() -> Self {
        Self::new()
    }
}

async fn spawn_spellcheck(
    dictionary: Arc<RwLock<CompositeDictionary>>,
    text: String,
    fragment_start: usize,
) -> Result<SpellCheckResult, String> {
    task::spawn_blocking(move || {
        let guard = dictionary
            .read()
            .map_err(|e| format!("failed to acquire dictionary read lock: {e}"))?;
        Ok(guard.check_fragment(&text, fragment_start))
    })
    .await
    .map_err(|e| format!("spell check task join error: {e}"))?
}

#[tauri::command]
pub async fn check_text_fragment(
    text: String,
    fragment_start: usize,
    state: tauri::State<'_, SpellCheckState>,
) -> Result<SpellCheckResult, String> {
    let dictionary = Arc::clone(&state.dictionary);
    spawn_spellcheck(dictionary, text, fragment_start).await
}

#[tauri::command]
pub async fn check_spelling(
    text: String,
    state: tauri::State<'_, SpellCheckState>,
) -> Result<SpellCheckResult, String> {
    let dictionary = Arc::clone(&state.dictionary);
    spawn_spellcheck(dictionary, text, 0).await
}

#[tauri::command]
pub async fn get_suggestions(
    word: String,
    max_suggestions: Option<usize>,
    state: tauri::State<'_, SpellCheckState>,
) -> Result<Vec<String>, String> {
    let dictionary = Arc::clone(&state.dictionary);
    let limit = max_suggestions.unwrap_or(5);
    let normalized = normalize_token(&word).unwrap_or_else(|| word.to_lowercase());

    task::spawn_blocking(move || {
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let guard = dictionary
            .read()
            .map_err(|e| format!("failed to acquire dictionary read lock: {e}"))?;
        Ok(guard.lookup_suggestions(&normalized, limit))
    })
    .await
    .map_err(|e| format!("suggestion task join error: {e}"))?
}

#[tauri::command]
pub async fn ignore_word(
    word: String,
    state: tauri::State<'_, SpellCheckState>,
) -> Result<(), String> {
    let dictionary = Arc::clone(&state.dictionary);

    task::spawn_blocking(move || {
        let mut guard = dictionary
            .write()
            .map_err(|e| format!("failed to acquire dictionary write lock: {e}"))?;
        guard.add_ignored_word(&word);
        Ok(())
    })
    .await
    .map_err(|e| format!("ignore word task join error: {e}"))?
}

#[tauri::command]
pub async fn add_to_dictionary(
    word: String,
    state: tauri::State<'_, SpellCheckState>,
) -> Result<(), String> {
    let dictionary = Arc::clone(&state.dictionary);

    task::spawn_blocking(move || {
        let mut guard = dictionary
            .write()
            .map_err(|e| format!("failed to acquire dictionary write lock: {e}"))?;
        guard.add_user_word(&word);
        Ok(())
    })
    .await
    .map_err(|e| format!("add word task join error: {e}"))?
}

#[tauri::command]
pub async fn is_word_correct(
    word: String,
    state: tauri::State<'_, SpellCheckState>,
) -> Result<bool, String> {
    let dictionary = Arc::clone(&state.dictionary);
    let normalized = normalize_token(&word).unwrap_or_else(|| word.to_lowercase());

    task::spawn_blocking(move || {
        if normalized.is_empty() {
            return Ok(true);
        }
        let guard = dictionary
            .read()
            .map_err(|e| format!("failed to acquire dictionary read lock: {e}"))?;
        Ok(guard.is_known(&normalized))
    })
    .await
    .map_err(|e| format!("word check task join error: {e}"))?
}

#[tauri::command]
pub async fn load_dictionary_file(
    path: String,
    state: tauri::State<'_, SpellCheckState>,
) -> Result<(), String> {
    let state_clone = state.inner().clone();

    task::spawn_blocking(move || state_clone.load_frequency_dictionary_from_path(path))
        .await
        .map_err(|e| format!("dictionary load task join error: {e}"))?
}

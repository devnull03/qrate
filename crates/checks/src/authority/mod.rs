//! Checks a column's values against an authority file — LCSH today, others by writing one small
//! struct (see [`source::AuthoritySource`]).
//!
//! Built in rather than shipped as a plugin because these are the lists the archival standards
//! name: a subject heading, a personal name, a place. A cataloguer should not have to install
//! anything to be told a heading doesn't exist.
//!
//! The shape is the one the Islandora plugin proved: ask about a term where it lives instead of
//! downloading the list, cache the answer — including the *rejections*, which is what stops a
//! wrong value being re-asked on every keystroke — and never let a server having a bad day look
//! like a cataloguer being wrong.

mod geonames;
mod lcsh;
pub mod source;
mod wikidata;

pub use geonames::NAME as GEONAMES;
pub use lcsh::NAME as LCSH;
pub use wikidata::NAME as WIKIDATA;

/// Percent-encode a query value. Hand-rolled because this is the only escaping in the crate and
/// the alternative is a dependency to encode a dozen characters.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use diagnostics::{
    ColumnFinding, ColumnSnapshot, DATASET_MAIN, Diagnostic, DiagnosticGroup, Diagnostics, Fix,
    FixProviders, GroupFix, Location, Severity, Source, SourceActions,
};
use gpui::{App, AppContext as _, Global, SharedString, Task};

use source::AuthoritySource;

/// Wait for typing to stop before calling out. Longer than the plugin host's debounce because
/// this leaves the machine.
const DEBOUNCE: Duration = Duration::from_millis(400);

/// How long a source's answers settle before its cache file is rewritten. A first pass over a
/// sheet lands a batch every few hundred milliseconds, and each write is the whole file.
const WRITE_DELAY: Duration = Duration::from_secs(2);

/// Terms looked up per run. The rest are picked up by the next run, so a freshly imported sheet
/// of ten thousand subjects resolves over several passes instead of one long stall — the same
/// budget the Islandora plugin uses.
const PER_RUN: usize = 50;

/// Requests allowed per [`WINDOW`], matching `plugin-host`'s ceiling for `qrate.http.get`. A
/// backstop against a loop nobody meant to write, not a politeness limit.
const BUDGET: u32 = 120;
const WINDOW: Duration = Duration::from_secs(60);

/// Per request. A server that has not answered in this long is not going to.
const TIMEOUT: Duration = Duration::from_secs(10);

/// How long a term whose request failed waits before it is asked again.
const RETRY_AFTER: Duration = Duration::from_secs(300);

/// What an authority said about one term.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Verdict {
    /// Whether the authority holds this exact heading.
    pub known: bool,
    /// Near matches, best first — what to offer when it doesn't.
    pub suggestions: Vec<String>,
}

/// One source's verdicts by lowercased term. Shared so a pending file write holds the map
/// without copying it on the UI thread.
type Verdicts = Arc<HashMap<String, Verdict>>;

/// Column name → the column revision and cache epoch its findings were made for.
type Published = HashMap<SharedString, (u64, u64, Vec<Diagnostic>)>;

/// Every verdict this machine has been told, by source name then lowercased term.
///
/// Lives beside the other caches in the data dir, never in the `.qrate` file: a verdict is about
/// what a server said, and a project file is a thing people commit and hand to each other.
#[derive(Default)]
struct Cache {
    terms: HashMap<String, Verdicts>,
    scopes: HashMap<String, String>,
    failures: HashMap<String, String>,
    /// Terms a request is out for, per source, so an overlapping run never asks for them twice.
    pending: HashMap<String, HashSet<String>>,
    /// Terms whose request failed, per source, with when they may be asked again.
    unanswered: HashMap<String, HashMap<String, Instant>>,
    /// Whether the files on disk have been read. Nothing is asked of a server before they are.
    loaded: bool,
    load: Option<Task<()>>,
    /// Keeps the in-flight run alive; dropping it cancels, which is what a newer run wants.
    run: Option<Task<()>>,
    /// The newest run's columns, which an answer or a finished load is published against.
    last: Option<Rc<Run>>,
    /// Bumped whenever a verdict or failure changes, which is what stales [`Self::published`].
    epochs: HashMap<String, u64>,
    published: HashMap<&'static str, Published>,
    writes: HashMap<String, Task<()>>,
}

impl Global for Cache {}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredCache<T> {
    scope: String,
    terms: T,
}

fn cache_path(source: &str) -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("authority").join(format!("{source}.json")))
}

/// Read every source's cache off disk once per launch, off the UI thread. An unreadable file
/// reads as nothing cached, which is always safe: a cache that cannot be read is one that gets
/// refilled.
fn ensure_loaded(cx: &mut App) {
    let cache = cx.default_global::<Cache>();
    if cache.loaded || cache.load.is_some() {
        return;
    }
    let read = cx.background_executor().spawn(async { read_caches() });
    let task = cx.spawn(async move |cx| {
        let stored = read.await;
        cx.update(|cx| {
            let cache = cx.default_global::<Cache>();
            for (name, scope, terms) in stored {
                if cache
                    .scopes
                    .get(&name)
                    .is_some_and(|current| *current != scope)
                {
                    continue;
                }
                cache.scopes.insert(name.clone(), scope);
                *cache.epochs.entry(name.clone()).or_default() += 1;
                let held = Arc::make_mut(cache.terms.entry(name).or_default());
                for (term, verdict) in terms {
                    held.entry(term).or_insert(verdict);
                }
            }
            cache.loaded = true;
            resume(cx);
        });
    });
    cx.default_global::<Cache>().load = Some(task);
}

fn read_caches() -> Vec<(String, String, HashMap<String, Verdict>)> {
    source::NAMES
        .iter()
        .filter_map(|name| {
            let text = std::fs::read_to_string(cache_path(name)?).ok()?;
            if let Ok(stored) = serde_json::from_str::<StoredCache<HashMap<_, _>>>(&text) {
                return Some((name.to_string(), stored.scope, stored.terms));
            }
            if let Ok(stored) = serde_json::from_str::<HashMap<String, (bool, Vec<String>)>>(&text)
            {
                let terms = stored
                    .into_iter()
                    .map(|(term, (known, suggestions))| (term, Verdict { known, suggestions }))
                    .collect();
                return Some((name.to_string(), name.to_string(), terms));
            }
            log::warn!("discarding the unreadable {name} cache");
            None
        })
        .collect()
}

fn write_cache(source: &str, scope: &str, verdicts: &HashMap<String, Verdict>) {
    let Some(path) = cache_path(source) else {
        return;
    };
    let stored = StoredCache {
        scope: scope.to_owned(),
        terms: verdicts,
    };
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| serde_json::to_string(&stored).map_err(std::io::Error::other))
        .and_then(|json| std::fs::write(&path, json));
    if let Err(err) = written {
        log::warn!(
            "could not save the {source} cache to {}: {err}",
            path.display()
        );
    }
}

/// Write `source`'s verdicts once they stop arriving. Replacing the task restarts the wait.
fn schedule_write(source: String, cx: &mut App) {
    let task = cx.spawn({
        let source = source.clone();
        async move |cx| {
            cx.background_executor().timer(WRITE_DELAY).await;
            let Some((scope, verdicts)) = cx.update(|cx| {
                let cache = cx.try_global::<Cache>()?;
                Some((
                    cache.scopes.get(&source)?.clone(),
                    cache.terms.get(&source)?.clone(),
                ))
            }) else {
                return;
            };
            cx.background_spawn(async move { write_cache(&source, &scope, &verdicts) })
                .await;
        }
    });
    cx.default_global::<Cache>().writes.insert(source, task);
}

/// One source's share of a run.
struct Job {
    source: Box<dyn AuthoritySource>,
    scope: String,
    columns: Vec<ColumnSnapshot>,
}

/// Everything a run's follow-ups need once the snapshot that started it is gone.
struct Run {
    jobs: Vec<Job>,
    subdelimiter: String,
    config: source::Config,
}

/// Split a cell into the values it actually holds. Archival subject and name columns are commonly
/// multi-valued, so checking the whole cell would reject every row that has two subjects in it.
fn values_in(cell: &str, subdelimiter: &str) -> Vec<String> {
    let parts: Vec<&str> = if subdelimiter.is_empty() {
        vec![cell]
    } else {
        cell.split(subdelimiter).collect()
    };
    parts
        .into_iter()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Check every column that names an authority, and publish what those authorities don't hold.
///
/// Registered as a deferred producer: this goes to the network, so it answers after the edit that
/// triggered it. The store is what the panel and the squiggle read, so a late answer is invisible
/// beyond arriving late.
pub fn check(columns: &[ColumnSnapshot], cx: &mut App) {
    ensure_loaded(cx);
    let subdelimiter = settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx).to_string();
    let config = source::Config::read(cx);

    let mut jobs = Vec::new();
    for source in source::all(&config) {
        let name = source.name();
        let mine: Vec<ColumnSnapshot> = columns
            .iter()
            .filter(|c| c.settings.authority.as_deref() == Some(name))
            .cloned()
            .collect();
        // Nothing points at this authority, or it is configured but unusable: publish nothing,
        // which also clears what it said before. An unusable one says why once, rather than
        // spending a request per term to be refused and reporting the refusals as bad data.
        let unusable = match mine.is_empty() {
            true => None,
            false => source.unavailable(),
        };
        if mine.is_empty() || unusable.is_some() {
            if let Some(why) = unusable {
                log::warn!("{name} is not checking {} column(s): {why}", mine.len());
            }
            cx.default_global::<Cache>().published.remove(name);
            Diagnostics::set(
                &Source::Validator(name.into()),
                DATASET_MAIN,
                Vec::new(),
                cx,
            );
            continue;
        }

        let scope = source.cache_scope();
        let cache = cx.default_global::<Cache>();
        if cache.scopes.get(name) != Some(&scope) {
            cache.scopes.insert(name.into(), scope.clone());
            cache.terms.remove(name);
            cache.failures.remove(name);
            cache.unanswered.remove(name);
            *cache.epochs.entry(name.into()).or_default() += 1;
        }
        jobs.push(Job {
            source,
            scope,
            columns: mine,
        });
    }

    if jobs.is_empty() {
        cx.default_global::<Cache>().last = None;
        return;
    }
    let run = Rc::new(Run {
        jobs,
        subdelimiter,
        config,
    });
    // Everything already cached is published before any request goes out, so a reopened project
    // shows what it knew immediately instead of after a round trip.
    publish(&run, cx);

    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(DEBOUNCE).await;
        cx.update(fetch_next);
    });
    let cache = cx.default_global::<Cache>();
    cache.last = Some(run);
    cache.run = Some(task);
}

/// Republish the newest run against what the cache now knows, and ask for the next batch.
fn resume(cx: &mut App) {
    if let Some(run) = cx
        .try_global::<Cache>()
        .and_then(|cache| cache.last.clone())
    {
        publish(&run, cx);
        fetch_next(cx);
    }
}

/// Claim the newest run's next batch of terms and look them up off the UI thread.
fn fetch_next(cx: &mut App) {
    let Some(run) = cx
        .try_global::<Cache>()
        .filter(|cache| cache.loaded)
        .and_then(|cache| cache.last.clone())
    else {
        return;
    };
    let asked = claim(&run.jobs, &run.subdelimiter, cx);
    if asked.is_empty() {
        return;
    }
    let config = run.config.clone();
    // Detached: a newer run cancels the debounce, never answers a request already paid for.
    cx.spawn(async move |cx| {
        let fetched = cx
            .background_spawn(async move {
                let sources = source::all(&config);
                asked
                    .into_iter()
                    .filter_map(|(name, scope, terms)| {
                        let source = sources.iter().find(|s| s.name() == name)?;
                        let result = lookup_all(source.as_ref(), &terms);
                        Some((name, scope, terms, result))
                    })
                    .collect::<Vec<_>>()
            })
            .await;
        cx.update(|cx| store(fetched, cx));
    })
    .detach();
}

/// Each source's next [`PER_RUN`] distinct terms that nothing is cached, pending, or waiting to
/// retry for, marked pending as they are handed out.
fn claim(jobs: &[Job], subdelimiter: &str, cx: &mut App) -> Vec<(String, String, Vec<String>)> {
    let cache = cx.default_global::<Cache>();
    let now = Instant::now();
    jobs.iter()
        .filter_map(|job| {
            let name = job.source.name();
            if cache.scopes.get(name) != Some(&job.scope) || cache.failures.contains_key(name) {
                return None;
            }
            let cached = cache.terms.get(name);
            let unanswered = cache.unanswered.get(name);
            let pending = cache.pending.entry(name.into()).or_default();
            let mut seen = HashSet::new();
            let ask: Vec<String> = job
                .columns
                .iter()
                .flat_map(|c| c.values.iter())
                .flat_map(|cell| values_in(cell, subdelimiter))
                .filter(|value| {
                    let key = value.to_lowercase();
                    !cached.is_some_and(|cached| cached.contains_key(&key))
                        && unanswered
                            .and_then(|unanswered| unanswered.get(&key))
                            .is_none_or(|retry_at| now >= *retry_at)
                        && !pending.contains(&key)
                        && seen.insert(key)
                })
                .take(PER_RUN)
                .collect();
            pending.extend(ask.iter().map(|term| term.to_lowercase()));
            (!ask.is_empty()).then(|| (name.to_owned(), job.scope.clone(), ask))
        })
        .collect()
}

/// Cache what came back, then publish it against the newest run — which also claims the next
/// batch, until every term has an answer. Only this producer re-runs; the rest of validation has
/// nothing new to say.
fn store(fetched: Vec<(String, String, Vec<String>, LookupBatch)>, cx: &mut App) {
    let mut answered = false;
    for (name, scope, asked, result) in fetched {
        let cache = cx.default_global::<Cache>();
        if let Some(pending) = cache.pending.get_mut(&name) {
            for term in &asked {
                pending.remove(&term.to_lowercase());
            }
        }
        if cache.scopes.get(&name) != Some(&scope) {
            continue;
        }
        let retry_at = Instant::now() + RETRY_AFTER;
        cache
            .unanswered
            .entry(name.clone())
            .or_default()
            .extend(result.unanswered.into_iter().map(|term| (term, retry_at)));
        if let Some(failure) = result.failure {
            log::warn!("{name} checks paused: {failure}");
            cache.failures.insert(name.clone(), failure);
            answered = true;
            *cache.epochs.entry(name.clone()).or_default() += 1;
        }
        if result.verdicts.is_empty() {
            continue;
        }
        answered = true;
        Arc::make_mut(cache.terms.entry(name.clone()).or_default()).extend(result.verdicts);
        *cache.epochs.entry(name.clone()).or_default() += 1;
        schedule_write(name, cx);
    }
    if answered {
        resume(cx);
    }
}

/// Turn what the cache knows into findings, for every job. A term nothing is cached for is not
/// reported — it has not been checked yet, which is not the same as being wrong.
///
/// A column is re-read only when it or the cache changed since it was last published, and a
/// source whose every column is unchanged is not republished at all.
fn publish(run: &Run, cx: &mut App) {
    let cache = cx.default_global::<Cache>();
    let mut published = std::mem::take(&mut cache.published);
    let empty = Verdicts::default();
    let mut batch = Vec::new();
    for job in &run.jobs {
        let name = job.source.name();
        let mut before = published.remove(name).unwrap_or_default();
        let unchanged = before.len() == job.columns.len();
        let cached = cache.terms.get(name).unwrap_or(&empty);
        let failure = cache.failures.get(name);
        let epoch = cache.epochs.get(name).copied().unwrap_or_default();
        let mut now = Published::new();
        let mut items = Vec::new();
        let mut reused = 0;
        for column in &job.columns {
            let found = match before.remove(&column.name) {
                Some((revision, at_epoch, found))
                    if revision == column.revision && at_epoch == epoch =>
                {
                    reused += 1;
                    found
                }
                _ => findings(job, column, cached, failure, &run.subdelimiter),
            };
            items.extend(found.iter().cloned());
            now.insert(column.name.clone(), (column.revision, epoch, found));
        }
        published.insert(name, now);
        if !(unchanged && reused == job.columns.len()) {
            batch.push((Source::Validator(name.into()), items));
        }
    }
    cache.published = published;
    Diagnostics::set_many(DATASET_MAIN, batch, cx);
}

/// One column's findings from one source, addressed.
fn findings(
    job: &Job,
    column: &ColumnSnapshot,
    cached: &HashMap<String, Verdict>,
    failure: Option<&String>,
    subdelimiter: &str,
) -> Vec<Diagnostic> {
    let name = job.source.name();
    let mut found: Vec<_> = column
        .values
        .iter()
        .enumerate()
        .flat_map(|(row, cell)| {
            let mut seen = HashSet::new();
            values_in(cell, subdelimiter)
                .into_iter()
                .filter_map(move |value| {
                    let key = value.to_lowercase();
                    if !seen.insert(key.clone()) {
                        return None;
                    }
                    cached.get(&key).filter(|verdict| !verdict.known)?;
                    let message: SharedString = job.source.rejection(&value).into();
                    Some(ColumnFinding {
                        row: Some(row),
                        severity: Severity::Error,
                        message: message.clone(),
                        group: Some(DiagnosticGroup {
                            key: key.into(),
                            summary: message,
                            subject: Some(value.into()),
                        }),
                    })
                })
        })
        .collect();
    if let Some(failure) = failure {
        let message: SharedString = format!("{name} could not check this column: {failure}").into();
        found.push(ColumnFinding {
            row: None,
            severity: Severity::Warning,
            message: message.clone(),
            group: Some(DiagnosticGroup {
                key: "authority unavailable".into(),
                summary: message,
                subject: None,
            }),
        });
    }
    diagnostics::address(name.into(), column, found)
}

struct LookupBatch {
    verdicts: Vec<(String, Verdict)>,
    /// Lowercased terms the server did not answer, to wait [`RETRY_AFTER`] before asking again.
    unanswered: Vec<String>,
    failure: Option<String>,
}

enum Lookup {
    Verdict(Verdict),
    Retry,
    Refused(String),
}

/// Ask `source` about each term in turn, skipping the rest once the request budget is spent.
fn lookup_all(source: &dyn AuthoritySource, terms: &[String]) -> LookupBatch {
    let mut batch = LookupBatch {
        verdicts: Vec::new(),
        unanswered: Vec::new(),
        failure: None,
    };
    for term in terms {
        if !take_token() {
            log::warn!(
                "{}: request budget spent, so the rest of this pass waits for the next one",
                source.name()
            );
            break;
        }
        match lookup_result(source, term) {
            Lookup::Verdict(verdict) => batch.verdicts.push((term.to_lowercase(), verdict)),
            Lookup::Retry => batch.unanswered.push(term.to_lowercase()),
            Lookup::Refused(failure) => {
                batch.failure = Some(failure);
                break;
            }
        }
    }
    batch
}

/// One term. `None` when the authority could not be reached or did not answer with anything
/// readable — that must never be cached, and must never become a finding, because "the server is
/// down" reported as "this heading is wrong" is the one mistake this crate must not make.
fn lookup_result(source: &dyn AuthoritySource, term: &str) -> Lookup {
    let response = match client().get(source.lookup_url(term)).send() {
        Ok(response) => response,
        Err(err) => {
            log::warn!("{}: could not check “{term}”: {err}", source.name());
            return Lookup::Retry;
        }
    };
    let status = response.status();
    let body = match response.text() {
        Ok(body) => body,
        Err(err) => {
            log::warn!(
                "{}: could not read the answer for “{term}”: {err}",
                source.name()
            );
            return Lookup::Retry;
        }
    };
    interpret(source, term, status, &body)
}

#[cfg(test)]
fn lookup(source: &dyn AuthoritySource, term: &str) -> Option<Verdict> {
    match lookup_result(source, term) {
        Lookup::Verdict(verdict) => Some(verdict),
        Lookup::Retry | Lookup::Refused(_) => None,
    }
}

fn interpret(
    source: &dyn AuthoritySource,
    term: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> Lookup {
    let refusal = source.refusal(body);
    if !status.is_success() {
        let reason = refusal.unwrap_or_else(|| format!("HTTP {status}"));
        if matches!(status.as_u16(), 401 | 403) {
            return Lookup::Refused(reason);
        }
        log::warn!(
            "{}: could not check “{term}”: HTTP {status}: {reason}",
            source.name()
        );
        return Lookup::Retry;
    }
    if let Some(reason) = refusal {
        return Lookup::Refused(reason);
    }

    // An empty list is the authority saying "no such heading" and must be cached as one. Only an
    // answer we could not read at all is dropped.
    let Some(mut labels) = source.labels(body) else {
        return Lookup::Retry;
    };
    let known = labels.iter().any(|label| label.eq_ignore_ascii_case(term));
    // One heading can appear several times, once per authority record that carries it.
    let mut seen = HashSet::new();
    labels.retain(|label| seen.insert(label.to_lowercase()));
    Lookup::Verdict(Verdict {
        known,
        suggestions: if known { Vec::new() } else { labels },
    })
}

/// Shared blocking client. Building one spins up a runtime and a thread, which is not a thing to
/// do per request — the same reason `plugin-host` keeps one.
fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("qrate/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default()
    })
}

/// Spend one request from the window's budget, or refuse.
fn take_token() -> bool {
    static BUCKET: Mutex<Option<(Instant, u32)>> = Mutex::new(None);
    let Ok(mut bucket) = BUCKET.lock() else {
        return false;
    };
    let now = Instant::now();
    let (since, spent) = bucket.unwrap_or((now, 0));
    let (since, spent) = if now.duration_since(since) >= WINDOW {
        (now, 0)
    } else {
        (since, spent)
    };
    if spent >= BUDGET {
        return false;
    }
    *bucket = Some((since, spent + 1));
    true
}

/// Offer the near matches an authority already gave us for this cell.
///
/// Reads the cache only — a menu opens on a click and cannot wait for a round trip. A term whose
/// verdict has not arrived offers nothing, and the menu simply doesn't appear.
fn offer(location: &Location, text: &str, subject: Option<&str>, cx: &App) -> Vec<Fix> {
    let Some(cache) = cx.try_global::<Cache>() else {
        return Vec::new();
    };
    let Some(source) = location
        .column
        .as_deref()
        .and_then(|column| settings::columns::get(column, cx).authority)
    else {
        return Vec::new();
    };
    let Some(verdicts) = cache.terms.get(&source) else {
        return Vec::new();
    };
    let subdelimiter = settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx).to_string();

    values_in(text, &subdelimiter)
        .into_iter()
        .filter(|value| subject.is_none_or(|subject| value == subject))
        .filter_map(|value| Some((value.clone(), verdicts.get(&value.to_lowercase())?)))
        .filter(|(_, verdict)| !verdict.known)
        .flat_map(|(value, verdict)| {
            verdict.suggestions.iter().map(move |suggestion| Fix {
                label: format!("{value} → {suggestion}").into(),
                // The whole cell, with just the rejected value swapped, so a multi-valued
                // cell keeps the values that were fine.
                replacement: text.replace(&value, suggestion).into(),
            })
        })
        .collect()
}

/// "Try again" once a source has been refused, otherwise "Refresh": both ask the server afresh.
fn actions(name: &str, cx: &App) -> Vec<GroupFix> {
    let failed = cx
        .try_global::<Cache>()
        .is_some_and(|cache| cache.failures.contains_key(name));
    let label = if failed {
        format!("Try {name} again")
    } else {
        format!("Refresh {name} results")
    };
    let name = name.to_owned();
    vec![GroupFix::action(label, move |cx| {
        refresh(&name, failed, cx)
    })]
}

/// Forget a refusal, or every verdict, and check again.
fn refresh(name: &str, only_failure: bool, cx: &mut App) {
    ensure_loaded(cx);
    let cache = cx.default_global::<Cache>();
    cache.failures.remove(name);
    cache.unanswered.remove(name);
    if !only_failure {
        cache.terms.remove(name);
        cache.writes.remove(name);
        if let Some(path) = cache_path(name)
            && let Err(err) = std::fs::remove_file(&path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!(
                "could not clear the saved {name} answers at {}, so a restart may show old results: {err}",
                path.display()
            );
        }
    }
    *cache.epochs.entry(name.into()).or_default() += 1;
    resume(cx);
}

/// Wire the authority check in. Reached through [`crate::init`].
pub fn init(cx: &mut App) {
    for name in source::NAMES {
        FixProviders::register(name, offer, cx);
        SourceActions::register(name, actions, cx);
    }
    diagnostics::AsyncValidators::register("authority", check, cx);
}

#[cfg(test)]
mod tests {
    use crate::authority::geonames::GeoNames;
    use crate::authority::{Lookup, Verdict, interpret, values_in};

    #[gpui::test]
    fn a_repeated_value_is_asked_once_and_never_while_pending(cx: &mut gpui::TestAppContext) {
        use crate::authority::{Cache, Job, claim};
        let job = || Job {
            source: Box::new(GeoNames {
                username: "archivist".into(),
            }),
            scope: "GeoNames:archivist".into(),
            columns: vec![diagnostics::ColumnSnapshot {
                name: "Place".into(),
                data_type: "Text".into(),
                settings: Default::default(),
                values: ["Vancouver", "vancouver", "Vancouver; Paris", "Surrey"]
                    .map(Into::into)
                    .into(),
                subdelimiter: ";".into(),
                row_ids: std::sync::Arc::new([]),
                revision: 1,
            }],
        };
        cx.update(|cx| {
            let cache = cx.default_global::<Cache>();
            cache.loaded = true;
            cache
                .scopes
                .insert("GeoNames".into(), "GeoNames:archivist".into());
            std::sync::Arc::make_mut(cache.terms.entry("GeoNames".into()).or_default()).insert(
                "surrey".into(),
                Verdict {
                    known: true,
                    suggestions: Vec::new(),
                },
            );
            let asked = claim(&[job()], ";", cx);
            assert_eq!(asked.len(), 1);
            assert_eq!(asked[0].2, ["Vancouver", "Paris"]);
            assert!(
                claim(&[job()], ";", cx).is_empty(),
                "an overlapping run finds every term already pending"
            );
        });
    }

    #[gpui::test]
    fn trying_a_refused_source_again_keeps_its_answers(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let cache = cx.default_global::<crate::authority::Cache>();
            cache.loaded = true;
            cache
                .failures
                .insert("GeoNames".into(), "credits spent".into());
            std::sync::Arc::make_mut(cache.terms.entry("GeoNames".into()).or_default()).insert(
                "paris".into(),
                Verdict {
                    known: true,
                    suggestions: Vec::new(),
                },
            );
            let offered = crate::authority::actions("GeoNames", cx);
            assert_eq!(offered[0].label, "Try GeoNames again");
            offered[0].apply(cx);
            let cache = cx.global::<crate::authority::Cache>();
            assert!(cache.failures.is_empty());
            assert!(cache.terms["GeoNames"].contains_key("paris"));
            assert_eq!(
                crate::authority::actions("GeoNames", cx)[0].label,
                "Refresh GeoNames results"
            );
        });
    }

    #[test]
    fn a_multi_valued_cell_splits_into_its_values() {
        assert_eq!(
            values_in("Photographs; Bridges ;; ", ";"),
            vec!["Photographs", "Bridges"]
        );
    }

    /// Without a configured subdelimiter the cell is one value — splitting on nothing would turn
    /// every character into a term to look up.
    #[test]
    fn no_subdelimiter_means_the_cell_is_one_value() {
        assert_eq!(
            values_in("World War, 1939-1945", ""),
            vec!["World War, 1939-1945"]
        );
        assert!(values_in("   ", ";").is_empty());
    }

    #[test]
    fn a_verdict_records_both_the_answer_and_what_to_offer() {
        let verdict = Verdict {
            known: false,
            suggestions: vec!["Photographs".into()],
        };
        assert!(!verdict.known);
        assert_eq!(verdict.suggestions.len(), 1);
    }

    #[test]
    fn a_geonames_refusal_stops_the_source_and_keeps_its_message() {
        let source = GeoNames {
            username: "invalid".into(),
        };
        let refused = r#"{"status":{"message":"user does not exist","value":10}}"#;
        assert!(matches!(
            interpret(
                &source,
                "Vancouver",
                reqwest::StatusCode::UNAUTHORIZED,
                refused
            ),
            Lookup::Refused(message) if message == "user does not exist"
        ));
    }

    #[test]
    fn suggestions_are_deduplicated_without_changing_rank_order() {
        let source = GeoNames {
            username: "valid".into(),
        };
        let body = r#"{"geonames":[
            {"name":"Vancouver Island"},
            {"name":"Vancouver"},
            {"name":"Vancouver Island"}
        ]}"#;
        let Lookup::Verdict(verdict) =
            interpret(&source, "Vancouvr", reqwest::StatusCode::OK, body)
        else {
            panic!("the response must produce a verdict");
        };
        assert_eq!(
            verdict.suggestions,
            ["Vancouver Island", "Vancouver"],
            "the API's best-first order is preserved"
        );
    }

    /// The budget is a backstop against a runaway loop, so what matters is that it stops rather
    /// than exactly where — a run that spends it all still gets [`super::BUDGET`] requests.
    #[test]
    fn the_request_budget_runs_out() {
        let spent = (0..super::BUDGET + 10)
            .filter(|_| super::take_token())
            .count();
        assert_eq!(spent, super::BUDGET as usize);
        assert!(!super::take_token(), "and stays out inside the window");
    }
}

/// Hits the real services. Ignored so CI never depends on somebody's network, and run with
/// `cargo test -p checks -- --ignored` when changing how an answer is read.
///
/// GeoNames is absent on purpose: every call needs an account name, so a test that hit it would
/// either carry someone's credential or fail for everyone who has not set one.
#[cfg(test)]
mod network {
    use crate::authority::lcsh::Lcsh;
    use crate::authority::lookup;
    use crate::authority::wikidata::Wikidata;

    #[test]
    #[ignore = "hits id.loc.gov"]
    fn lcsh_knows_a_real_heading_and_not_an_invented_one() {
        let real = lookup(&Lcsh, "Photographs").expect("id.loc.gov answered");
        assert!(real.known, "Photographs is a subject heading");

        let invented = lookup(&Lcsh, "Zzzq not a heading").expect("id.loc.gov answered");
        assert!(!invented.known);
    }

    /// `suggest2` is left-anchored and does no fuzzy matching, so what it offers is completions
    /// of a partial heading — not corrections of a typo. Worth pinning: the fix menu is only ever
    /// as good as this, and a change here is a change to what the menu can do.
    #[test]
    #[ignore = "hits id.loc.gov"]
    fn a_partial_heading_comes_back_with_completions() {
        let partial = lookup(&Lcsh, "Photograph albu").expect("id.loc.gov answered");
        assert!(!partial.known);
        assert!(
            partial
                .suggestions
                .iter()
                .any(|s| s.starts_with("Photograph album")),
            "a partial heading offers what completes it, got {:?}",
            partial.suggestions
        );

        // A typo is not a prefix of anything, so nothing is offered — it is still correctly
        // reported as not a heading, which is the part that matters.
        let typo = lookup(&Lcsh, "Photograpy").expect("id.loc.gov answered");
        assert!(!typo.known);
        assert!(typo.suggestions.is_empty());
    }

    #[test]
    #[ignore = "hits wikidata.org"]
    fn wikidata_knows_a_real_entity_and_not_an_invented_one() {
        let real = lookup(&Wikidata, "Vancouver").expect("wikidata answered");
        assert!(real.known, "Vancouver has an entry");

        let invented = lookup(&Wikidata, "Zzzqnotathing").expect("wikidata answered");
        assert!(!invented.known);
        assert!(invented.suggestions.is_empty(), "nothing matched at all");
    }

    /// Unlike LCSH, `wbsearchentities` matches inside a label and across aliases, so a partial
    /// name does come back with somewhere to go. This is why Wikidata is worth having alongside
    /// a subject-heading list rather than instead of one.
    #[test]
    #[ignore = "hits wikidata.org"]
    fn wikidata_offers_something_for_a_partial_name() {
        let partial = lookup(&Wikidata, "Vancouver Isl").expect("wikidata answered");
        assert!(
            !partial.suggestions.is_empty(),
            "a partial name offers matches, got {:?}",
            partial.suggestions
        );
    }
}

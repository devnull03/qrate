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

mod lcsh;
pub mod source;

pub use lcsh::NAME as LCSH;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use diagnostics::{
    ColumnSnapshot, DATASET_MAIN, Diagnostics, Fix, FixProviders, Location, Severity, Source,
};
use gpui::{App, AppContext as _, Global, SharedString, Task};

use source::AuthoritySource;

/// Wait for typing to stop before calling out. Longer than the plugin host's debounce because
/// this leaves the machine.
const DEBOUNCE: Duration = Duration::from_millis(400);

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

/// What an authority said about one term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Whether the authority holds this exact heading.
    pub known: bool,
    /// Near matches, best first — what to offer when it doesn't.
    pub suggestions: Vec<String>,
}

/// Every verdict this machine has been told, by source name then lowercased term.
///
/// Lives beside the other caches in the data dir, never in the `.qrate` file: a verdict is about
/// what a server said, and a project file is a thing people commit and hand to each other.
#[derive(Default)]
struct Cache {
    terms: HashMap<String, HashMap<String, Verdict>>,
    loaded: bool,
    /// Keeps the in-flight run alive; dropping it cancels, which is what a newer run wants.
    run: Option<Task<()>>,
}

impl Global for Cache {}

fn cache_path(source: &str) -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("authority").join(format!("{source}.json")))
}

/// Read every source's cache off disk once per launch. An unreadable file reads as nothing
/// cached, which is always safe: a cache that cannot be read is one that gets refilled.
fn ensure_loaded(cx: &mut App) {
    if cx.default_global::<Cache>().loaded {
        return;
    }
    let mut terms = HashMap::new();
    for name in source::names() {
        let Some(path) = cache_path(name) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match serde_json::from_str::<HashMap<String, (bool, Vec<String>)>>(&text) {
            Ok(stored) => {
                terms.insert(
                    name.to_string(),
                    stored
                        .into_iter()
                        .map(|(term, (known, suggestions))| (term, Verdict { known, suggestions }))
                        .collect(),
                );
            }
            Err(err) => log::warn!("discarding the unreadable {name} cache: {err}"),
        }
    }
    let cache = cx.default_global::<Cache>();
    cache.terms = terms;
    cache.loaded = true;
}

fn write_cache(source: &str, verdicts: &HashMap<String, Verdict>) {
    let Some(path) = cache_path(source) else {
        return;
    };
    let stored: HashMap<&String, (bool, &Vec<String>)> = verdicts
        .iter()
        .map(|(term, v)| (term, (v.known, &v.suggestions)))
        .collect();
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

/// One source's share of a run.
struct Job {
    source: Box<dyn AuthoritySource>,
    columns: Vec<ColumnSnapshot>,
    /// Terms nothing is cached for, already capped at [`PER_RUN`].
    ask: Vec<String>,
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

    let mut jobs = Vec::new();
    for source in source::all() {
        let name = source.name();
        let mine: Vec<ColumnSnapshot> = columns
            .iter()
            .filter(|c| c.settings.authority.as_deref() == Some(name))
            .cloned()
            .collect();
        // Nothing points at this authority: publish nothing, which also clears what it said
        // before a column stopped naming it.
        if mine.is_empty() {
            Diagnostics::set(
                &Source::Validator(name.into()),
                DATASET_MAIN,
                Vec::new(),
                cx,
            );
            continue;
        }

        let cached = cx
            .default_global::<Cache>()
            .terms
            .entry(name.into())
            .or_default();
        let mut seen = HashSet::new();
        let ask: Vec<String> = mine
            .iter()
            .flat_map(|c| c.values.iter())
            .flat_map(|cell| values_in(cell, &subdelimiter))
            .filter(|value| seen.insert(value.to_lowercase()))
            .filter(|value| !cached.contains_key(&value.to_lowercase()))
            .take(PER_RUN)
            .collect();

        jobs.push(Job {
            source,
            columns: mine,
            ask,
        });
    }

    if jobs.is_empty() {
        return;
    }

    let task = cx.spawn(async move |cx| {
        // Everything already cached is published before the first request goes out, so a reopened
        // project shows what it knew immediately instead of after a round trip.
        cx.update(|cx| publish(&jobs, &subdelimiter, cx));

        let asked: Vec<(String, Vec<String>)> = jobs
            .iter()
            .filter(|job| !job.ask.is_empty())
            .map(|job| (job.source.name().to_string(), job.ask.clone()))
            .collect();
        if asked.is_empty() {
            return;
        }

        cx.background_executor().timer(DEBOUNCE).await;
        let fetched = cx
            .background_spawn(async move {
                let sources = source::all();
                asked
                    .into_iter()
                    .filter_map(|(name, terms)| {
                        let source = sources.iter().find(|s| s.name() == name)?;
                        Some((name.clone(), lookup_all(source.as_ref(), &terms)))
                    })
                    .collect::<Vec<_>>()
            })
            .await;

        cx.update(|cx| {
            for (name, verdicts) in fetched {
                if verdicts.is_empty() {
                    continue;
                }
                let cache = cx.default_global::<Cache>();
                let for_source = cache.terms.entry(name.clone()).or_default();
                for (term, verdict) in verdicts {
                    for_source.insert(term, verdict);
                }
                let snapshot = for_source.clone();
                cx.background_executor()
                    .spawn(async move { write_cache(&name, &snapshot) })
                    .detach();
            }
            publish(&jobs, &subdelimiter, cx);
        });
    });
    cx.default_global::<Cache>().run = Some(task);
}

/// Turn what the cache knows into findings, for every job. A term nothing is cached for is not
/// reported — it has not been checked yet, which is not the same as being wrong.
fn publish(jobs: &[Job], subdelimiter: &str, cx: &mut App) {
    for job in jobs {
        let name = job.source.name();
        let empty = HashMap::new();
        let cached = cx
            .try_global::<Cache>()
            .and_then(|c| c.terms.get(name))
            .unwrap_or(&empty);

        let items: Vec<_> = job
            .columns
            .iter()
            .flat_map(|column| {
                let found: Vec<(usize, Severity, SharedString)> = column
                    .values
                    .iter()
                    .enumerate()
                    .filter_map(|(row, cell)| {
                        let rejected: Vec<String> = values_in(cell, subdelimiter)
                            .into_iter()
                            .filter(|value| {
                                cached
                                    .get(&value.to_lowercase())
                                    .is_some_and(|verdict| !verdict.known)
                            })
                            .collect();
                        (!rejected.is_empty()).then(|| {
                            (
                                row,
                                Severity::Error,
                                rejected
                                    .iter()
                                    .map(|value| job.source.rejection(value))
                                    .collect::<Vec<_>>()
                                    .join("; ")
                                    .into(),
                            )
                        })
                    })
                    .collect();
                diagnostics::address(name.into(), column, found)
            })
            .collect();
        Diagnostics::set(&Source::Validator(name.into()), DATASET_MAIN, items, cx);
    }
}

/// Ask `source` about each term in turn, skipping the rest once the request budget is spent.
fn lookup_all(source: &dyn AuthoritySource, terms: &[String]) -> Vec<(String, Verdict)> {
    terms
        .iter()
        .map_while(|term| {
            if !take_token() {
                log::warn!(
                    "{}: request budget spent, so the rest of this pass waits for the next one",
                    source.name()
                );
                return None;
            }
            Some(lookup(source, term).map(|verdict| (term.to_lowercase(), verdict)))
        })
        .flatten()
        .collect()
}

/// One term. `None` when the authority could not be reached or did not answer with anything
/// readable — that must never be cached, and must never become a finding, because "the server is
/// down" reported as "this heading is wrong" is the one mistake this crate must not make.
fn lookup(source: &dyn AuthoritySource, term: &str) -> Option<Verdict> {
    let response = client()
        .get(source.lookup_url(term))
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(reqwest::blocking::Response::text);
    let body = match response {
        Ok(body) => body,
        Err(err) => {
            log::warn!("{}: could not check “{term}”: {err}", source.name());
            return None;
        }
    };

    // An empty list is the authority saying "no such heading" and must be cached as one. Only an
    // answer we could not read at all is dropped.
    let mut labels = source.labels(&body)?;
    let known = labels.iter().any(|label| label.eq_ignore_ascii_case(term));
    // One heading can appear several times, once per authority record that carries it.
    labels.dedup();
    Some(Verdict {
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
fn offer(_: &Location, text: &str, cx: &App) -> Vec<Fix> {
    let Some(cache) = cx.try_global::<Cache>() else {
        return Vec::new();
    };
    let subdelimiter = settings::effective_text(settings::FILTER_SUBDELIMITER_KEY, cx).to_string();

    cache
        .terms
        .values()
        .flat_map(|verdicts| {
            values_in(text, &subdelimiter)
                .into_iter()
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
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Wire the authority check in. Reached through [`crate::init`].
pub fn init(cx: &mut App) {
    for name in source::names() {
        FixProviders::register(name, offer, cx);
    }
    diagnostics::AsyncValidators::register("authority", check, cx);
}

#[cfg(test)]
mod tests {
    use crate::authority::{Verdict, values_in};

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

/// Hits the real `id.loc.gov`. Ignored so CI never depends on somebody's network, and run with
/// `cargo test -p authority -- --ignored` when changing how an answer is read.
#[cfg(test)]
mod network {
    use crate::authority::lcsh::Lcsh;
    use crate::authority::lookup;

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
}

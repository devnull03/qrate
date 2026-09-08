# Spellcheck upgrades

## Purpose

Reduce spelling false positives in multilingual catalogue data, distinguish capitalization from
spelling, and help cataloguers reconcile alternate spellings of a person's name.

## Current system

- `SpellCheck` owns one `spellbook::Dictionary` behind an `RwLock`. The dictionary is selected and
  parsed at startup, and changing it requires a restart.
- Canadian and American English are embedded. Other Hunspell dictionaries can be downloaded, but
  only the selected dictionary is loaded.
- The synchronous `ColumnValidator` API already supplies a whole column and its project settings.
  It is sufficient for both per-value dictionary selection and column-wide name comparison.
- Spelling corrections use a dedicated `SpellActions` menu. The newer `FixProviders` registry can
  offer capitalization and name-variant replacements without putting machine-readable data in a
  diagnostic message.
- The Problems panel can already filter independent diagnostic sources. Value variants should be a
  separate source rather than pretending to be spelling errors.

## Research findings

### Language identification

Three Rust language detectors were considered:

| Option | Result |
|---|---|
| `whatlang` | Small (81 KiB crate), offline, 70 languages, and exposes confidence. A probe with catalogue-length values found its built-in `> 0.9` reliability rule too conservative: even clear English, Spanish, and German sentences often returned unreliable, while a French sentence was misidentified. Restricting it to four candidate languages did not make short titles reliable. |
| `whichlang` | Small and fast, but supports only 16 languages and does not expose an ambiguity result suitable for the conservative rule required here. |
| `lingua` | Designed for short and mixed-language text and exposes a minimum relative distance. Its high-accuracy data is shipped as one model crate per language; the English model alone is about 2.6 MB compressed. Compiling the full downloadable dictionary catalogue into qrate would add dozens of models, while low-accuracy mode explicitly loses accuracy below 120 characters. |

The recommended detector is therefore the dictionaries qrate already downloads. Score each value
against the installed base-language dictionaries, select a dictionary only when one score clearly
wins, and report nothing when the evidence is weak or tied. This keeps language support aligned
with the actual spell-checking capability and adds no second language catalogue.

Regional variants must be one candidate during identification. In particular, `en` and `en-CA`
cannot compete as if they were different languages. The configured variant remains the regional
choice when the detected base language is English.

The exact threshold must be calibrated against a checked-in fixture, not selected from one example.
The initial rule to test is:

1. ignore data-shaped and candidate-name tokens before scoring;
2. require at least two informative tokens when more than one base language is installed;
3. require the winning dictionary to accept at least 60% of those tokens; and
4. require a lead of at least 20 percentage points over the runner-up.

With only one base language available, retain today's behavior. With several languages installed,
a short, unknown, or tied value produces no spelling finding. One misspelling inside otherwise clear
prose can still be found because the surrounding known words identify the dictionary.

### Capitalization

`spellbook` already preserves Hunspell case rules. Its checker can also ask whether a lowercase word
would be valid in title or upper case. Token classification should use that information in this
order:

1. exact dictionary form: clean;
2. another canonical case is known: capitalization finding and replacement;
3. unknown title-cased token or title-cased sequence: candidate proper noun, so suppress it;
4. otherwise: misspelling.

This ordering improves the existing “Ignore names” rule. Today capitalized tokens are discarded
before the dictionary is consulted, so a known word with the wrong case cannot be distinguished
from an unknown name. All-uppercase catalogue titles remain valid when Hunspell accepts them.

### Value variants

Name reconciliation is record linkage, not spell checking. Published comparisons of name-matching
methods favor token-aware comparisons and Jaro-Winkler-style similarity over raw whole-string
Levenshtein distance. The requested edit-distance behavior can remain explainable with a smaller
rule set:

- normalize punctuation and whitespace, apply Unicode NFKC and full case folding, and preserve the
  displayed value;
- recognize comma order and compare both direct and family-name-first token orders;
- treat equal token multisets in a different order as a strong candidate;
- for spelling variants, require equal token counts, at least one stable exact token, and a bounded
  normalized Damerau-Levenshtein distance on the remaining aligned token;
- use accent-insensitive text only to generate candidates, never to silently declare two names
  equal; and
- compare distinct values, then attach findings back to every affected row.

This deliberately does not attempt transliteration, nickname inference, or identity resolution.
“Bill” and “William” need authority data, not a string-distance threshold.

Variant review should be off by default and enabled per column. The generic rules apply to names,
organizations, places, subjects, collection titles, and controlled labels without claiming that
similar text identifies the same entity. Person names additionally benefit from order-aware
comparison. Places and subjects should prefer their configured authority validator when one is
available. This is an additive `ColumnSettings` preference, not a replacement profile format. Each
finding names both displayed forms and the reason they are close. Its fixes offer the other current
displayed form for that one cell; accepting one uses the existing undoable whole-cell edit path.
There is no automatic canonical choice and no bulk rewrite.

## Implementation plan

### 1. Establish behavior with fixtures

- Add a compact multilingual fixture covering long prose, short titles, one-word values, shared
  words, mixed-language rows, same-cell ambiguity, and a typo surrounded by identifiable prose.
- Add name pairs and non-pairs covering comma order, token reversal, diacritics, apostrophes,
  hyphens, initials, unrelated common names, and repeated identical values.
- Tune the language and edit-distance thresholds against the fixture. Record every threshold beside
  the rule and test its boundary.

### 2. Load a dictionary set

- Replace the single dictionary field with a map keyed by catalogue code.
- Load the configured regional dictionary plus every complete downloaded dictionary off the UI
  thread. Group codes by base language for detection and keep the configured regional variant as
  the tie-breaker within a group.
- Replay the custom word list into every loaded dictionary.
- Keep incomplete or unreadable downloads out of the candidate set. Never fall back to English
  after another language was confidently identified but its dictionary cannot be loaded.
- Update settings text to explain that downloaded languages participate automatically. Keep
  download/remove behavior and the configured regional preference.

### 3. Separate detection from word classification

- Introduce pure functions for tokenization, per-value language scoring, and token classification.
- Use Unicode-aware boundaries and accept straight and curly apostrophes plus name hyphens.
- Return an explicit outcome such as `Selected`, `Ambiguous`, or `InsufficientEvidence`; do not
  encode “unknown” as the default language.
- Share the selected dictionary and classification path between validation and the right-click
  suggestion menu so they cannot disagree about which words are misspelled.

### 4. Publish spelling and capitalization separately

- Keep spelling at `Warning`.
- Publish capitalization as a quieter `Note` under a distinct `capitalization` source, with wording
  such as `capitalization: “alice” is known as “Alice”`.
- Register capitalization replacements through `FixProviders`.
- Preserve “Add to dictionary” only for actual misspellings.
- Revisit the “Ignore names” switch after the classifier lands. Prefer removing the switch if the
  ordered classifier fully subsumes it; do not retain two controls for the same rule.

### 5. Add opt-in value-variant review

- Add a backward-compatible, default-off `variant_review` field to `ColumnSettings`, the Columns
  settings picker, and the `column_config.csv` import/export contract.
- Implement a separate `value variants` validator and a small shared candidate store used by its fix
  provider.
- Compare unique normalized values within enabled columns. Start with blocked comparisons by token
  count and stable token; benchmark a 2,000-row worst-case fixture before deciding whether this must
  move to a background validator.
- Emit `Note` findings only above the calibrated threshold. Include the reason: reordered tokens,
  punctuation/diacritic variation, or one close token.
- Offer each observed displayed form as an individual whole-cell fix. Revalidation clears resolved
  findings; undo restores both the value and finding.

### 6. Integrate and document

- Update Spelling and Columns settings descriptions, `docs/diagnostics.md`, and the sample column
  configuration.
- Rebase the Problems panel portion on `gpui-kit-migration` if it lands first. The current overlap is
  confined to `crates/diagnostics/src/panel.rs`.
- Run focused unit tests first, then `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings -A dead_code`, and `cargo fmt --all --check`.
- Manually verify dictionary download/removal, startup loading, ambiguous short values, both
  right-click surfaces, undo, and restart persistence.

## Diagnostic contract

| Source | Default severity | Example | Automatic change |
|---|---|---|---|
| `spell` | Warning | `misspelled: recieve` | Never |
| `capitalization` | Note | `capitalization: “alice” is known as “Alice”` | Never |
| `value variants` | Note | `“Varda, Agnès” may be a reordered form of “Agnès Varda”` | Never |

Severity overrides continue to use the existing per-source column setting.

## Scope

1. Select applicable dictionaries per cell or value using language identification with a documented
   confidence threshold.
2. Treat low-confidence and ambiguous language values conservatively instead of flagging them as
   spelling errors.
3. Publish known-word case mismatches as a separate capitalization warning.
4. Suppress title-cased candidate proper nouns from ordinary spelling findings.
5. Identify likely person-name variants with normalized, order-aware token comparison and edit
   distance.
6. Offer an opt-in review flow in which the cataloguer chooses the displayed canonical form.

## Non-goals

- Automatically changing names or silently merging records.
- Treating capitalization warnings as misspellings.
- Replacing project column descriptions/configuration with a new profile format.

## Dependencies and merge notes

Dictionary and normalization logic can proceed independently. The diagnostic review UI will touch
the Problems panel and must be rebased on `gpui-kit-migration` before merge if that migration lands
first.

## Definition of done

- Tests cover mixed-language text, ambiguous language, title-cased proper nouns, and case-only
  mistakes.
- Name suggestions are explainable, opt-in, and never apply without the user's choice.
- Findings have distinct wording and severity for spelling, capitalization, and name variants.
- An installed language that cannot be selected confidently causes no finding rather than an
  English false positive.
- Validation and correction menus classify the same token with the same dictionary.
- A 2,000-row name column stays within the validation latency budget established by the benchmark.

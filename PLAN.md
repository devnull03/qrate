# Spellcheck upgrades

## Purpose

Reduce spelling false positives in multilingual catalogue data, distinguish capitalization from
spelling, and help cataloguers reconcile alternate spellings of a person's name.

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

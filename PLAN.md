# Bulk cleanup

## Purpose

Provide reviewed bulk transformations for catalogue values while preserving scope visibility and a
single undo step for each accepted operation.

## Scope

1. Preview selected rows and columns before and after an operation.
2. Support whitespace cleanup, Unicode normalization, case normalization, delimiter splitting or
   joining, regex replacement, and duplicate detection/removal.
   - Use NFC for canonical Unicode normalization.
   - Offer NFKC as a separate compatibility-normalization operation because it can change meaning.
   - Use Unicode default casing for lower-, upper-, and title-case operations. Do not use the host
     locale.
3. Report skipped and failed cells without silently changing them.
4. Apply an accepted transformation as one complete undoable batch.

## Non-goals

- Unreviewed automatic cleanup.
- Spreadsheet formulas or presentation formatting.
- Replacing the current find-and-replace flow where it remains the simpler operation.

## Dependencies and merge notes

Do not implement the preview/confirmation UI until PR #118 has landed. That PR changes the table
panel and delegate surfaces this work would otherwise conflict with. The transformation semantics
and fixtures may be designed before then.

## Definition of done

- Every operation shows the affected-cell preview and scope before it writes.
- Cancel makes no changes; apply is one undo step.
- Tests cover filtered rows, selection scope, no-op values, and failures.

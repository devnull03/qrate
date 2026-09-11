# Diagnostics

qrate checks project data continuously and lists what it finds in the Problems panel, in
the right dock. A finding on a cell also shows as a small marker on that cell in the grid.

## What qrate checks

- **Spelling**, in over 60 languages. Downloaded dictionaries participate in automatic
  per-value language selection; short or ambiguous values are left alone instead of being
  checked as the wrong language. If only English dictionaries are installed, qrate also skips
  reliably identified non-English text and common non-English apostrophe elisions.
- **Capitalization**, when a dictionary knows the word but requires a different case.
  Capitalization findings are warnings. The Notes tab is reserved for user notes.
- **Value variants**, for columns where you opt into reviewing inconsistent displayed forms.
  This catches punctuation, diacritic, word-order, and close-spelling differences in names,
  organizations, places, subjects, titles, and other labels. Similarity is a review hint, not
  a claim that two values identify the same entity.
  Right-click a column header and select **Review value variants** to enable this check.
  Select the checked command again to disable it.
- **Date formats**, so a malformed or ambiguous date is caught before export.
- **File links**, so a row whose linked file cannot be found is reported instead of
  silently showing a blank preview. See [Files and photos](files-and-photos.md).
  Only a column set to the `Filename` type is checked this way.
- **Headings against LCSH, GeoNames, and Wikidata**, so a subject or place heading can be
  checked against those authorities.
- **Plugin validators**, if the project has plugins that add their own checks. See
  [Plugins](plugins/index.md).

Each of these runs as an independent validator. A validator reports its complete set of
findings each time it runs, and that set replaces what it reported last time.

## The Problems panel

Repeated spelling, capitalization, and value-variant findings appear in collapsed groups.
Each group shows an occurrence count. Expand the group to see its locations.
Expanded value-variant groups show the value at each location instead of repeating the summary.
The expansion button supports keyboard activation. Click an occurrence to select its cell, row, or column.
A group header does not select an arbitrary location. Its context menu can resolve all occurrences.

Findings can describe four scopes:

| Scope | Location label | Navigation |
|---|---|---|
| Cell | `Row N · Column` | Select the cell |
| Row | `Row N` | Select the row |
| Column | Column name | Select the column |
| Dataset | Dataset name | No cell selection |

The severity tabs count diagnostic occurrences, not collapsed groups or unique cells.
Two distinct misspelled words in one cell count as two occurrences.
Repeated copies of the same word in one cell count once.
A cell can also belong to more than one value-variant pair.

The source filter changes both the visible findings and the tab counts.
It is a multi-select filter. Uncheck one or more diagnostic sources to hide them.
User notes do not appear as a source because the Notes tab already selects them.
The All, Errors, and Warnings tabs show computed findings only. The Notes tab shows user notes only.
The severity filter changes the visible findings, but each tab keeps its own total.
Notes and validators without group metadata remain separate entries.

### Producer contracts

The diagnostics store keeps each finding at its exact location.
The panel groups findings only when a producer supplies `DiagnosticGroup` metadata.
Group identity includes the dataset, source, severity, and producer key. The summary is display text, not identity.

Column validators return `ColumnFinding`. A row index describes a cell. An absent row index describes the whole column.
Row-wide and dataset-wide producers publish addressed diagnostics directly.
Existing plugin scripts keep their current row-based output format.

Spelling keys include the selected dictionary and observed token.
Capitalization keys also include the proposed spelling.
Value-variant keys include the column and an unordered pair of exact displayed values.
Pair review does not infer that a third similar value identifies the same entity.

## Applying a fix

Some findings offer a suggested correction. Right-click the cell, or open its **Fixes**
menu, and pick a suggestion to apply it. Applying a fix replaces the cell's whole value, so
you always know exactly what you are accepting.

Value-variant fixes offer forms that already occur in the same column. qrate never chooses a
canonical form, merges records, or changes every matching row automatically.

Right-click a spelling or capitalization group to apply one correction to all its occurrences.
The group resolver applies the changes as one undo step.

Right-click a value-variant group to replace all occurrences with either displayed form.
You can also mark the exact pair as distinct for that column. qrate saves this choice with the project.
Variant groups stay pair-based. Similarity between two pairs does not create a transitive entity cluster.

qrate uses the configured subdelimiter to read multiple logical values in one cell.
Validators borrow these values as text slices, so splitting does not allocate a list for each cell.
Value-variant fixes preserve the other logical values and replace the complete cell through the normal edit path.

A fix offered against one version of a cell's text does not apply once that text has
changed. This stops a stale suggestion from silently overwriting a newer edit.

Expanded cell occurrences offer the same spelling and fix menus as the grid.
Row, column, dataset, and group entries do not offer cell fixes.
Each accepted fix uses the existing undoable cell edit path. Validation then refreshes the groups.

## Design credit

qrate's value-clustering workflow is inspired by OpenRefine's
[Cluster and edit](https://openrefine.org/docs/manual/cellediting#cluster-and-edit) feature and
[clustering methods](https://openrefine.org/docs/technical-reference/clustering-in-depth).
qrate uses an independent implementation for its diagnostics and cataloging workflow.
This acknowledgment does not imply OpenRefine's endorsement.

OpenRefine publishes its source under BSD-3-Clause and its documentation under CC BY 4.0.
This implementation does not copy OpenRefine source, documentation text, or UI assets.
Any future adaptation must retain the applicable copyright, license, and attribution notices.

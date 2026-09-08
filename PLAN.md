# Grouped diagnostics and reusable value clustering

## Purpose

This branch will make repeated findings easier to review. It will also move value clustering out
of the spell checker.

A spelling mistake can occur in 100 cells. The Problems panel now shows 100 independent rows. The
panel should show one problem group with an occurrence count. The user can expand the group and
jump to each affected location.

Value variants have the same need. A pair such as `Agnès Varda` and `Varda, Agnès` is one review
question, not one unrelated note per cell.

This branch will keep the first review UI simple. A separate future task will cover a full cluster
review workspace with canonical value selection and bulk merge controls.

## Completed baseline

The branch already contains these spell-check upgrades:

- qrate loads all installed dictionaries.
- Each value selects a dictionary only when the evidence is strong enough.
- Ambiguous and short unknown values do not default to English.
- Capitalization uses a separate `capitalization` diagnostic source.
- Candidate proper names do not become ordinary spelling warnings.
- The opt-in `value variants` validator finds formatting, token order, diacritic, and close-spelling
  differences.
- Existing fixes replace one whole cell through the undoable table edit path.

These behaviors stay in place during the diagnostic and crate changes.

## Root cause

The diagnostic model stores one `Diagnostic` for one exact `Location`. This is correct for cell
markers, navigation, notes, fixes, and source replacement.

The display model uses the same shape. `ProblemsPanel` converts each `Diagnostic` directly into one
visible row. It has no stable key that says several diagnostics describe the same problem.

The validator boundary also limits built-in synchronous validators. `ColumnValidator::validate`
returns `(row, severity, message)`. It can report a cell in the current column, but it cannot report
a column-wide finding.

`Location` itself already supports all required scopes:

- dataset: no row and no column;
- column: a column and no row;
- row: a row and no column;
- cell: both a row and a column.

The branch does not need another location representation. It needs a clear scope view, a richer
validator result, and a separate grouped panel projection.

## Design rules

1. Keep each diagnostic attached to its exact location.
2. Let a producer define group identity. Do not parse a human message.
3. Group only diagnostics from the same dataset, source, severity, and producer key.
4. Keep diagnostics without group metadata as independent rows.
5. Keep group aggregation in the diagnostics crate.
6. Keep matching algorithms out of the diagnostics crate.
7. Never treat text similarity as proof of entity identity.
8. Do not add bulk replacement in the first grouped-panel change.
9. Preserve source replacement and validator severity overrides.
10. Preserve one-cell fixes on expanded occurrence rows.

## Proposed diagnostic contracts

### Location scope

Add a derived scope method instead of replacing `Location`:

```rust
pub enum Scope<'a> {
    Dataset,
    Column(&'a str),
    Row(usize),
    Cell { row: usize, column: &'a str },
}

impl Location {
    pub fn scope(&self) -> Scope<'_>;
}
```

This method gives the panel one exhaustive scope match. It does not migrate stored notes or change
the existing table lookup keys.

Add constructors for new call sites:

```rust
Location::dataset(dataset)
Location::column(dataset, column)
Location::row(dataset, row, row_id)
Location::cell(dataset, row, row_id, column)
```

Existing struct literals can migrate when their files change. This branch does not need a
repository-wide mechanical rewrite.

### Diagnostic group

Add optional producer metadata to `Diagnostic`:

```rust
pub struct DiagnosticGroup {
    pub key: SharedString,
    pub summary: SharedString,
}

pub struct Diagnostic {
    pub location: Location,
    pub severity: Severity,
    pub source: Source,
    pub message: SharedString,
    pub group: Option<DiagnosticGroup>,
    pub filed: Option<Filed>,
}
```

`key` is machine-readable and stable within one source. `summary` is the text for the collapsed
group row.

Examples:

| Source | Group key | Summary |
|---|---|---|
| `spell` | `en:recieve` | `“recieve” is misspelled` |
| `capitalization` | `alice→Alice` | `Use “Alice” instead of “alice”` |
| `value variants` | normalized unordered pair | `“Agnès Varda” and “Varda, Agnès” may be variants` |

The panel groups by `(dataset, source, severity, group.key)`. This prevents an override from hiding
the severity of some occurrences inside a different tab.

Authored notes remain ungrouped. Existing validators remain ungrouped until they supply a group.

### Validator finding

Replace the tuple return value with a named type:

```rust
pub struct ColumnFinding {
    pub row: Option<usize>,
    pub severity: Severity,
    pub message: SharedString,
    pub group: Option<DiagnosticGroup>,
}
```

`row: Some(row)` reports a cell. `row: None` reports the current column.

The named type makes later fields possible without another tuple migration. It also makes the scope
of each result visible at its construction site.

The plugin script contract does not change in this branch. `plugin-host` will translate its current
row findings into ungrouped `ColumnFinding` values.

Row-wide and dataset-wide producers continue to publish addressed `Diagnostic` values. A
column-wise validator must not claim a row-wide result because it only owns one column snapshot.

## Grouped panel projection

Keep `Diagnostics::items` as the source of truth. Build a derived panel model during
`ProblemsPanel::refresh`.

```rust
enum ProblemEntry {
    Group {
        id: ProblemGroupId,
        severity: Severity,
        summary: SharedString,
        source: SharedString,
        occurrences: Vec<Occurrence>,
    },
    Occurrence(Occurrence),
}
```

The implementation can use a flat visible list after it applies expansion state. This keeps
`uniform_list` and its fixed row height.

### Collapsed group row

A collapsed row will show:

- the severity icon;
- the group summary;
- the source;
- an `N occurrences` badge;
- an expand control.

Clicking the expand control will show the occurrence rows. The group row will not choose an
arbitrary location or apply a fix.

### Occurrence row

An occurrence row will keep the current behavior:

- show `Row N · Column`, row, column, or dataset scope;
- jump to the location on click;
- offer the current one-cell spelling and fix menus;
- read the current cell text when the menu opens.

Column, row, and dataset occurrences have no cell text. Their context menus will omit cell fixes.

### Counts and filters

Severity tabs will keep occurrence totals. A tab that says `Warnings (100)` will still mean 100
affected locations.

The visible list will show fewer top-level rows after grouping. Each group badge will state its own
occurrence count.

Source filtering will run before grouping. Severity filtering will also run before grouping. The
same filtered diagnostics will therefore determine both the tab count and the displayed groups.

### Expansion state

Store expanded `ProblemGroupId` values in `ProblemsPanel`. Clear keys that no longer exist after a
diagnostic refresh.

Derive the ID from dataset, source, severity, and producer group key. Do not use the visible summary
or an item index.

## Reusable clustering crate

Create `crates/clustering`.

The crate will own:

- Unicode token normalization;
- strict, sorted, and diacritic-insensitive keys;
- candidate blocking;
- normalized Damerau-Levenshtein comparison;
- value frequencies and source rows;
- pair evidence and stable pair keys;
- the `value variants` validator;
- its one-cell fix provider.

The crate will depend on:

- `caseless`;
- `unicode-normalization`;
- `strsim`;
- `diagnostics`;
- `settings`;
- `gpui` for validator registration state and fix lookup.

The core matching module must stay free of GPUI types. The validator adapter can convert core
results into `SharedString`, diagnostics, and fixes.

Move these dependencies out of `spellcheck`:

- `caseless`;
- `unicode-normalization`;
- `strsim`.

After the move, `spellcheck` will own dictionaries, language selection, spelling, capitalization,
and custom words. It will not own general value comparison.

The app will register `clustering::ValueVariants` independently from the spell checker. The source
name remains `value variants` so existing severity settings continue to work.

## Value-variant grouping

The first grouped implementation will review candidate pairs. It will not build transitive entity
clusters.

For each accepted pair, the clustering crate will return:

- both displayed values;
- the rows for each value;
- each value count;
- the matching reason;
- a stable unordered pair key.

All diagnostics for that pair will use the same `DiagnosticGroup`. The collapsed summary will show
both values and the reason.

This avoids unsafe single-link chaining. If A matches B and B matches C, qrate will not assume that
A matches C.

The future cluster-review workspace can use the same core results. It can add canonical selection,
member selection, row previews, and one-step bulk edits.

## Files and crates affected

### `crates/diagnostics`

- Add `DiagnosticGroup`, `ColumnFinding`, and the derived `Scope`.
- Update `address` and `ColumnValidator`.
- Add a pure panel projection that groups filtered diagnostics.
- Add expansion state and grouped rows to `ProblemsPanel`.
- Keep `Diagnostics::set`, `Diagnostics::at`, and the row index atomic.
- Test grouping, counts, filters, scopes, expansion, navigation, and fixes.

### `crates/spellcheck`

- Return `ColumnFinding` from spelling and capitalization validators.
- Add stable group keys for repeated spelling and capitalization findings.
- Remove value-variant code and general comparison dependencies.
- Keep dictionary behavior unchanged.

### `crates/clustering`

- Receive the current value-variant implementation and tests.
- Separate the pure matcher from the diagnostic adapter.
- Return pair evidence, frequencies, rows, and stable pair keys.
- Publish grouped value-variant findings.

### `crates/checks`

- Return `ColumnFinding` from date checks.
- Convert authority results to the named type.
- Leave findings ungrouped unless the rule has a clear stable subject.

### `crates/plugin-host`

- Adapt plugin validator output to `ColumnFinding`.
- Keep the public Luau result format unchanged.
- Leave plugin findings ungrouped in this phase.

### `crates/app`

- Register the value-variant validator and fixes from `clustering`.
- Remove the spell-check ownership link.

### Workspace and documentation

- Add `crates/clustering` to the workspace.
- Update `Cargo.lock`.
- Update `CLAUDE.md` crate map.
- Update `docs/diagnostics.md`.
- Add OpenRefine credit to the diagnostic documentation.

### Not affected in the first phase

- `table` already supplies one-cell fix hooks and exact location navigation.
- `workspace` already hosts `ProblemsPanel`.
- `settings` already stores the opt-in value-variant setting.
- `plugin-api` does not expose grouped diagnostic metadata yet.

## Implementation sequence

### 1. Pin current behavior

- Add tests for 100 identical spelling findings.
- Add tests for a value-variant pair that occurs in many rows.
- Add tests for dataset, column, row, and cell labels.
- Confirm that fixes query the current cell value.

### 2. Add the diagnostic contracts

- Add `DiagnosticGroup`.
- Add `ColumnFinding`.
- Add `Location::scope` and constructors.
- Update built-in validator implementations.
- Keep plugin output compatible through its adapter.
- Confirm that source replacement still clears stale findings.

### 3. Add the pure group projection

- Group only diagnostics with explicit metadata.
- Keep ungrouped diagnostics independent.
- Calculate occurrence totals and top-level group totals.
- Filter before grouping.
- Sort groups by severity, summary, and stable key.
- Sort occurrences by table location.
- Test mixed scopes and mixed severity overrides.

### 4. Render grouped diagnostics

- Render collapsed groups with occurrence badges.
- Expand groups inline.
- Keep the list virtualized.
- Keep navigation and context menus on occurrence rows.
- Add keyboard and accessibility labels to expansion controls.
- Remove stale expansion keys after revalidation.

### 5. Teach spelling to group

- Group repeated misspellings by language and normalized token.
- Group capitalization findings by observed and canonical forms.
- Keep different replacement targets in different groups.
- Confirm that 100 repeated mistakes produce one collapsed row and 100 occurrences.

### 6. Extract value clustering

- Create the `clustering` crate.
- Move the matcher without changing thresholds.
- Split pure matching from the GPUI adapter.
- Publish one group for each accepted value pair.
- Keep fixes cell-specific.
- Run the existing 2,000-value blocking test in the new crate.

### 7. Document and validate

- Document all four diagnostic scopes.
- Document group identity and count semantics.
- Credit OpenRefine as a design influence.
- Run focused tests after each crate change.
- Run `./scripts/ci.sh` before the branch is ready for review.
- Test expansion, filtering, navigation, fixes, undo, and revalidation in the app.

## OpenRefine credit and license boundary

OpenRefine influenced the proposed review flow and the strict-to-broad clustering order:

- OpenRefine, “Cluster and edit”:
  <https://openrefine.org/docs/manual/cellediting#cluster-and-edit>
- OpenRefine, “Clustering Methods In-depth”:
  <https://openrefine.org/docs/technical-reference/clustering-in-depth>
- OpenRefine source:
  <https://github.com/OpenRefine/OpenRefine>

OpenRefine documentation uses CC BY 4.0. OpenRefine source code uses the BSD 3-Clause license.

This branch will independently implement the algorithms from their published descriptions and
standard algorithm references. It will not copy OpenRefine source code, UI assets, or documentation
text.

Add this acknowledgment to `docs/diagnostics.md`:

> qrate's value-clustering workflow is inspired by OpenRefine's Cluster and edit feature. qrate
> uses an independent implementation designed for its diagnostics and cataloging workflow.

Link both feature names to the OpenRefine documentation. Do not imply that OpenRefine endorses
qrate.

An acknowledgment is sufficient for design influence and independently implemented standard
algorithms. Do not add OpenRefine to `NOTICES` unless qrate copies or adapts its copyrighted source
or distributes its material.

If later work copies or adapts OpenRefine code:

1. Record the exact upstream file and commit.
2. Keep the upstream copyright notice.
3. Add the BSD 3-Clause license text to `NOTICES`.
4. Mark the adapted qrate file.
5. Keep documentation excerpts under CC BY 4.0 with title, author, source, license, and change notes.

## Future issue: cluster review workspace

Track the full workspace in [GitHub issue #125](https://github.com/devnull03/qrate/issues/125).

Track this outside the current implementation:

- a dedicated cluster review workspace;
- value frequencies and affected-row counts;
- canonical value selection;
- a custom canonical value;
- member inclusion and exclusion;
- sample row and thumbnail context;
- match evidence and strategy controls;
- skip and reject judgments;
- persistent decisions;
- one-step bulk replacement;
- re-cluster after an accepted change;
- value-frequency facets;
- authority reconciliation candidates;
- mapping export and reuse.

This issue should cite OpenRefine as a design influence. It should state that qrate will use an
independent implementation.

## Non-goals

- Do not build the full cluster-review workspace on this branch.
- Do not add phonetic, n-gram, PPM, or authority clustering on this branch.
- Do not merge similar values automatically.
- Do not group diagnostics by message text.
- Do not replace atomic diagnostics with aggregate records.
- Do not change stored note schema.
- Do not change the plugin API.
- Do not add group-wide fixes yet.

## Definition of done

- Repeated equivalent findings appear as one collapsed group.
- Each group shows its occurrence count.
- A user can expand a group and jump to every exact location.
- Existing one-cell fixes work from expanded occurrences.
- Dataset, column, row, and cell scopes have clear labels.
- Source and severity filters produce correct groups and counts.
- Spelling and capitalization emit stable group metadata.
- Value clustering lives outside `spellcheck`.
- Value-variant pairs emit stable group metadata.
- Similarity never causes an automatic data change.
- Existing spell-check and value-variant behavior stays intact.
- Tests cover grouping, scopes, filters, fixes, revalidation, and comparison bounds.
- The documentation credits OpenRefine without claiming affiliation.
- The full workspace test and CI script pass.

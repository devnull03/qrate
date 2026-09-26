# Import duplicates plan (ASNT-77, #87)

Folds [#87](https://github.com/devnull03/qrate/issues/87) into the Filesystem Collections branch
(PR #134, now merged). That branch turned every import into a component plan, so the duplicate policy
belongs to the planner rather than the old flat `match_folder` extra-files list.

## Why it lands here

Digitization arrives in batches. The second batch is usually dropped onto a project that already
holds the first, often as the same parent folder with new files in it. Before this work, the
branch did this:

- **Open table:** `append_components` never looks at existing rows. Dropping a folder that is
  already imported creates a second copy of every component.
- **Wizard, two rows name one file:** `append_folder_components` collects claims into a
  `HashMap<component, row>`, so the last row wins and the other row silently stays unlinked.
- **Wizard, one row names several files** (the same basename in two subfolders): the row claims
  nothing, and each file gets a new row. The row and the files are now duplicates of each other.
- **Wizard, preview and creation disagree:** `match_folder` indexes files with
  `settings::filenames::keys`, which includes id prefixes. Creation indexes them with
  `lookup_keys`, which does not. A multi-part item (`2020_04_001` → `2020_04_001_001.jpg`) is
  counted as matched in the Files step, then not linked at creation, so its files arrive as new
  rows beside the row that names them.

## What counts as a duplicate

An incoming file is checked against existing components in this order. The first rule that decides
wins:

1. **Same stored source.** An existing component's `source_path`, resolved against the files root,
   is the same file as the incoming absolute path. This is exact and is the only rule that applies
   to directories.
2. **Same filename key, exactly one candidate.** The incoming file's `filenames::keys` hit exactly
   one existing row's Filename cell (`lookup_keys`). This is the rule that finds a spreadsheet row
   for its file.
3. **Same filename key, several candidates.** This is **ambiguous**. It is reported and never
   merged automatically, whatever the policy.

Inside one import, the same absolute path reached twice (a folder plus a file inside it dropped
together) collapses to one component without asking. That is a single file, not a choice.

Rows with identical *metadata* are not duplicates. Two rows can describe the same photograph on
purpose, and qrate's Problems panel is where that gets flagged, not the importer.

## The policy

```rust
pub enum DuplicatePolicy {
    /// Leave the existing component alone and drop the incoming one. The default: re-importing a
    /// batch folder is the common case and must be a no-op for what is already there.
    Skip,
    /// Keep the existing row and its typed metadata; re-link it to the incoming file and move it
    /// to the imported position in the hierarchy.
    Update,
    /// Import it as a new component anyway.
    AddAsNew,
}
```

**"Overwrite" deliberately means `Update`, not replacement.** qrate links files in place and copies
nothing, so the only thing an import can bring that the row lacks is *where the file is* and *where
it sits in the arrangement*. Blanking a catalogued row because its file was dropped again would
destroy the archivist's work for no gain, so no policy clears cells.

Ambiguous matches ignore the policy. They are listed in the preview, and each one becomes a new
component unless the archivist picks a target row there.

The chosen policy is saved per project in `__settings` as `import_duplicate_policy`, so the next
batch starts with the same answer.

## Design

### `file-ingest`: one pure resolver

A new `duplicates` module. It knows nothing about the project, so the caller hands it plain facts:

```rust
pub struct ExistingComponent {
    pub key: u64,                    // the caller's RowId
    pub absolute_source: Option<PathBuf>,
    pub filename_keys: Vec<String>,  // lookup_keys of the Filename cell
}

pub enum Resolution {
    Create,
    Skip { existing: u64 },
    Update { existing: u64 },
    Ambiguous { candidates: Vec<u64> },
}

pub struct ResolvedPlan {
    pub plan: ImportPlan,             // within-import duplicates already collapsed
    pub resolutions: Vec<Resolution>, // parallel to plan.components
}

pub fn resolve(
    plan: ImportPlan,
    existing: &[ExistingComponent],
    file_keys: impl Fn(&str) -> Vec<String>,
    policy: DuplicatePolicy,
) -> ResolvedPlan;
```

`file_keys` is passed in as `settings::filenames::keys` so both callers use one key rule, which
fixes the preview/creation disagreement. A skipped directory still anchors its children: an incoming
file under a skipped folder is parented to the *existing* folder row, so a second batch lands inside
the first batch's series instead of in a new copy of it.

### Wizard

- `match_folder` runs `resolve` against the spreadsheet rows, which are the "existing components"
  here, and `FolderMatch` gains `duplicate_rows`, `ambiguous_files`, and the resolved plan.
  `extra_files` goes away.
- The Files step shows the counts and, only when there is at least one duplicate or ambiguous
  match, a three-way choice for the policy.
- `append_folder_components` consumes the `ResolvedPlan` instead of re-scanning and re-matching. For
  a new project, `Update` and `Skip` differ only for rows that claim the same file: `Update` links
  the first row by sheet order and reports the rest, and `Skip` links none of them.

### Open table

- `import_external_paths` builds `ExistingComponent`s from the live delegate (structure
  `source_path` plus Filename cells) and resolves on the background executor.
- The prompt names the duplicate counts and offers `Skip duplicates`, `Update existing`,
  `Add all as new`, and `Cancel`. A drop with no duplicates keeps today's two buttons.
- `append_components` takes the `ResolvedPlan`. `Create` rows are appended. `Update` becomes a
  Filename cell change plus a structure change on the existing row. Everything goes in **one**
  history step: `Step::RowsAdded` gains a `cells` field carrying those before/after cell values,
  since it already carries before/after structure.

## Status

Steps 1-3 shipped to `main` with PR #134. Two things differ from the design above, both
deliberately:

- **`Update` re-links, it does not re-arrange.** The existing row keeps its place in the hierarchy;
  only its Filename cell and stored source path follow the file. Moving a catalogued component
  because its file was dropped again would undo arrangement the archivist did by hand.
- **The wizard's control appears only when the folder holds a file that several rows name**, which
  is the one ambiguity project creation cannot settle on its own. A file matching exactly one row
  always links that row, since that is what a folder-backed project is for.

## Delivery

Each step was one commit on `feat/filesystem-collections-plan`, merged to `main` with PR #134.

1. **Resolver.** `file_ingest::duplicates` with unit tests for every rule and policy: exact source,
   single key, ambiguous, within-import collapse, and a skipped parent adopting new children.
2. **Wizard.** Use `keys` on the file side, route `match_folder` and creation through `resolve`,
   add the policy control and persisted setting. Tests: two rows naming one file under each policy,
   one row naming two same-named files, and a multi-part item matched in preview and at creation.
3. **Open table.** Resolve before the prompt, apply `Update` in the same undo step. A gpui test
   drops the same folder twice under `Skip` (no new rows, undo restores) and under `Update` (the
   moved file is re-linked, typed cells untouched).
4. **Docs and trackers.** Add a Duplicates subsection to the Import preview section of
   `filesystem-collections-plan.md`. Add a memory-log entry to the ASNT-77 Notion page, link #87
   from PR #134 (`Closes #87 (Notion ID: 3b821d32-b13b-818d-a1b6-c542b16fe2d7)`).

## Definition of done

From #87, adapted to this branch: `./scripts/ci.sh` is green, and importing a folder/spreadsheet
combination with an intentional duplicate produces the chosen behaviour, covered by tests next to
the matching code in both the wizard and the open table.

## Out of scope

- Content hashing to spot the same file under a different name or path. That is expensive on
  network drives, and the source-path and key rules cover the batch workflow #87 describes.
- Duplicate *metadata* detection between rows, which belongs to diagnostics.

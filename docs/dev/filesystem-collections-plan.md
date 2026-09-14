# Filesystem collections implementation plan

## Outcome

qrate will create and extend archival hierarchies from files and folders. A directory can become
an archival component, its children can become subordinate components, and every component remains
a normal editable row. The table will show the hierarchy inline with disclosure controls and allow
components or complete subtrees to be reordered and reparented.

Files and folders can enter qrate through the launcher, project wizard, open table, gallery,
Details panel, Filename cells, and Project Settings. Browse buttons remain available and call the
same import pipeline as drops.

The release name is **Filesystem Collections**. The user-facing release line is:

> Build projects from files and folders, add material by dropping it into an open project, and
> arrange nested components without leaving the table.

## Implementation status

- In progress: shared filesystem inventory, relative-path preservation, and adoption by project
  creation and open-project file resolution.
- Pending: schema version 4, hierarchical table behavior, import preview, drop surfaces, and
  interchange contracts.

## Archival model

RAD and DACS describe archival material at multiple levels and require the relationship between a
component and the next higher component to remain clear. RAD commonly names fonds, collection,
series, file, and item; it also permits subdivisions where the arrangement requires them. DACS does
not prescribe one mandatory set of levels. RiC expresses broader whole-part and hierarchical
relationships rather than requiring one fixed tree vocabulary.

qrate will follow the shared structural requirements instead of treating one standard's English
labels as its database model:

- Every archival description is a row, including fonds, collections, series, files, and items.
- Each row has at most one immediate parent in the table hierarchy.
- A parent can have any number of ordered children.
- A row has a level-of-description term from the active profile or a project-defined term.
- The relationship is stored independently of visible metadata columns.
- Information in a parent row is not copied into its children.
- A component without a linked file is valid.
- The hierarchy reflects the current intellectual arrangement. It need not remain identical to the
  disk hierarchy after import.

The first profiles will be:

| Profile | Initial level terms | Default folder level | Default file level |
|---|---|---|---|
| RAD | Fonds, Collection, Sous-fonds, Series, Subseries, File, Item | Series | Item |
| DACS | Collection, Record Group, Series, Subseries, File, Item | Series | Item |
| ISAD(G) | Fonds, Sub-fonds, Series, Subseries, File, Item | Series | Item |
| RiC | Record Set, Record, Record Part | Record Set | Record |
| Custom | User-defined ordered terms | User choice | User choice |

These are editable project defaults, not validation of a single permitted depth sequence. A RAD
project can skip a level or use a local subdivision. The UI stores stable vocabulary keys and
separate display labels so a project can rename `Series` to `Series / grouping`, translate it, or
add a local level without rewriting relationships.

## User-visible columns

Folder import will not force `Collection`, `Parent collection`, `Relative path`, or similar columns
into the dataset. The hierarchy and source locator are project structure.

The existing column type combines a value shape with a semantic role. `Title` and `Filename` are
already roles in practice. Extend the current model with these special types:

- `Level of Description`: an optional visible projection of the row's internal level.
- `Parent Reference`: an optional visible projection of its immediate parent's identifier or title.

Users can name these columns anything. During project creation they map source columns to Title,
Filename, Level of Description, and Parent Reference roles. Title and Filename remain required for
a folder-backed project. The latter two are optional because folder import can populate internal
structure without exposing implementation columns.

When a projected structural cell is edited, qrate validates and applies the corresponding
structural operation. It must not permit a displayed Parent Reference value to disagree with the
stored hierarchy. Ambiguous title references require the user to choose a specific component;
identifiers are preferred when available.

In a later cleanup, split `ColumnType` into value kind and semantic role. That migration is not a
prerequisite for this release and should not be bundled unless adding the two structural types makes
the current enum materially confusing in implementation.

## Project schema version 4

Keep user metadata in `dataset_main`. Add a private table keyed by the stable row IDs already stored
there:

```sql
CREATE TABLE __row_structure (
    row_id       INTEGER PRIMARY KEY,
    parent_id    INTEGER REFERENCES dataset_main(_row_id) ON DELETE CASCADE,
    level_key    TEXT NOT NULL,
    sibling_order INTEGER NOT NULL,
    source_path  TEXT,
    source_kind  TEXT CHECK (source_kind IN ('file', 'directory'))
);
```

SQLite treats `NULL` parent IDs as distinct in unique indexes, so root sibling order cannot be
enforced with a simple `(parent_id, sibling_order)` constraint. Normalize and validate sibling order
in the structure writer rather than adding a constraint that protects children but gives roots a
false appearance of protection.

`source_path` is the normalized path relative to the project's files root when possible. It is
internal link metadata, not an archival description column. Absolute paths are allowed only for an
explicit externally linked file. Store paths with `/` separators in SQLite and convert at the OS
boundary.

Add profile configuration to `__settings`:

- `description_profile`: `rad`, `dacs`, `isadg`, `ric`, or `custom`.
- `description_levels`: ordered JSON records containing stable key, display label, and optional
  broader default.
- `folder_level_key` and `file_level_key`: import defaults.
- `table_hierarchy_expanded`: the expanded row IDs, persisted as presentation state.

### Migration behavior

Opening a version 3 project remains read-only. Do not create `__row_structure` merely by opening it.
On the first hierarchy write, create the table in a transaction, set every existing row to a root
component with the project's default item level, write the requested change, then set
`PRAGMA user_version = 4`.

New projects create the table immediately. Loading treats an absent structure row as an ungrouped
root, which keeps partially migrated or manually edited projects usable. Save dataset and structure
in one transaction so a crash cannot leave parent IDs referring to the previous dataset rewrite.
The current `save_dataset` drops and recreates `dataset_main`; it must be replaced with a transaction
that temporarily preserves or rewrites `__row_structure` around that operation.

Validate on load and before save:

- all referenced row IDs exist;
- a row is never its own parent;
- following parents always terminates;
- sibling order is unique and contiguous after normalization;
- stored level keys exist in the project vocabulary;
- source paths do not escape the files root unless marked external.

Invalid relationships become Problems findings. qrate should detach a cycle or dangling component
only through an explicit accepted fix, never silently.

## Crate changes

### New `crates/file-ingest` crate

This crate owns filesystem discovery and pure import planning. It has no GPUI or project-state
dependency.

Core types:

```rust
pub struct ScanRequest {
    pub roots: Vec<PathBuf>,
    pub recursive: bool,
    pub include_hidden: bool,
    pub follow_directory_symlinks: bool,
}

pub struct DiscoveredEntry {
    pub root: PathBuf,
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub kind: EntryKind,
    pub parent: Option<usize>,
}

pub struct ImportPlan {
    pub components: Vec<PlannedComponent>,
    pub warnings: Vec<ImportWarning>,
}
```

The scanner will preserve relative paths, sort deterministically by normalized path, ignore existing
OS cruft, detect duplicate names, report unreadable entries, and avoid following directory symlinks
by default. A visited-directory identity guard is still required when following links is enabled.

Move the wizard's `list_files`, matching inventory, ignored-file rules, and the recursive traversal
inside `PhotoIndex` onto this shared inventory. Filename-key matching remains in `settings` because
it is also used outside import.

Scan work runs on GPUI's background executor in its callers. A new scan supersedes the previous
request; results carry a request generation so a late scan cannot overwrite a newer drop.

### `settings`

- Add schema-v4 structure types and read/write operations.
- Extend `ProjectData` with row structure aligned by stable row ID, not source index.
- Add lazy v3-to-v4 write migration.
- Save dataset rows and row structure atomically.
- Add project profile and level-vocabulary settings.
- Add `LevelOfDescription` and `ParentReference` column types.
- Add structural validators that return actionable errors without depending on the table crate.

### `project-wizard`

- Replace `FolderMatch.extra_files: Vec<String>` with a complete `ImportPlan`.
- Accept dropped spreadsheet, column-config, file, and directory paths.
- Add an Arrange step for folder-backed projects.
- Let users choose a standards profile, rename its suggested levels, add local levels, and choose
  the default folder and file levels.
- Map Title, Filename, optional Level of Description, and optional Parent Reference columns.
- Preview the generated tree and counts before creation.
- Pass initial row structure through `ProjectSpec`; do not reconstruct it from filenames in the
  settings crate.

### `table`

- Replace the single-purpose row insertion API with batch insertion of complete rows and structure.
- Extend history rows to carry parent, level, sibling order, and source path.
- Add structural operations for insert subtree, delete subtree, move before/after/into, reparent,
  change level, indent, outdent, and group selected rows.
- Compute visible rows from filters plus expanded ancestors.
- Render disclosure controls and indentation in the pinned row header.
- Add external drop handlers and one import-preview entry point shared by table and gallery.
- Refresh file resolution from stored source paths first, then retain filename-key fallback for old
  projects and manually entered values.
- Emit one `TableChanged`, diagnostics alignment, dirty mark, and autosave per completed operation.

### `workspace`

- Route gallery and Details drops into the table import command.
- Make gallery order follow the visible hierarchical table order.
- Show parent components as ordinary cards when the active view includes them.
- Add breadcrumb context to Details so the selected component's ancestors are visible.

### `app`

- Add File menu commands for Import Files or Folders and Relink Missing Files.
- Route launcher-level external paths by type.
- Preserve relative directory layout in ZIP export.
- Add the standards profile and vocabulary editor to Project Settings.

### Other contracts

- `diagnostics`: use row IDs for hierarchy findings; validate cycles, dangling parents, missing
  source paths, and invalid projected structural values.
- `data-exchange`: export hierarchy through format-specific mappings. CSV and Sheets can optionally
  project level and parent fields with user-chosen headers. JSON-LD should express explicit
  whole-part relationships. ZIP preserves directories.
- `plugin-api` and `plugin-host`: expose row ID, parent row ID, level key/label, children, and source
  locator in command context and row hooks. This changes copied plugin contracts and therefore
  requires the synchronized repositories described in `CLAUDE.md`.
- `ai`, table agent adapter, and `AGENTS.md`: add hierarchy fields to protocol row selection and
  query results only when requested. Add revision-bound hierarchy mutation proposals only if the
  bridge later gains structural draft operations; this release does not grant direct writes.

## Table presentation

The pinned row-number column becomes the tree gutter:

```text
▾  1  Collection A
   ▾  2  Photographs
         3  portrait.jpg
         4  picnic.jpg
   ▸  5  Correspondence
      8  Unarranged item
```

The chevron is present only when a component has children. Indentation is based on ancestor depth
and capped visually so a malformed or extremely deep tree cannot consume the table width. The
source row number remains visible and stable enough for current diagnostics references.

Collapse state is presentation state and is not undoable. Structural changes are data changes and
are undoable. Collapsing a selected subtree keeps the parent selected and removes hidden descendants
from action targets. Expanding restores descendants without selecting them.

Filtering uses ancestor context:

- A matching component is visible.
- Its ancestors are visible so the match has context.
- Descendants that do not match remain hidden unless the filter itself matches the parent and the
  user selects “include descendants of matching components.”
- Collapse still applies inside the filtered result.
- Search next/previous traverses only visible results.

Sorting a column offers two modes:

- **Within each parent** sorts siblings and preserves the hierarchy. This is the default.
- **Flat results** temporarily displays a flat sorted view and disables structural drag handles.

Sorting does not rewrite sibling order until the user explicitly chooses “Make this order
permanent.”

## Moving and reparenting cases

Internal row drag is distinct from external file drop. The row header starts a component drag; file
drops remain active over cells and empty table space.

Each row drag shows three target zones:

- upper edge: move before the target as its sibling;
- centre: make the dragged component a child of the target;
- lower edge: move after the target as its sibling.

The operation moves the entire subtree. Multi-selection first removes selected descendants whose
ancestor is also selected, then moves the remaining disjoint subtrees in visible order.

| Case | Behavior |
|---|---|
| Move a leaf before or after a sibling | Change sibling order only. |
| Move a parent | Move its complete subtree. |
| Drop onto a component | Reparent as its last child. |
| Drop onto itself or a descendant | Reject because it creates a cycle. |
| Drop between different parents | Use the target sibling's parent. |
| Indent | Make it the last child of its previous visible sibling. |
| Outdent | Make it the next sibling after its current parent. |
| Move filtered rows | Allow only when parent and destination context are visible. |
| Move in flat-sort mode | Disable and explain that the hierarchy is not visible. |
| Move several selected siblings | Preserve their relative order. |
| Move selections from several parents | Preview the resulting parents before applying. |
| Delete a parent | Ask whether to delete the subtree or promote its children. |
| Duplicate a parent | Duplicate its complete subtree with fresh row IDs. |
| Cut and paste a parent | Move its complete subtree. |
| Copy and paste a parent | Duplicate its complete subtree. |
| Insert above or below | Create a sibling using the active profile's inferred level. |
| Insert child | Create the first or last child using the configured next-level default. |

One history step stores the pre- and post-operation structure plus affected rows. Undo restores row
content, stable IDs where restoring a deletion, source paths, parents, levels, order, notes, and
resolved previews together.

## External drop behavior

### Launcher

- One `.qrate` file opens the project.
- One spreadsheet starts Spreadsheet + folder and preloads it.
- Folders or ordinary files start a folder-backed project.
- A mixed drop containing `.qrate` and import material asks the user which action to take.

### Wizard

- Spreadsheet fields accept supported spreadsheet files.
- Folder fields accept one directory.
- The main Files area accepts multiple files and folders and creates an import plan.
- Column configuration accepts a supported config file.
- Invalid drop types leave existing valid selections unchanged.

### Open table and gallery

- Dropping one file on a Filename cell sets that row's link.
- Dropping several files on a row offers “add as children” and “add as siblings.”
- Dropping a folder on a row defaults to adding its generated directory component as a child.
- Dropping on empty table or gallery space appends root components.
- Dropping between rows imports at that sibling position.
- Large or recursive drops always show the import preview.
- A small file drop can apply immediately and offer Undo.

### Files outside the project root

The preview offers:

1. Set the common ancestor as the new files root, with a resolution impact summary.
2. Link the files externally by absolute path.
3. Cancel.

qrate continues to link files in place. Copying files into a managed project directory is separate
future work and must not be implied by the word “import.”

## Import preview

The preview is mandatory for folders, more than a small threshold of files, duplicate matches, root
changes, and any warning. It contains:

- a collapsible tree with included/excluded controls;
- recursive scan toggle;
- hidden-file toggle;
- default folder and file levels;
- destination parent;
- title and filename mapping;
- duplicate and existing-row matches, resolved by the policy in [`import-duplicates-plan.md`](import-duplicates-plan.md);
- unreadable and skipped paths;
- final component and file counts.

Changing an option rebuilds the pure `ImportPlan`; it does not mutate the project. Apply performs
one transaction and one undoable table operation.

Existing row matching defaults to stored source path, then exact normalized relative path, then the
current filename-key heuristic. Ambiguous matches never merge automatically.

## Delivery sequence

### 1. Shared filesystem inventory

- Add `file-ingest` to the workspace.
- Move recursive traversal and ignored-file behavior into it.
- Preserve relative paths and directory entries.
- Adapt wizard matching and `PhotoIndex` without changing visible behavior.
- Test unreadable subtrees, symlink loops, hidden files, duplicate basenames, Unicode paths, and
  deterministic ordering on all supported platforms.

### 2. Schema and model

- Add schema-v4 structure types and lazy migration.
- Make project creation write initial hierarchy.
- Make dataset autosave atomic with structure.
- Add profile vocabulary settings and special column types.
- Update project schema tests and the migration example tools.

### 3. Hierarchical table

- Add hierarchy-aware view-to-source mapping.
- Render depth and disclosure controls.
- Implement collapse, expand, expand all, and collapse all.
- Implement subtree insert, delete, duplicate, indent, outdent, reorder, and reparent with undo.
- Define filter, search, selection, copy/paste, notes, diagnostics, and gallery behavior through
  focused tests.

### 4. Import planner and wizard

- Add standards-profile selection and editable terms.
- Add folder-tree preview and column-role mapping.
- Replace flattened extra-file rows with planned components.
- Keep Browse and drop paths on the same state transitions and validators.

### 5. External drops in the open project

- Add GPUI `ExternalPaths` handling at launcher, wizard, table, gallery, Details, Filename cells,
  and Project Settings.
- Add visual drag-over states and accessible equivalent commands.
- Add the import preview and route every surface through it.

### 6. Interchange and contracts

- Add hierarchy projections for CSV and Sheets.
- Add whole-part relationships to JSON-LD.
- Preserve source directories in ZIP.
- Update plugin contracts and their copied repositories.
- Update agent protocol documentation where its read surface changes.

### 7. Release finish

- Update user documentation and screenshots.
- Add upgrade and rollback notes for schema version 4.
- Test Windows, macOS, and Linux external drops, including network and removable drives.
- Run the full repository CI gate before the implementation PR leaves draft.

## Acceptance scenarios

1. Dropping a folder with two nested image folders creates parent components and item components,
   preserves both duplicate basenames, and links each preview to the correct file.
2. A user can rename generated folder components and assign different RAD levels without changing
   source paths.
3. A user can collapse a series, filter for a descendant, and still see enough ancestors to
   understand the result.
4. Moving a series moves its descendants, survives save and reopen, and undoes in one step.
5. Reparenting onto a descendant is rejected without changing row order or dirty state.
6. Deleting a parent offers subtree deletion or child promotion, and either choice undoes fully.
7. A v3 project opens without a write, gains structure on its first hierarchy edit, and reopens in
   an older release with its dataset intact even though the older release ignores hierarchy.
8. CSV and Sheets export use user-selected structural field names; a project that does not request
   those fields gains no extra columns.
9. ZIP export preserves nested paths and does not overwrite files with duplicate basenames.
10. Keyboard users can perform every structural operation available through dragging.

## Decisions to keep explicit during implementation

- Parent rows are archival components and remain available to search, diagnostics, plugins,
  exports, Sheets, gallery, notes, and agent queries.
- Disk layout seeds arrangement but does not own it after import.
- Hierarchy is stored by stable row ID and never inferred from visual adjacency.
- Source paths are private project structure unless a user maps them into a visible column.
- Standards profiles provide vocabulary and mapping defaults; they do not forbid local practice.
- Dragging is an interaction layer over commands. Browse, menus, and keyboard actions invoke the
  same operations.
- No scan mutates project data before the import plan is accepted.

## Primary standards references

- Canadian Council of Archives, [*Rules for Archival Description*, Chapter
  1](https://www.archivescanada.ca/wp-content/uploads/2022/08/RAD_Chapter01_July2008.pdf),
  especially rules 0.27 through 0.29 and 1.0D.
- Society of American Archivists, [*Describing Archives: A Content
  Standard*](https://files.archivists.org/pubs/DACS2E-2013_v0315.pdf), Statement of Principles and
  Chapter 1, Levels of Description.
- International Council on Archives, [*ISAD(G): General International Standard Archival
  Description*, second
  edition](https://www.ica.org/app/uploads/2023/12/CBPS_2000_Guidelines_ISADG_Second-edition_EN.pdf),
  multilevel description rules.
- International Council on Archives, [*Records in Contexts Conceptual Model*
  1.0](https://www.ica.org/app/uploads/2023/12/RiC-CM-1.0.pdf) and [RiC-O
  1.1](https://www.ica.org/standards/RiC/RiC-O_1-1.html), whole-part and hierarchical relations.

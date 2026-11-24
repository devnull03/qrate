# Capability reference

<!-- Generated from qrate's capability catalog. Do not edit: run `QRATE_BLESS=1 cargo test -p app capability_reference` after changing a capability. -->

Everything an agent can ask qrate to do. Call any of these with `qrate agent call <name>` and the parameters as JSON on stdin; `qrate agent capabilities` prints this list with full JSON Schemas. Every answer also carries `revision` and `result`.

| Effect | What it means |
|---|---|
| `read` | Changes nothing. |
| `stage` | A draft the archivist applies from qrate. Needs the `revision` you read. |
| `edit` | Changes the project at once, as one undo step credited to you. Needs the `revision`. |
| `confirm` | Waits for the archivist to confirm it in qrate, showing your `reason`. |
| `reach_files` / `reach_network` | Leaves the project, behind the archivist's permission. |

## files

### `check_access`

Check whether a path is readable without asking or widening any grant.

Effect: `reach_files`. Answer: `file_access`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes |  |

### `find_candidates`

Search folders for files that could be what rows link to, by file name and then by name without extension, especially for missing files. Stage the ones you are sure of with stage_relinks.

Effect: `reach_files`. Answer: `find_candidates`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `rows` | array | no | Source rows to look for, at most 100. Empty means every row whose file is missing. |
| `folders` | array | yes | Folders to search, 1 to 8, each inside the project or covered by a read grant. |
| `recursive` | boolean | no | Search subfolders too. Defaults to true. |

### `forget_access`

Give up every read grant you hold for this project.

Effect: `reach_files`. Answer: `file_access_revoked`.

No parameters.

### `import_files`

Attach readable files and folders as new rows, linking originals in place and skipping files already in the project. One undo step.

Effect: `edit`. Answer: `imported_files`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `paths` | array | yes |  |
| `recursive` | boolean | no |  |

### `request_access`

Ask the archivist for read-only access to one file or folder outside the project, with a reason. Lasts until qrate closes or they revoke it.

Effect: `reach_files`. Answer: `file_access`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes |  |
| `reason` | string | yes | Shown to the archivist. At most 512 bytes. |

### `stage_relinks`

Stage new file links for rows, such as for files that moved, as one draft. Each file must exist and be inside the project or covered by a read grant; applying repoints the rows as one undo step.

Effect: `stage`. Answer: `staged_relinks`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `links` | array | yes | At most 50,000. |
| `note` | string | no | Shown beside the draft. |

## media

### `image`

Read one row's linked file as a PNG of up to 2048 pixels, to read labels, captions or handwriting. Only files the project links; use thumbnails to skim.

Effect: `read`. Answer: `image`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `row` | integer | yes |  |
| `page` | integer | no | Page of a multi-page file, from 0. |
| `max_edge` | integer | no | Longest side in pixels, 256 to 2048. Defaults to 1568. |

### `similar`

Rank rows by how much their images look like one row's image, or like a short description, using qrate's on-device visual search. Useful for spotting duplicates and series. Needs visual search turned on by the archivist.

Effect: `read`. Answer: `similar`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `row` | integer | no | Find images like this row's. Give `row` or `text`. |
| `text` | string | no | Find images matching this description, such as "a parade float". |
| `limit` | integer | no | Rows to return, 1 to 50. Defaults to 20. |

### `thumbnails`

Read small PNG previews of up to four rows' linked files, when the image itself is the evidence.

Effect: `read`. Answer: `thumbnails`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `items` | array | yes | At most 4. |

## project

### `capabilities`

List every capability with its effect and the JSON Schemas of its parameters and answer.

Effect: `read`. Answer: `capabilities`.

No parameters.

### `overview`

Read the open project's summary, columns with their types and descriptions, selection count, diagnostic counts and the exact revision. Start here.

Effect: `read`. Answer: `overview`.

No parameters.

### `program_run`

Run the saved scratch program against the live table with JSON args.

Effect: `read`. Answer: `program_run`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `args` | object | no | Any JSON, handed to the program as `args`. |

### `program_save`

Compile and keep one read-only Luau scratch program, for analysis bounded queries cannot do efficiently.

Effect: `read`. Answer: `program_saved`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `source` | string | yes | Luau source returning `function(table, args)`. No network, filesystem, process, clock, randomness, plugin storage, UI or staging access; 2 s and 16 KB of output per run. |

### `query`

Read a bounded page of rows or diagnostics, optionally filtered, grouped, counted or distinct. Follow next_cursor only when more evidence matters.

Effect: `read`. Answer: `query`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `source` | object | no | Which records to read. Defaults to every row. |
| `select` | array | no | Fields to return, in order. Empty returns every field. |
| `where` | array | no |  |
| `group_by` | array | no | Count records per combination of these fields instead of returning them. |
| `distinct` | string | no | Return each exact value of one field once. Mutually exclusive with `select` and `group_by`. |
| `order_by` | object | no |  |
| `limit` | integer | no | Records per page, 1 to 50. |
| `cursor` | string | no | `next_cursor` from the previous page of the same query. |

### `wait_for_change`

Wait up to 25 seconds for the archivist to edit the table, change the selection, or apply or discard your drafts, then say what changed. Call it in a loop to follow along; `qrate agent watch` does that for you.

Effect: `read`. Answer: `wait_for_change`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | no | The `revision` you last saw; a different one answers at once. |
| `selection` | array | no | The `selection` you last saw; a different one answers at once. |
| `since` | integer | no | The `seq` of the last event you saw. |
| `timeout_s` | integer | no | How long to wait, 1 to 25 seconds. Defaults to 20. |

## review

### `add_note`

Attach a note to a cell or row, signed with your name, to explain a finding or ask the archivist something. Never overwrites a note someone else wrote.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `row` | integer | yes |  |
| `column` | string | no |  |
| `text` | string | yes | At most 2,000 bytes. |

### `adopt_check`

Ask the archivist to keep the program you saved (program_save) as one of the project's checks. It must return a list of {row, column, severity, message} when args.mode is "check", and it runs in the same read-only sandbox whenever the table changes.

Effect: `confirm`. Answer: `adopt_check`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `name` | string | yes | Shown in the Problems panel, at most 64 characters. Adopting a name you used before replaces that check. |
| `reason` | string | yes | Shown to the archivist: what the check looks for. At most 512 bytes. |

### `authority_search`

Look a term up in an authority file (LCSH, Wikidata or GeoNames) through qrate, to suggest a controlled heading. The archivist allows each authority once per session. Stage what you choose with stage_patch.

Effect: `reach_network`. Answer: `authority_search`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `authority` | string | yes | `LCSH`, `Wikidata` or `GeoNames`. |
| `term` | string | yes | At most 200 characters. |
| `reason` | string | no | Shown to the archivist the first time you reach this authority. |

### `checks`

List the agent-written checks this project runs, with who wrote them.

Effect: `read`. Answer: `checks`.

No parameters.

### `export_check`

Dry-run an export (CSV, Excel, CSL-JSON for Zotero, or the ZIP with files) in memory and report what would fail or be lost. Writes nothing.

Effect: `read`. Answer: `export_check`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `format` | string | yes |  |

### `history`

Read the project's History, newest first: who changed what and how, including your own credited changes. Filter to one row to see how it came to be.

Effect: `read`. Answer: `history`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `limit` | integer | no | Entries to return, 1 to 50. Defaults to 20. |
| `before` | integer | no | `next_before` from the previous page. |
| `row` | integer | no | Only changes to this source row. |

### `notes`

Read the notes on cells and rows, with who wrote them and when.

Effect: `read`. Answer: `notes`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `rows` | array | no | Only notes on these source rows. |

### `value_variants`

Group a column's values that look like variant spellings of each other (case, accents, punctuation, word order, near misses), with how many cells hold each. Fix them with stage_patch or stage_column_rewrite.

Effect: `read`. Answer: `value_variants`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `column` | string | yes |  |
| `limit` | integer | no | Groups to return, 1 to 100. Defaults to 20. |

## stage

### `stage_changeset`

Stage a whole migration as one draft: cell values (patch), file renames, relinks, new rows and deletions. The archivist reviews it in the table and applies it in one step, confirming any deletion with your reason; one undo takes it all back. A refused change set discards your staged drafts.

Effect: `stage`. Answer: `stage_changeset`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `patch` | array | no |  |
| `renames` | array | no |  |
| `relinks` | array | no |  |
| `add_rows` | array | no | New rows, each as column name → value. At most 1,000. |
| `delete` | object | no | Rows to delete inside a change set, and why. |
| `note` | string | no | Shown beside the draft. |

### `stage_column_rewrite`

Stage new values for one whole column as a single draft the archivist previews and applies. Never edit a column cell by cell. Not for file columns: rename files with stage_renames, or relink rows with stage_relinks.

Effect: `stage`. Answer: `staged_rewrite`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `column` | string | yes |  |
| `values` | array | yes | `[source row, new value]` pairs, each row once, at most 50,000. |
| `note` | string | no | Shown once beside the draft, such as where the values came from. |

### `stage_findings`

Publish one complete batch of advisory findings to the Problems panel, each with an optional whole-cell replacement the archivist can apply.

Effect: `stage`. Answer: `staged`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `findings` | array | yes | The complete batch, at most 200. Replaces this agent's previous findings. |

### `stage_patch`

Stage new values for several columns of several rows as one draft, each row optionally guarded by the values you read. Use it for splitting, merging or normalising across columns. Not for file columns: rename files with stage_renames, or relink rows with stage_relinks.

Effect: `stage`. Answer: `staged_patch`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `rows` | array | yes | Each source row at most once; 50,000 cells in all. |
| `note` | string | no | Shown beside the draft, such as where the values came from. |

### `stage_renames`

Stage renames of files rows link to, inside the project folder, as one draft. Applying renames all or none and updates the rows; undo renames them back.

Effect: `stage`. Answer: `staged_renames`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `renames` | array | yes |  |

## table

### `add_column`

Add a new, empty column after `after`, or first.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `name` | string | yes |  |
| `after` | string | no |  |

### `add_rows`

Append up to 1,000 rows, each as column name → value. One undo step.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `rows` | array | yes |  |

### `delete`

Ask the archivist to delete rows or one column, showing your reason. Waits until they answer; if they decline, do not ask again for the same thing.

Effect: `confirm`. Answer: `deleted`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `rows` | array | no | Source rows, at most 1,000. Give rows or a column, not both. |
| `column` | string | no |  |
| `reason` | string | yes | Shown to the archivist. At most 512 bytes. |

### `describe_column`

Set a column's data type, its description, or both.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `column` | string | yes |  |
| `data_type` | string | no |  |
| `description` | string | no |  |

### `duplicate_row`

Copy a row in below itself.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `row` | integer | yes |  |

### `filter`

Show only rows whose column holds one of `keep`, or clear that column's filter.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `column` | string | yes |  |
| `keep` | array | no |  |

### `indent_row`

Make a row a child of the row above it at its level.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `row` | integer | yes |  |

### `move_column`

Move a column after `after`, or to the front.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `column` | string | yes |  |
| `after` | string | no |  |

### `move_row`

Move a row before or after another row, or make it that row's child.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `row` | integer | yes |  |
| `target` | integer | yes |  |
| `placement` | string | yes |  |

### `outdent_row`

Move a row up one level of the hierarchy.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `row` | integer | yes |  |

### `rename_column`

Rename a column to a name not already used.

Effect: `edit`. Answer: `edited`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `revision` | integer | yes | The table's revision. Read it from `overview`; a call that names rows or cells must pass the revision it read them at, and is refused as stale if the table changed since. |
| `column` | string | yes |  |
| `name` | string | yes |  |

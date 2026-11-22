# Capability reference

<!-- Generated from qrate's capability catalog. Do not edit: run `QRATE_BLESS=1 cargo test -p table capability_reference` after changing a capability. -->

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

## media

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

## stage

### `stage_column_rewrite`

Stage new values for one whole column as a single draft the archivist previews and applies. Never edit a column cell by cell.

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

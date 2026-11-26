# Agent instructions

- **Changing qrate code?** Read [CLAUDE.md](CLAUDE.md).
- **Reviewing the project open in qrate?** Use the `qrate agent` commands below. The saved `.qrate` file can omit unsaved on-screen edits.

## Reading a running qrate

These commands reach the desktop app only. qrate in a browser tab has no local process to connect to and no assistant runtime, so there is nothing for them to read there.

First confirm the right project is on screen: `qrate app status` and `qrate project info` print JSON in a pipe (`{"running": …, "project": …}`), and `qrate <PROJECT.qrate>` opens one. `qrate help <command>` documents every option; [`skills/qrate-cli`](skills/qrate-cli/SKILL.md) covers finding and driving the command.

Run `qrate agent <command>`. The parameters are one JSON object on stdin, and the answer is JSON on stdout. Name yourself with `--agent codex` or `QRATE_AGENT=codex`. The name only labels your calls in qrate's Agent panel; it proves nothing about who you are.

| Exit | Meaning | Where to look |
|---|---|---|
| `0` | Answered | stdout: the response |
| `1` | Refused, for example a stale revision, bad parameters, or agent access switched off in Settings ▸ Agent | stdout: `{"error": …}` |
| `2` | Usage error, such as stdin that is not a JSON object, or a terminal where stdin should be | stderr |
| `3` | qrate is not running or cannot be reached | stderr |

Everything qrate can do for an agent is a **capability**. `qrate agent capabilities` lists them all with their effect and the JSON Schemas of their parameters and answer, and `qrate agent call <name>` calls any of them with the parameters on stdin. [`docs/cli/capabilities.md`](docs/cli/capabilities.md) is the same list as a page, generated from the catalog (a test fails if it drifts). Every answer carries `revision` and `result` beside its own fields. The named commands (`query`, `stage-findings`, `edit`, `delete`, …) are shorthands kept for scripts already using them.

Each capability declares one effect, and qrate enforces it the same way for every call:

| Effect | What it means for you |
|---|---|
| `read` | Changes nothing. |
| `stage` | A draft the archivist applies from qrate; pass the `revision` you read, or it is refused as `stale_revision`. Cell values and file names only ever change this way. |
| `edit` | Changes the table's shape at once, as one undo step credited "<archivist> using <agent>". Pass the `revision`. At most 120 a minute (`rate_limited`). |
| `confirm` | Shows the archivist your `reason` in a qrate dialog and waits (10 minutes). Refusals: `declined` (do not ask again), `confirm_timed_out`, `confirm_pending` (another is waiting), `reason_missing`, `no_window`. |
| `reach_files` | Files outside the project, behind the archivist's read grants. A path you pass must be inside the project or granted (`file_access_required`). |
| `reach_network` | Asks the archivist the first time you reach each destination in a session. |

qrate marks each staged change on its cell (old value struck through, new value beside it, on a yellow-outlined cell) with a bar above the table to apply or discard them all; right-clicking a marked cell applies or rejects that one. Drafts are kept per `--agent` name, so staging again replaces only your own. `stage_changeset` bundles a patch, renames, relinks, new rows and deletions into one draft that applies as one undo step, with any deletion confirmed when the archivist applies it. `wait_for_change` (and `qrate agent watch`) reports what the archivist edited, selected, applied or discarded. Agent findings stay ungrouped atomic diagnostics; findings the archivist ignored are absent from `overview` counts and `diagnostics` queries.

Internally, shared structural, arrangement, column-description and import entry points require an explicit History origin before mutation; the recorder rejects non-empty steps without change records. Table snapshots include column declarations, so their types/descriptions and History commit in one SQLite transaction. A new grid command must name its capability in `GRID_COMMANDS` (`crates/table/src/capabilities/mod.rs`) or a test fails.

qrate's Pi comes from a full install's `agent` folder or, when that is absent, from the optional component qrate installs on demand into `components/agent/<version>` in the same application-data directory. Without either, the Agent panel offers to install it (Settings ▸ Components does too) and starts Pi once it lands, with no restart.
qrate's bundled Pi runs as a contained child process with `QRATE_CLI` (this CLI) and `QRATE_AGENT=pi` set. Stop, Restart, panel teardown, and app exit terminate its process tree; do not rely on an older Pi process surviving a restart.
The bundled assistant loads its extension and `SYSTEM.md` with all skills disabled. The `skills/` folder in this repo packages the same instructions for an agent running outside qrate.

### Commands

The full list is [`docs/cli/capabilities.md`](docs/cli/capabilities.md). The ones most reviews need:

- `overview` (no stdin): project, columns (with `Title` for the primary row name), selection, diagnostic counts and the revision.
- `query`: bounded live rows or diagnostics. `source.kind` is `all_rows`, `selected_rows`, `rows`, `search`, or `diagnostics`; operations are `select`, `where`, `distinct`, `group_by`, `order_by`, `limit` (default 20, at most 50) and a revision-bound `cursor`. Results send one `fields` array and positional `items`, plus `returned`, `remaining`, `truncated` and `next_cursor`.
- `stage_patch` (several columns of several rows, each row optionally guarded by the values you read), `stage_column_rewrite` (one whole column), `stage_findings` (advisory findings with optional whole-cell replacements), `stage_renames` and `stage_relinks` (files rows link to), `stage_changeset` (all of these, plus new rows and deletions, as one draft).
- Shape edits, each its own capability: `add_rows`, `add_column`, `rename_column`, `move_column`, `describe_column`, `move_row`, `indent_row`, `outdent_row`, `duplicate_row`, `filter`, `add_note`. `edit` with `"op"` still works for the older form.
- `delete`: ask the archivist to delete rows or a column, with a reason.
- Review helpers: `value_variants` (variant spellings in a column), `history`, `notes`, `export_check` (dry-run an export), `image` and `similar` (larger previews and on-device visual search), `authority_search` (LCSH, Wikidata, GeoNames through qrate), `adopt_check` (keep a saved program as a project check, after the archivist confirms).
- `program_save` / `program_run`: a confined Luau scratch program for analysis queries cannot do. Programs have no network, filesystem, process, clock, randomness, plugin storage, UI, raw qrate objects, or staging access; their only linked-file methods are bounded UTF-8 reads and PDF text search.

```sh
echo '{"source":{"kind":"selected_rows"},"select":["Title"],"limit":10}' | qrate agent query --agent codex
echo '{"revision":7,"rows":[{"row":0,"set":{"Title":"Harvest Festival"},"expect":{"Title":"Harvest"}}]}' | qrate agent call stage_patch --agent codex
```

Use `stage_patch` or `stage_column_rewrite` for bulk value changes, never hundreds of findings. Use `stage_renames` to rename files a project links and `find_candidates` plus `stage_relinks` for files that moved; never `mv`, `ren` or `Move-Item` them yourself, because qrate cannot follow or undo that. Rename refusals name `rename_outside_project`, `rename_source_mismatch`, `rename_target_collision` or `rename_target_exists`; a swap or chain inside one batch is fine, and applying renames every file or none, with one undo renaming them back.

Every staged finding needs the exact `revision`, source `row`, `column`, severity, one-sentence message, and exact current whole-cell `expected`; `replacement` is optional and must be the whole proposed value. Never report that staging corrected data.

The capability layer is `crates/capability` (trait, dispatcher, catalog). The contract types are `crates/ai/src/agent.rs`; the table's capabilities are `crates/table/src/capabilities/`; `authority_search` is in `crates/checks/src/authority/search.rs`. The public command is `crates/cli/src/main.rs`, the private transport is `crates/app/src/app_control.rs`, the program runner is `crates/plugin-host/src/agent_program.rs`, and the audit panel is `crates/workspace/src/panels/agent.rs`. Keep this file synchronized when those surfaces change.

### Approved local files

- `request_access` (`{"path": "file or folder", "reason": "why"}`): request read-only access in qrate's own confirmation dialog. A file grant covers one file; a directory grant includes descendants. Relative inputs are resolved against the project folder. Canonical paths enforce the boundary, including symlinks. Grants are scoped to the agent label and current project and last until qrate closes or the archivist revokes them.
- `check_access` (`{"path": "..."}`): check existing access without prompting. The project folder and configured files folder are already readable. Refuses with `file_access_required` when a new approval is needed. Never bypass a declined request or repeat it without a new instruction from the archivist.
- `forget_access` (`{}`): revoke this agent's explicit grants for the current project. The Agent panel's session menu also offers **Revoke file access** for all agents.
- `import_files` (`{"revision": ..., "paths": ["..."], "recursive": false}`): attach approved paths through qrate's file-ingest and duplicate-resolution flow. It links originals in place, skips existing components, adds at most 1,000 components as one credited undo step, and returns `imported_files` with `added`, `skipped`, new source `rows`, and bounded `warnings`. Re-checks the revision and grant generation after discovery; a changed project, revision, or revoked grant refuses the import. It never copies, moves, or updates existing files or cell values.

Pi's built-in read/ls/find/grep tools check grants with qrate on every call. A read grant never approves writes or shell commands. Agent labels remain labels, not authenticated identities. Scratch Luau programs retain their original confinement.

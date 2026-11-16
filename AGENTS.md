# Agent instructions

- **Changing qrate code?** Read [CLAUDE.md](CLAUDE.md).
- **Reviewing the project open in qrate?** Use the `qrate agent` commands below. The saved `.qrate` file can omit unsaved on-screen edits.

## Reading a running qrate

First confirm the right project is on screen: `qrate app status` and `qrate project info` print JSON in a pipe (`{"running": …, "project": …}`), and `qrate <PROJECT.qrate>` opens one. `qrate help <command>` documents every option; [`skills/qrate-cli`](skills/qrate-cli/SKILL.md) covers finding and driving the command.

Run `qrate agent <command>`. The parameters are one JSON object on stdin, and the answer is JSON on stdout. Name yourself with `--agent codex` or `QRATE_AGENT=codex`. The name only labels your calls in qrate's Agent panel; it proves nothing about who you are.

| Exit | Meaning | Where to look |
|---|---|---|
| `0` | Answered | stdout: the response |
| `1` | Refused, for example a stale revision, bad parameters, or agent access switched off in Settings ▸ Agent | stdout: `{"error": …}` |
| `2` | Usage error, such as stdin that is not a JSON object, or a terminal where stdin should be | stderr |
| `3` | qrate is not running or cannot be reached | stderr |

No command changes a cell or a file. Staged bulk edits are allowed because applying one is the archivist's menu action, never a request: there is no auto-apply and no trusted agent. `stage-findings` only replaces this agent's draft findings in the Problems panel. It can also offer whole-cell replacements under a finding in that cell's Problems menu. Only the archivist applies them: one at a time, or all of one agent's at once with **Apply all N fixes from <agent>** in the same menu, which is a single undo step and skips any cell edited since staging. **Discard all** drops the batch. Drafts are kept per `--agent` name, so staging again replaces only your own, and the History panel credits an accepted fix as "<author> using <agent>".
Agent findings stay ungrouped atomic diagnostics. Findings the archivist ignored are absent from `overview` counts and `diagnostics` queries, exactly as they are hidden in the Problems panel.

qrate's Pi comes from a full install's `agent` folder or, when that is absent, from the optional component qrate installs on demand into `components/agent/<version>` in the same application-data directory. Without either, the Agent panel offers to install it (Settings ▸ Components does too) and starts Pi once it lands, with no restart.
qrate's bundled Pi runs as a contained child process with `QRATE_CLI` (this CLI) and `QRATE_AGENT=pi` set. Stop, Restart, panel teardown, and app exit terminate its process tree; do not rely on an older Pi process surviving a restart.
The bundled assistant loads its extension and `SYSTEM.md` with all skills disabled. The `skills/` folder in this repo packages the same instructions for an agent running outside qrate.

### Commands

- `overview` (no stdin): compact project, column, selection, diagnostic-count, and revision information. Column `data_type` values include `Title` for the primary human-readable row name.
- `query`: bounded live rows or diagnostics. `source.kind` is `all_rows`, `selected_rows`, `rows`, `search`, or `diagnostics`. Operations are `select`, `where`, `distinct`, `group_by`, `order_by`, `limit`, and a revision-bound `cursor`.
- `program-save` (`{"source": …}`): validate and activate a confined Luau function without running it.
- `program-run` (`{"revision": …, "args": …}`): run the saved function once against an immutable snapshot at an exact revision.
- `thumbnails` (`{"items": […]}`): return at most four qrate-generated 512-pixel PNG derivatives by source row/page.
- `stage-findings` (`{"revision": …, "findings": […]}`): publish a complete advisory draft batch.
- `stage-column-rewrite` (`{"revision": …, "column": …, "values": [[row, value], …], "note": …}`): stage new values for one whole column as a single draft.
- `stage-renames` (`{"revision": …, "renames": [{"row": …, "from": …, "to": …}]}`): stage renames of files the rows link to as a single draft.

```sh
echo '{"source":{"kind":"selected_rows"},"select":["Title"],"limit":10}' | qrate agent query --agent codex
```

Queries default to 20 and allow at most 50 records. Collection results send one `fields` array and positional `items`, plus `returned`, `remaining`, `truncated`, and `next_cursor`. Ask for the smallest useful field set, and follow a cursor only when more evidence could change the answer.

Programs have no network, filesystem, process, clock, randomness, plugin storage, UI, raw qrate objects, or staging access. Their only linked-file methods are qrate-resolved bounded UTF-8 reads and PDF text search. Original paths and bytes are never returned.

Use `stage-column-rewrite` for a bulk edit of one column, never hundreds of findings. It takes up to 50,000 `[source row, whole new value]` pairs, each row once and each value at most 64 KB, at the exact current `revision`; a stale revision, an unknown column, or a row out of range is refused. Rows already holding the value come back in `unchanged`. It shows as one Problems entry, "Rewrite <column>: N rows (<note>)", which the archivist previews, applies as one undo step, or discards. Staging the same column again replaces your earlier rewrite; empty `values` retracts it.

Use `stage-renames` to rename files a project links; never `mv`, `ren` or `Move-Item` them yourself, because qrate cannot follow or undo that. Each rename names the source `row` that links the file and `from`/`to` paths relative to the folder holding the `.qrate` file. The whole batch is refused if any path is absolute or leaves that folder (`rename_outside_project`), `from` is not the file its row links (`rename_source_mismatch`), two renames share a `from` or `to` (`rename_target_collision`), or `to` exists and is not itself being renamed away (`rename_target_exists`); a swap or a chain inside one batch is fine. It shows as one Problems entry, "Rename N files". Applying renames every file or none, updates the cell that names each file, and is one undo step that renames the files back. Staging again replaces your earlier renames; an empty list retracts them. To change identifiers and file names together, stage the rewrite and the renames at the same `revision`.

Every staged finding needs the exact response `revision`, source `row`, `column`, severity, one-sentence message, and exact current whole-cell `expected`. `replacement` is optional and must be the whole proposed value. A stale cell is rejected. Never report that staging corrected data.

The contract is `crates/ai/src/agent.rs`. The public command is `crates/cli/src/main.rs`, the private transport is `crates/app/src/app_control.rs`, the live adapter is `crates/table/src/agent.rs`, the private runner is `crates/plugin-host/src/agent_program.rs`, and the audit panel is `crates/workspace/src/panels/agent.rs`. Keep this file synchronized when those surfaces change.

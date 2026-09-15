# Agent instructions

- **Changing qrate code?** Read [CLAUDE.md](CLAUDE.md).
- **Reviewing the project open in qrate?** Use the `qrate agent` commands below. The saved `.qrate` file can omit unsaved on-screen edits.

## Reading a running qrate

Run `qrate agent <command>`. The parameters are one JSON object on stdin, and the answer is JSON on stdout. Name yourself with `--agent codex` or `QRATE_AGENT=codex`. The name only labels your calls in qrate's Agent panel; it proves nothing about who you are.

| Exit | Meaning | Where to look |
|---|---|---|
| `0` | Answered | stdout: the response |
| `1` | Refused, for example a stale revision, bad parameters, or agent access switched off in Settings ▸ Agent | stdout: `{"error": …}` |
| `2` | Usage error, such as stdin that is not a JSON object | stderr |
| `3` | qrate is not running or cannot be reached | stderr |

No command changes a cell. `stage-findings` only replaces this agent's draft findings in the Problems panel. It can also offer whole-cell replacements under a finding in that cell's Problems menu. Only the archivist applies them.
Agent findings stay ungrouped atomic diagnostics. Findings the archivist ignored are absent from `overview` counts and `diagnostics` queries, exactly as they are hidden in the Problems panel.

qrate's bundled Pi runs as a contained child process with `QRATE_CLI` (this CLI) and `QRATE_AGENT=pi` set. Stop, Restart, panel teardown, and app exit terminate its process tree; do not rely on an older Pi process surviving a restart.
The bundled assistant loads its extension and `SYSTEM.md` with all skills disabled. The `skills/` folder in this repo packages the same instructions for an agent running outside qrate.

### Commands

- `overview` (no stdin): compact project, column, selection, diagnostic-count, and revision information. Column `data_type` values include `Title` for the primary human-readable row name.
- `query`: bounded live rows or diagnostics. `source.kind` is `all_rows`, `selected_rows`, `rows`, `search`, or `diagnostics`. Operations are `select`, `where`, `distinct`, `group_by`, `order_by`, `limit`, and a revision-bound `cursor`.
- `program-save` (`{"source": …}`): validate and activate a confined Luau function without running it.
- `program-run` (`{"revision": …, "args": …}`): run the saved function once against an immutable snapshot at an exact revision.
- `thumbnails` (`{"items": […]}`): return at most four qrate-generated 512-pixel PNG derivatives by source row/page.
- `stage-findings` (`{"revision": …, "findings": […]}`): publish a complete advisory draft batch.

```sh
echo '{"source":{"kind":"selected_rows"},"select":["Title"],"limit":10}' | qrate agent query --agent codex
```

Queries default to 20 and allow at most 50 records. Collection results send one `fields` array and positional `items`, plus `returned`, `remaining`, `truncated`, and `next_cursor`. Ask for the smallest useful field set, and follow a cursor only when more evidence could change the answer.

Programs have no network, filesystem, process, clock, randomness, plugin storage, UI, raw qrate objects, or staging access. Their only linked-file methods are qrate-resolved bounded UTF-8 reads and PDF text search. Original paths and bytes are never returned.

Every staged finding needs the exact response `revision`, source `row`, `column`, severity, one-sentence message, and exact current whole-cell `expected`. `replacement` is optional and must be the whole proposed value. A stale cell is rejected. Never report that staging corrected data.

The contract is `crates/ai/src/agent.rs`. The public command is `crates/cli/src/main.rs`, the private transport is `crates/app/src/app_control.rs`, the live adapter is `crates/table/src/agent.rs`, the private runner is `crates/plugin-host/src/agent_program.rs`, and the audit panel is `crates/workspace/src/panels/agent.rs`. Keep this file synchronized when those surfaces change.

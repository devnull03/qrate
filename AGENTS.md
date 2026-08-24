# Agent instructions

- **Changing qrate code?** Read [CLAUDE.md](CLAUDE.md).
- **Reviewing the project open in qrate?** Use bridge protocol 2 below. The saved `.qrate` file can omit unsaved on-screen edits.

## Live qrate bridge

qrate publishes `agent-bridge.json` in the platform application-data directory. It contains `url`, a per-launch `token`, and `bridge_protocol: 2`. If it is missing, qrate is not running or Settings ▸ Agent is off. Reject any other protocol version.

POST one JSON request to `url` with `Authorization: Bearer <token>`, `Content-Type: application/json`, and `X-Agent: codex`. The agent name is a visible label, not authority. Re-read the endpoint after `forbidden` because the token changes each launch.

The bridge never changes a cell. `stage_findings` only replaces that agent's draft findings in the Problems panel and optionally offers whole-cell replacements in the Fixes menu.

qrate's bundled Pi runs as a contained child process. Stop, Restart, panel teardown, and app exit terminate its process tree; do not rely on an older Pi process surviving a restart.

### Protocol 2 methods

- `overview`: compact project, column, selection, diagnostic-count, and revision information.
- `query`: bounded live rows or diagnostics. Sources are `all_rows`, `selected_rows`, `rows`, `search`, and filtered `diagnostics`. Operations include `select`, `where`, `distinct`, `group_by`, `order_by`, `limit`, and revision-bound `cursor`.
- `program_save`: validate and activate a confined Luau function without running it.
- `program_run`: run the saved function once against an immutable snapshot at an exact revision.
- `thumbnails`: return at most four qrate-generated 512-pixel PNG derivatives by source row/page.
- `stage_findings`: publish a complete advisory draft batch.

Queries default to 20 and allow at most 50 records. Collection results send one `fields` array and positional `items`, plus `returned`, `remaining`, `truncated`, and `next_cursor`. Ask for the smallest useful field set and follow a cursor only when more evidence could change the answer.

Programs have no network, filesystem, process, clock, randomness, plugin storage, UI, raw qrate objects, or staging access. Their only linked-file methods are qrate-resolved bounded UTF-8 reads and PDF text search. Original paths and bytes are never returned.

Every staged finding needs the exact response `revision`, source `row`, `column`, severity, one-sentence message, and exact current whole-cell `expected`; `replacement` is optional and must be the whole proposed value. A stale cell is rejected. Never report that staging corrected data.

The contract is `crates/ai/src/agent.rs`, transport is `crates/app/src/agent_bridge.rs`, live adapter is `crates/table/src/agent.rs`, private runner is `crates/plugin-host/src/agent_program.rs`, and the audit panel is `crates/workspace/src/panels/agent.rs`. Keep this file synchronized when those surfaces change.

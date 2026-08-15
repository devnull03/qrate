# Agent instructions

For any agent runtime, not just one vendor. Two unrelated jobs live here — pick the one you are
actually doing.

- **Changing qrate's code?** Read [CLAUDE.md](CLAUDE.md). It is the contributor guide: crate map,
  build and test commands, code style, git workflow. It is not Claude-specific despite the name,
  and nothing in it is repeated below.
- **Reading the metadata in a running qrate?** That is the rest of this file.

## Reading a running qrate

qrate can hand you the metadata it currently holds in memory, including cell edits the archivist
has not saved yet. This is the only route to that state — the `.qrate` file on disk is the last
save, not what is on screen.

### The bridge is read-only, and that is the point

There is no write operation in the protocol. You cannot fix a row, only say what you would change
and let the archivist decide. Never tell the user you have corrected something.

The archive is somebody's real collection. When you report a problem, quote the row index and the
column name so they can find it, and say what the data says rather than what you assume it means —
a date that looks wrong may be exactly what the source object is stamped with.

### Connecting

qrate only listens when the app was started with `QRATE_AGENT_BRIDGE=1`. If the endpoint file is
missing, say so and ask the user to relaunch qrate with that variable set — do not guess at data.

```bash
# Windows
ENDPOINT="$LOCALAPPDATA/qrate/agent-bridge.json"
# macOS: ~/Library/Application Support/qrate/agent-bridge.json
# Linux: ~/.local/share/qrate/agent-bridge.json

URL=$(sed -n 's/.*"url":"\([^"]*\)".*/\1/p' "$ENDPOINT")
TOKEN=$(sed -n 's/.*"token":"\([^"]*\)".*/\1/p' "$ENDPOINT")

ask() {
  curl -s -X POST "$URL" -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' -d "$1"
}
```

`sed` rather than a JSON parser on purpose — the endpoint file is two flat fields, and a machine
running qrate is not guaranteed to have `python` or `jq`.

The token is regenerated every launch. Re-read the file if a call starts returning `forbidden`.

If your runtime cannot run a shell, any HTTP client will do: POST the request JSON to `url` with an
`Authorization: Bearer <token>` header. The bridge ignores the method and path, so it is only HTTP
to the extent that an ordinary client can talk to it.

### The requests

One JSON object per call: `{"method": ..., "params": ...}`. `params` is omitted for the methods
that take none.

| Call | Returns |
|---|---|
| `{"method":"project_summary"}` | project name, row and column counts, whether a files folder is set |
| `{"method":"columns"}` | every column with its configured data type and notes |
| `{"method":"rows","params":{"rows":[0,1,2]}}` | those rows as column/value pairs — max 100 per call |
| `{"method":"search_rows","params":{"query":"1974","limit":20}}` | rows whose column name or value contains the query, case-insensitive |
| `{"method":"diagnostics"}` | what qrate's own validators already flagged, with severity and source |
| `{"method":"selected_rows"}` | source-row indices the archivist has selected right now |

Every response carries a `revision`. If it changes between calls, the archivist edited something
mid-review — re-read anything you are about to quote.

Row indices are **source** rows and ignore any active filter, so an index from `selected_rows` or
`diagnostics` can be passed straight to `rows`.

### Working order

1. `project_summary` and `columns` first — the column names and notes tell you what the collection
   is and what each field is supposed to hold. Review nothing before reading them.
2. `diagnostics` next. qrate already found the malformed dates and misspellings; repeating them adds
   nothing. Your value is what a validator cannot see — a title that contradicts its own date field,
   a creator spelled two ways across rows, a description that belongs to a different object.
3. Then read rows. `selected_rows` when the ask is about "these", `search_rows` when it is about a
   term, explicit indices when the diagnostics pointed at them.
4. Report by row index and column name. Separate "this is wrong" from "this looks inconsistent".

Errors come back as `{"error": ...}`: `project_unavailable` means no project is open,
`table_unavailable` means no table is loaded yet, and the rest are bad requests — check the row
count and query limits above before retrying.

The contract's types are `crates/ai/src/agent.rs`, the transport is `crates/app/src/agent_bridge.rs`,
and the live-table adapter is `crates/table/src/agent.rs`. Change any of them and this file is part
of the change.

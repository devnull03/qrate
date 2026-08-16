# The Agent panel

An external AI agent that you run yourself can read the project open in qrate. qrate
allows this by default. To stop it, open **Settings ▸ Agent** and switch off **Allow
agents to read this app**. The port closes immediately, with no relaunch needed. See
[`AGENTS.md`](../AGENTS.md) for the protocol.

qrate listens on your own machine only, behind a token that changes at every launch. A
program that could reach this connection could already read your `.qrate` file directly,
so the bridge does not widen what a local program can see. It does show unsaved edits,
which the file does not.

The **Agent** panel, in the right dock, lists everything that happened on that connection.
An agent cannot change a cell. It can only read data and stage findings that you accept or
ignore.

## How to read an entry

An entry has up to six parts:

| Part | What it tells you |
| --- | --- |
| `+2:07` | Time since the first entry of this session, in minutes and seconds. Not a clock time. |
| `claude-code` | The name the agent gave itself. See [Names are not proof](#names-are-not-proof). |
| `rows` | The method the agent called, or `connected` / `disconnected`. |
| `3 row(s)` | What the agent asked for. Absent for a method that takes no parameters. |
| `3 rows` | What qrate answered, or why it refused. |
| `4ms` | How long qrate took to answer. |

## The three kinds of entry

**An answered call** shows its result in grey. The result is a size, never your data:
`1893 rows × 32 columns`, `3 rows`, `12 diagnostics`. qrate never puts cell contents in
this list.

**A refused call** shows its reason in red. Read these first. Common reasons:

| Reason | What happened |
| --- | --- |
| `forbidden` | The caller sent a wrong token or no token. qrate makes a new token at each launch. |
| `malformed_request` | The caller sent a method or a parameter the protocol does not have. |
| `project_unavailable` | No project is open. |
| `too_many_rows`, `invalid_search_limit`, `too_many_findings` | The caller asked for more than one call permits. |

**A connect or disconnect** shows in blue. The protocol has no session: each call is one
request, one answer, and a closed socket. qrate infers both events. `connected` is the
first call from a name that passes the token check. `disconnected` is one minute of
silence from that name.

## Staged findings

`stage_findings` is the only method that changes what you see. Its result reads
`2 staged, 1 stale`.

- **Staged** findings go to the Problems panel, beside your own validators' findings. A
  finding that proposes a new value also adds it to that cell's right-click **Fixes**
  menu.
- **Stale** findings are dropped. A finding is stale when the cell no longer holds the
  text the agent read. This stops a correction to text nobody reviewed.

Staged findings are never written to the `.qrate` file. They are gone when you close the
project. A proposal changes a cell only after you click it in the Fixes menu.

## Names are not proof

The name in an entry is a label the caller chose, in an `X-Agent` header. qrate cannot
verify it. Anything that holds the token can claim any name. Use the name to tell two of
your own agents apart, not to decide whether to trust a caller.

## Copy an entry

Right-click an entry. **Copy** copies that one line. **Copy all** copies the full list.
Both give tab-separated text, which pastes into a spreadsheet as columns and into a bug
report as a readable line.

The list reads top to bottom, oldest first, and follows new entries as they arrive. It
holds the most recent 200 entries. It is in memory only, never written to your project,
and gone when you quit.

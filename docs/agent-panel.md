# The Agent panel

An AI agent that you run yourself can read the project open in qrate. It does this with the
`qrate agent` commands. qrate allows this by default. To stop it, open **Settings ▸ Agent**
and switch off **Allow agents to read this app**. qrate refuses every agent command from
then on, with no relaunch needed. See [Agents and skills](cli/agents.md) for how to set up an
agent, and [`AGENTS.md`](../AGENTS.md) for the full contract.

Only a program that runs as you, on your own machine, can ask. A program that can do that
could already read your `.qrate` file directly, so the commands do not widen what a local
program can see. They do show unsaved edits, which the file does not.

The **Agent** panel, in the right dock, has two tabs. **Terminal** runs qrate's bundled Pi agent.
**Log** lists every agent command that reached qrate. An agent cannot change a cell. It can
only read data and stage findings that you accept or ignore.

## Start Pi

Open a project, then open **Agent ▸ Terminal**. qrate resumes the Pi session for that project. Click
the **+** button (**New Pi session**) for a clean session. Right-click it for **Stop**, which ends
the process, or **Restart**, which resumes the session.

The first time, type `/login openrouter` and follow Pi's sign-in flow. Your credential is stored by
Pi in qrate's private Pi profile; qrate does not read or store it. Pi starts with OpenRouter and the
`openrouter/free` router. The terminal runs only Pi, not a general shell.

Pi can use its ordinary coding tools when you explicitly ask for coding work. It asks before every
shell command, write, or edit, and before reading outside the open project's directory. qrate's
metadata tools remain read-only and can only stage proposed findings.

## How to read an entry

An entry has up to six parts:

| Part | What it tells you |
| --- | --- |
| `+2:07` | Time since the first entry of this session, in minutes and seconds. Not a clock time. |
| `claude-code` | The name the agent gave itself. See [Names are not proof](#names-are-not-proof). |
| `query` | The command the agent ran, or `connected`. |
| `AllRows, max 20` | What the agent asked for. Absent for a command that takes no parameters. |
| `20 returned, 180 remaining` | What qrate answered, or why it refused. |
| `4ms` | How long qrate took to answer. |

## The three kinds of entry

**An answered call** shows its result in grey. The result is a size, never your data:
`1893 rows × 32 columns`, `20 returned, 180 remaining`, `2 staged, 0 stale`. qrate never
puts cell contents in this list.

**A refused call** shows its reason in red. Read these first. Common reasons:

| Reason | What happened |
| --- | --- |
| `agent access is off in Settings` | **Allow agents to read this app** is switched off. |
| `malformed_request` | The caller sent a command or a parameter that qrate does not have. |
| `project_unavailable` | No project is open. |
| `invalid_query_limit`, `too_many_query_fields`, `too_many_thumbnails`, `too_many_findings` | The caller asked for more than one call permits. |
| `stale_cursor` | The project changed since the revision the caller named, so it has to read again. |

**A first call** shows in blue as `connected`. Each `qrate agent` command is its own short
program, so there is no session to open or close. `connected` marks the first time qrate
sees a name.

## Staged findings

The `stage-findings` command is the only one that changes what you see. The log shows it as
`stage_findings`, and its result reads `2 staged, 1 stale`.

- **Staged** findings go to the Problems panel, beside your own validators' findings. A
  finding that proposes a new value also offers it under that finding in the cell's
  right-click **Problems** menu.
- **Stale** findings are dropped. A finding is stale when the cell no longer holds the
  text the agent read. This stops a correction to text nobody reviewed.

Staged findings are never written to the `.qrate` file. They are gone when you close the
project. A proposal changes a cell only after you click it in the Problems menu.

## Names are not proof

The name in an entry is a label the caller chose, with `--agent` or the `QRATE_AGENT`
environment variable. qrate cannot verify it. Any local program can claim any name. Use the
name to tell two of your own agents apart, not to decide whether to trust a caller.

## Copy an entry

Right-click an entry. **Copy** copies that one line. **Copy all** copies the full list.
Both give tab-separated text, which pastes into a spreadsheet as columns and into a bug
report as a readable line.

The list reads top to bottom, oldest first, and follows new entries as they arrive. It
holds the most recent 200 entries. It is in memory only, never written to your project,
and gone when you quit.

---
name: qrate-live-review
description: Read the metadata in a running qrate app — the open project's columns, rows, diagnostics, and current selection — with the `qrate agent` commands, review it, and stage what you found back into qrate's Problems panel and Fixes menu as drafts. Use when asked to look at, review, audit, or answer questions about the project that is open in qrate right now ("what's in my table", "review the rows I have selected", "what problems does qrate see"), rather than about files on disk. No command can change a single cell: every proposal waits for the archivist to click it.
---

# Review the project open in qrate

Read **"Reading a running qrate"** in [AGENTS.md](../../../AGENTS.md) and follow it. It covers every
command, its input, exit codes, and limits.

That file is the canonical copy because the commands are not Claude-specific — any agent runtime
can run them, and a second copy of the contract here would drift from the code the first time a
command changes.

## Declare yourself as `claude-code`

Pass `--agent claude-code` on every call. AGENTS.md says to name your runtime and cannot say what
that name is — being the Claude Code entry point, this file is the only place that knows. Without
it your calls land in the archivist's Agent panel as `unnamed agent`, and when something goes wrong
they cannot tell which agent did what.

Two exceptions:

- Running more than one review at once? Add a suffix: `claude-code/review-2`. The panel groups by
  this string, so two sessions sharing a name read as one agent.
- Never send a name that is not yours. It is a label qrate cannot verify, which is exactly why
  claiming somebody else's would be a lie the archivist has no way to catch.

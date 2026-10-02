---
name: qrate-cli
description: Drive the qrate desktop app from a shell with the `qrate` command — open a `.qrate` project or a `qrate://` link, check whether qrate is running and which project is open, and read that project's summary as JSON. Use when asked to open, launch, or check on qrate or a qrate project from a terminal ("open my catalog in qrate", "is qrate running", "which project is open"), and before any work on a live project to confirm the right one is on screen. To read rows or review the data itself, continue with the qrate-live-review skill.
---

# Use the qrate command

`qrate` is the supported way for a script or an agent to reach the qrate desktop app. It talks
to the qrate that is already running, so it sees the project on screen, unsaved edits included.

## Ask the command, not this file

Run `qrate help`, then `qrate help <command>` for the one you need. That text ships inside the
program, so it always matches the installed version. This skill lists no options on purpose: a
copy here would go stale the first time a flag changes.

## Find the command

Run `qrate version`. If the shell cannot find it:

- Inside qrate's own Agent panel, use `$QRATE_CLI`. qrate sets it to the right program.
- Windows: `%LOCALAPPDATA%\qrate\bin\qrate.exe`, or `%ProgramFiles%\qrate\bin\qrate.exe` for an
  all-users install.
- macOS: `/Applications/qrate.app/Contents/MacOS/qrate-cli`.
- Linux: `bin/qrate` in the folder the archive was extracted into.

Never run the other `qrate` executable, the one outside `bin`, to get output. It is the desktop
app and prints nothing to a terminal.

## The order to work in

1. `qrate app status`. In a pipe it prints `{"running": …, "project": …}`. It exits `0` whether
   qrate is running or not, so read the JSON.
2. If the project you need is already open, go on. If a **different** project is open, stop and
   ask the archivist first. See the rule below.
3. To open one: `qrate <PROJECT.qrate>`. It returns at once. Poll `qrate app status` until
   `project` is the path you opened; do not assume it is ready.
4. `qrate project info` for the name, path, files folder, and row and column counts.
5. To read rows, problems, or the selection, switch to the `qrate agent` commands and follow the
   **qrate-live-review** skill and `AGENTS.md`.

Do not add `--wait` unless you mean to block until the archivist quits qrate.

## Exit codes

| Exit | Meaning |
|---|---|
| `0` | Completed. The answer is on stdout. |
| `1` | Refused or failed. A refusal is JSON on stdout: `{"error": …}`. |
| `2` | Your command line or stdin was wrong. Read stderr, fix it, do not retry unchanged. |
| `3` | qrate is not running or cannot be reached. Start it, or tell the archivist. |

## Rules

- **Opening a project replaces the one on screen.** qrate saves the open project's pending edits
  and switches. Never do that to a project the archivist has open unless they asked for it.
- **Do not read the `.qrate` file to answer a question about the open project.** It is a SQLite
  file, and it can lack edits that are on screen and not saved yet. Ask the running app.
- **No command changes a cell.** Do not say that one did.
- **Name yourself.** On every `qrate agent` call, pass `--agent <your runtime>` or set
  `QRATE_AGENT`. The archivist reads that name in the Agent panel.

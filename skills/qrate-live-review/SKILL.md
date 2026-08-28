---
name: qrate-live-review
description: Read the metadata in a running qrate app — the open project's columns, rows, diagnostics, and current selection — over the local agent bridge, review it, and stage what you found back into qrate's Problems panel and Fixes menu as drafts. Use when asked to look at, review, audit, or answer questions about the project that is open in qrate right now ("what's in my table", "review the rows I have selected", "what problems does qrate see"), rather than about files on disk. The bridge cannot change a single cell: every proposal waits for the archivist to click it.
---

# Review the project open in qrate

Read **"Reading a running qrate"** in [AGENTS.md](../../AGENTS.md) and follow it. It has the
connection recipe, every request, and the review discipline. If you do not have that file, fetch it
from the repository root before going further — this skill deliberately does not restate the
protocol, so that a method which changes in qrate's code cannot leave a stale copy standing here.

## Name yourself

Send `X-Agent: <your runtime>` on every call — `claude-code`, `codex`, `pi`, whatever you actually
are. Without it your calls land in the archivist's Agent panel as `unnamed agent`, and when
something goes wrong they cannot tell which agent did what.

Two rules about that name:

- Running more than one review at once? Add a suffix: `codex/review-2`. The panel groups by this
  string, so two sessions sharing a name read as one agent reconnecting.
- Never send a name that is not yours. It is a label qrate cannot verify, which is exactly why
  claiming somebody else's would be a lie the archivist has no way to catch.

## What "files" means here

The working directory holds a project *file*; the collection is whatever that project links to.
When the archivist asks about their files, answer from the project's filename columns and the
thumbnails the bridge returns — not from a directory listing. A file the project does not name is
not part of the collection.

## The shape of a review

1. `overview` first, and keep its `revision` — staging anything later needs that exact number.
2. Read the diagnostics qrate already has before reporting a problem it found on its own.
3. Query the smallest thing that answers the question. Follow a cursor only when more rows could
   change the answer.
4. Quote the source-row index and the column name. Say what the data contains before saying what
   it means.
5. Stage one complete batch. Copy `expected` from the current whole-cell value exactly; propose a
   `replacement` only where the correction is well supported.
6. Tell the archivist the findings are drafts waiting for them. Never say a cell was changed —
   nothing you can do through this bridge changes one.

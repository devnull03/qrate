---
name: qrate-live-review
description: Read the metadata in a running qrate app — the open project's columns, rows, diagnostics, and current selection — over the local agent bridge, review it, and stage what you found back into qrate's Problems panel and Fixes menu as drafts. Use when asked to look at, review, audit, or answer questions about the project that is open in qrate right now ("what's in my table", "review the rows I have selected", "what problems does qrate see"), rather than about files on disk. The bridge cannot change a single cell: every proposal waits for the archivist to click it.
---

# Review the project open in qrate

Read **"Reading a running qrate"** in [AGENTS.md](../../../AGENTS.md) and follow it. It has the
connection recipe, every request, and the review discipline.

That file is the canonical copy because the bridge is not Claude-specific — any agent runtime can
speak to it, and a second copy of the protocol here would drift from the code the first time a
method changes.

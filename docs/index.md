# qrate docs

qrate is a desktop app for collection catalogs. It edits a project in a spreadsheet grid,
checks the data against validators, and exports it to the formats other systems expect.

## Start here

- [Projects](projects.md): create, import, and open a `.qrate` project, and add files or rows to it
- [The grid](grid.md): edit cells, group rows, search, filter, and undo
- [Files and photos](files-and-photos.md): link records to files, and view them
- [Visual search](visual-search.md): find records by what their pictures show
- [Diagnostics](diagnostics.md): the Problems panel, spelling, and fixes
- [Columns](columns.md): column types, authority lists, and per-column settings
- [Export and Google Sheets](export-and-sync.md): CSV, Excel, JSON-LD, CSL-JSON, ZIP, and Sheets sync
- [The Agent panel](agent-panel.md): how a local AI agent reads a project

## Plugins

- [Plugins](plugins/index.md): find, install, and manage plugins
- [Develop plugins](plugins/developing.md): create, test, and publish a plugin
- [Plugin API reference](plugins/api-reference.md): hooks, host functions, and declarations
- [Islandora plugin](plugins/islandora.md): use Islandora vocabularies in qrate

## For contributors

Start with [CONTRIBUTING.md](../CONTRIBUTING.md). [Setup and releases](dev/SETUP.md) covers
local prerequisites, CI, and the release runbook, and [Embedded Pi agent](dev/agent-runtime.md)
covers the bundled agent. The rest of [`docs/dev`](dev) holds design notes and plans.

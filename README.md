# qrate - Digital Archival Workspace

A local-first workspace designed to streamline the cataloging and description of cultural heritage materials.

## About

qrate replaces the fragmented toolchain archivists currently use—bouncing between file browsers, image viewers, spreadsheets, and task trackers—with a unified, high-performance environment for managing cultural heritage metadata.

### The Problem

Archivists often deal with thousands of images and documents when cataloging collections. Managing description metadata across these files typically involves:

1. Opening files in a separate viewer
2. Typing data into an Excel spreadsheet
3. Checking external websites (Library of Congress, Getty) for standardization
4. Manually tracking progress across multiple tools

This **context switching** increases errors and limits throughput. Traditional CSV editors also struggle with large datasets, often crashing or becoming unresponsive.

### The Solution

qrate unifies these tasks into a single workspace. The application is database-backed, using SQLite with virtual scrolling to handle large datasets efficiently without consuming excessive memory. The unified interface allows archivists to view media files directly beside the metadata grid with resizable panels, eliminating the need to switch between multiple applications. Built-in validation tools including spellcheck and an annotation system help maintain data quality throughout the cataloging process. The system is designed to support archival standards like RAD (Rules for Archival Description) and MODS, making it suitable for professional archival work. Following a local-first philosophy, all data is stored locally in open SQLite format to ensure long-term access and data sustainability.

## Key Features

### Data Management

The application efficiently handles large datasets through virtual scrolling, loading only the visible rows at any given time. Every edit is automatically saved and instantly persisted to disk using ACID transactions, protecting your work from data loss. A background thumbnail pipeline generates optimized thumbnails for image previews, ensuring fast loading times when browsing through collections. The flexible import system allows you to convert existing CSV files into the .qrate format while preserving your data structure.

### Workspace

The workspace features a fully customizable layout with resizable panels including a left sidebar, right details panel, and bottom panel. The row details panel provides an efficient way to view and edit field values inline using keyboard shortcuts, streamlining the data entry process. An integrated image viewer allows you to preview images with thumbnail caching for faster loading, keeping media files in context while you work. The interface supports both dark and light themes with system-aware detection and manual override options.

### Archival Workflow

Column widths and layout configurations persist across sessions, allowing you to maintain your preferred workspace setup. The system is ready to support archival standards like RAD, MODS, and other metadata schemas used in cultural heritage institutions. Inline editing capabilities let you modify cells directly with keyboard shortcuts for rapid data entry. Built-in spelling validation with custom dictionary support helps maintain consistent terminology across your catalog. The annotation system allows you to add notes and comments to individual cells, providing a built-in review tracking mechanism for quality control.

## Quick Start

### Opening Files

To import a CSV file, use the File menu or welcome screen to convert your existing CSV into the .qrate format. Simply select your CSV file and choose where to save the new .qrate database file. If you're working with a previously created database, use the File menu or welcome screen to open an existing .qrate file. To start fresh, create a new blank workspace through the File menu, then choose a save location and configure your column structure.

### Working with Data

Double-click any cell to begin editing—your changes are saved automatically as you work. Column widths can be adjusted by dragging the column borders; these preferences persist across sessions. The virtual scrolling system allows smooth navigation through large datasets. When you select a row, the image viewer displays associated media files so your metadata and media remain in context.

### Keyboard Shortcuts

| Action | Shortcut |
|--------|----------|
| Toggle Details Panel | Ctrl+K |
| Save Edit | Ctrl+Enter |
| Cancel Edit | Escape |
| Alt+Click Field | Edit field in details panel |

### Development / Run Locally (quick)

This project uses Svelte 5, Vite, Tailwind v4, and Tauri (see `package.json` for exact versions). We standardize on `pnpm` for package management.

Prerequisites
- Node.js (recommended LTS; test with the version your environment requires)
- Rust toolchain (for building Tauri: stable + required targets for Windows if using MSVC)
- `pnpm` installed globally (we enforce `pnpm` workflows)

Quick local steps
1. Clone the repo:
   - git clone <repo-url>
   - cd qrate
2. Install dependencies:
   - pnpm install
3. Run the frontend in dev mode:
   - pnpm dev
   - (If you use workspace or filters, run the appropriate pnpm command for your setup; check `package.json` scripts.)
4. Build for production:
   - pnpm build
   - For Tauri packaging, follow the Tauri docs (ensure Rust + Tauri CLI installed)

Windows / Tauri note
- On Windows you may need the MSVC toolchain (Visual Studio Build Tools) and the appropriate Rust targets; see the Tauri docs for platform-specific steps.

Refer to CONTRIBUTING.md for the full development setup, CI, and packaging instructions.

## .qrate File Format

A .qrate file is a SQLite database stored in a hidden folder structure:

```
project.qrate          (marker file)
.project.qrate/        (hidden folder)
├── data.db            (SQLite database)
├── data.db-wal        (write-ahead log)
└── data.db-shm        (shared memory)
```

This structure keeps the working files hidden while presenting a clean single-file interface.

## Architecture Overview

qrate uses a virtual-scrolling frontend that requests only visible rows while a Rust backend services data, layout, thumbnails, and optional AI features. The backend organizes responsibilities into modular subsystems and exposes a compact Tauri IPC surface for frontend commands:

```
┌──────────────────────────────────────────────────────────────┐
│                     Svelte 5 Frontend                        │
├─────────────────┬────────────┬─────────────────┬─────────────┤
│ WorkbenchLayout │ RevoGrid   │ RowDetailsPanel │ ImageViewer │
│ (Resizable)     │ (Virtual)  │ (Inline Edit)   │ (Thumbnails)│
├─────────────────┴────────────┴─────────────────┴─────────────┤
│                    Reactive Stores                           │
│  qrateStore │ layoutStore │ appSettings │ globalSettings     │
├──────────────────────────────────────────────────────────────┤
│                    Tauri IPC (JSON)                          │
├──────────────────────────────────────────────────────────────┤
│                     Rust Backend                             │
├──────────────┬───────────────┬───────────────────┬───────────┤
│ AppState     │ LayoutManager │ ThumbnailPipeline │ Settings  │
│ (Connections)│ (Persistence) │ (Image Processing)│ (Schema)  │
├──────────────┴───────────────┴───────────────────┴───────────┤
│                 SQLite (rusqlite + WAL)                      │
├──────────────────────────────────────────────────────────────┤
│                    .qrate File                               │
└──────────────────────────────────────────────────────────────┘
```

- Frontend: Svelte 5 UI, RevoGrid for the high-performance grid, resizable workbench layout, and a details panel that is authored as a layout panel.
- Stores: reactive stores manage file state, layout state, app settings, and global settings.
- IPC: Tauri invokes send structured JSON payloads to the backend for file I/O, layout persistence, thumbnails, spellcheck, annotations, and optional AI-assisted suggestions.
- Backend modules:
  - App state / connection pool: manages SQLite connections per open project.
  - File module: file & project creation, CSV import, project open/close, paginated row fetches and updates.
  - Layout module: layout persistence and region management (layout state moved into a dedicated layout module that persists window/region configurations).
  - Compression / thumbnail pipeline: background pipeline that generates cached thumbnails on disk for fast image preview; supports cancellation and cache lookup.
  - Annotations / spellcheck: cell annotation CRUD and dictionary-backed spell checking.
  - AI module (optional): a provider trait with implementations for mock and external providers (for example, Cohere) that can be used to suggest metadata or generate draft descriptions.
  - Settings and schema: project and global settings schemas with validation.

Persistent storage remains SQLite (rusqlite + WAL) inside the `.project.qrate` folder structure; the backend emphasizes short transactions and fast, immediate persistence for edits.

Additional architectural view (alternate layout-focused table)

```
┌─────────────────────────────────────────────────────────────┐
│                         Layout Stack                        │
├────────────────────────┬────────────────────────────────────┤
│ Panels & Regions       │ RowDetailsPanel (now a panel)      │
│ (resizable, persisted) │ ImageViewer (thumbnail-driven)     │
├────────────────────────┼────────────────────────────────────┤
│ Persistence            │ Layout DB (per-window layout)      │
│ (quick writes)         │ SQLite (separate layout DB file)   │
├────────────────────────┼────────────────────────────────────┤
│ Background services    │ Thumbnail pipeline, AI provider    │
│ (async processors)     │ (provider trait with mock/cohere)  │
└────────────────────────┴────────────────────────────────────┘
```

### Data Flow

1. **Virtual Scrolling**: Frontend requests only visible rows in batches
2. **Immediate Persistence**: Cell edits trigger Rust commands that execute SQLite transactions
3. **Layout Persistence**: Panel sizes and visibility saved to a separate layout database
4. **Thumbnail Pipeline**: Background processing generates and caches image thumbnails

## Configuration

### Project Settings

Each .qrate file maintains its own project-specific settings. The Files Folder setting specifies the base directory containing your referenced media files. The File Path Pattern provides a template for locating files using variables like `{files_folder}` and `{file_column}`. You can designate which column contains filenames through the File Column Name setting. The Use Thumbnails Only option determines whether to load compressed previews or full resolution images. The Row Limit controls how many rows are loaded per batch during virtual scrolling, allowing you to balance performance with your workflow needs.

### Global Settings

Application-wide settings control the default behavior across all projects. The Theme setting allows you to choose between System, Light, or Dark mode. Default Row Limit sets the default batch size when creating new projects. The Default File Path Pattern establishes the standard template for file location in newly created workspaces.

## Current State (v0.0.1-dev)

### Implemented

The current version includes a SQLite backend with ACID transactions to ensure data integrity, paired with virtual scrolling through RevoGrid for efficient handling of large datasets. CSV import functionality allows you to convert existing spreadsheets into the .qrate file format. The auto-save system provides immediate persistence for every edit, while column metadata persistence ensures your workspace configuration is maintained across sessions. The application supports both dark and light themes with system integration. An integrated image viewer with thumbnail caching provides fast previews of media files. The resizable layout system persists your panel configurations, and the details panel enables inline field editing for efficient data entry. Additional quality control features include spellcheck integration and a cell annotations system for tracking review notes.

### Implemented

- Storage & grid
  - SQLite-backed project files (WAL mode) with immediate persistence for cell edits.
  - Virtualized grid using RevoGrid to support very large row counts with low memory overhead.
- File & project operations
  - Create/open/close project flows and CSV import that convert spreadsheets into the `.qrate` format.
  - Paginated row fetching and efficient single-cell updates to minimize transaction scope.
- Layout & UI
  - Resizable workbench layout with panel persistence. Layout persistence is handled by the layout module; panels (including the details panel) are authored as layout panels for better composability.
  - Details panel (row inspector) integrated into the layout panels system and accessible via keyboard shortcuts.
  - New small UI components (e.g., `radio-group`) to standardize form inputs.
- Media & thumbnails
  - Background thumbnail pipeline that produces cached thumbnails on disk; thumbnail commands support start/cancel and cache lookup.
  - Integrated image viewer reads thumbnail paths to provide fast previews while browsing large collections.
- Quality & metadata tools
  - Spellcheck hooks with custom dictionary support and an annotations system for per-cell review notes.
  - An AI abstraction layer and pluggable providers (a development/mock provider plus an example external provider) for optional metadata suggestions and assisted workflows.
- Observability & commands
  - Tauri commands cover file operations, layout operations, thumbnail pipeline control, settings CRUD, spellcheck, and annotations. Command signatures are defined in the `src-tauri` Rust modules and are the authoritative interface the frontend calls.

Repository diffs (docs-commit -> HEAD)

The commits and file-level changes between the commit that last modified the docs and the current HEAD are summarized below. Use the commands in the following "Developer diff commands" block to reproduce the diffs locally.

Commits in range (most recent first)
- 547f828 — 2025-12-05 — feat: bunch of final build fixes
- cd47ad6 — 2025-12-05 — Merge pull request #11 from devnull03/ai
- 7dd9ef9 — 2025-12-05 — Update src/lib/components/layout/panels/RowDetailsPanel.svelte
- af244fc — 2025-12-05 — Update src/lib/components/layout/panels/RowDetailsPanel.svelte
- 27be162 — 2025-12-06 — feat: add CSV filesFolder validation and use cross-platform path joining
- c87a4fc — 2025-12-05 — Update src-tauri/Cargo.toml
- ed50299 — 2025-12-05 — Update src-tauri/Cargo.toml
- 7a15b6c — 2025-12-05 — feat: proper project management
- 7f900c5 — 2025-12-05 — feat: base ai stuff
- 7f85bd2 — 2025-12-05 — Working AI-suggestion side panel; clicking 'accept' moves the text for a given row to the corresponding row on the 'Details' panel. All panels can be properly resized.
- a67f464 — 2025-12-04 — reat: restructuring---

Summary of file changes (src / src-tauri)
- Added AI subsystem under `src-tauri/src/ai/`:
  - `mod.rs` (module entry)
  - `providers/cohere.rs` (external provider example)
  - `providers/mock.rs` (development/mock provider)
  - `providers/mod.rs`
  - `traits.rs` (provider trait definitions)
- Compression / thumbnail pipeline updates:
  - `src-tauri/src/compression/cache.rs` (modified)
  - `src-tauri/src/compression/commands.rs` (modified)
  - `src-tauri/src/compression/mod.rs` (modified)
  - `src-tauri/src/compression/processor.rs` (modified)
- Database & file I/O:
  - `src-tauri/src/database.rs` (modified — project creation/loading improvements)
  - `src-tauri/src/file/commands.rs` (modified — create/open/import/get_rows/update_cell flows)
  - `src-tauri/src/error.rs` (new centralized error module)
- Layout refactor:
  - `src-tauri/src/layout/mod.rs` (new/modified)
  - `src-tauri/src/layout/commands.rs` (modified)
  - `src-tauri/src/layout_state.rs` (removed)
- Tauri config & manifests:
  - `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json` (modified)
  - `src-tauri/src/lib.rs` (command registration adjusted)
  - `src-tauri/src/settings.rs` (modified)
  - `src-tauri/src/window/commands.rs` (modified)
- Frontend component moves and additions:
  - `src/lib/components/layout/panels/RowDetailsPanel.svelte` (added; details panel authored as a layout panel)
  - `src/lib/components/RowDetailsPanel.svelte` (removed from old location)
  - `src/lib/components/ui/radio-group/` (new radio-group component)
  - `src/lib/components/grid/RevoGrid.svelte` (modified RevoGrid wrapper)
  - `src/lib/components/SimpleTitleBar.svelte`, `src/lib/components/TitleBar.svelte` (modified)
  - `src/lib/components/projects/ImportWizard.svelte` (modified)
  - `src/lib/services/menu/viewMenu.ts` (modified)
  - `src/lib/stores/qrateStore.svelte.ts` (store API/behavior updates)
  - `src/routes/+page.svelte`, `src/routes/projects/+page.svelte`, `src/routes/settings/+page.svelte` (route updates reflecting layout & panel changes)

Representative repository tree (top-level relevant subset)
```/dev/null/tree.txt#L1-40
src/
├── lib/
│   ├── components/
│   │   ├── layout/
│   │   │   └── panels/
│   │   │       └── RowDetailsPanel.svelte
│   │   ├── grid/
│   │   │   └── RevoGrid.svelte
│   │   ├── projects/
│   │   │   └── ImportWizard.svelte
│   │   └── ui/
│   │       └── radio-group/
│   │           ├── index.ts
│   │           ├── radio-group.svelte
│   │           └── radio-group-item.svelte
│   └── stores/
│       └── qrateStore.svelte.ts
└── routes/
    ├── +page.svelte
    ├── projects/
    └── settings/
src-tauri/
├── src/
│   ├── ai/
│   │   ├── mod.rs
│   │   ├── providers/
│   │   │   ├── cohere.rs
│   │   │   └── mock.rs
│   │   └── traits.rs
│   ├── compression/
│   ├── file/
│   ├── layout/
│   └── database.rs
└── tauri.conf.json
```

Developer diff commands

Run these commands locally (from the repository root) to reproduce the exact commit and file diffs. Replace the long commit hash below with the commit that last modified the docs if needed.

```/dev/null/git-commands.sh#L1-6
# fetch latest and show commits between docs commit and HEAD for src paths
git fetch origin
git log --pretty=oneline 54669a6852463b1ed23f32e6f6e76110a93d7c08..HEAD -- src src-tauri

# show file change statuses between the same range
git diff --name-status 54669a6852463b1ed23f32e6f6e76110a93d7c08..HEAD -- src src-tauri

# show the full diff for src and src-tauri
git diff 54669a6852463b1ed23f32e6f6e76110a93d7c08..HEAD -- src src-tauri
```

Notes
- The Tauri Rust modules are the authoritative source for available IPC command names and parameter shapes; consult `src-tauri/src/lib.rs` and the respective modules for exact signatures.
- The AI provider layer is implemented as a pluggable trait with a mock provider for local testing and an example external provider (Cohere) for integration testing; provider configuration and activation are controlled through the backend settings module.
- For a per-commit changelog rather than this summary, run the `git log` command above and inspect each commit's diff with `git show <commit>`.

### Planned Features

Future development will focus on enhancing workflow capabilities and archival standards support. Undo/Redo support will provide more flexible editing options. Server-side sorting and filtering will enable more sophisticated data exploration. Export to CSV functionality will allow you to share data with other systems. Batch operations will enable efficient multi-row editing for related records. AI-powered metadata suggestions will help draft initial descriptions while maintaining human oversight. RAD and MODS template support will provide structured frameworks for archival description. Controlled vocabulary integration with systems like LCSH and TGM will improve standardization. A grid view for thumbnails will offer an alternative visualization mode, and automatic EXIF metadata extraction will capture technical information from image files.

## Design Philosophy

qrate follows a local-first approach where your data lives on your machine in open, accessible formats rather than locked in proprietary cloud systems. The system prioritizes quality over raw speed, providing built-in validation and review tools that help maintain data integrity throughout the cataloging process. By using SQLite as the storage format, the application ensures long-term accessibility of your data—SQLite databases can be read decades from now without proprietary software. The design is fundamentally human-centered, providing tools that assist and augment the archivist's expertise rather than attempting to replace professional judgment with automation.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for full development setup, architecture notes, and contribution guidelines. Below are short, important rules and onboarding notes to follow before opening a PR:

Key contributor rules (summary)
- Use `pnpm` for all package management and scripts. Do not use `npm` or `yarn` unless a CI job explicitly requires it.
- Prefer Tailwind for styling. When using Tailwind, keep class usage minimal and prefer utility composition patterns.
- For Svelte code in this repo follow the project's conventions: prefer `onclick` attribute usage where components expect it and reuse existing UI components in `lib/components/ui` (shadcn components) instead of creating new button/element variants.
- When editing code, prefer diffs for reviewable changes and keep PRs small and focused.
- Do not create new `.md` files unless explicitly requested. (This repository prefers centralized docs; exceptions must be discussed in an issue.)
- After making changes, include an `@Rules Cleanup` note in the final commit message and/or PR description to indicate you verified rule compliance.

PR checklist (minimum)
- All lint and type checks pass (run `pnpm check`).
- Manual smoke test of the feature (brief steps in PR description).
- Update README/CONTRIBUTING changelog if the change affects setup or public behavior.
- Reference the issue the PR addresses (if any) and add reviewers as needed.

If you're new and want a simple first task, check issues labeled `good first issue` or `help wanted`, then open a draft PR to start discussion.

For full contributor workflow, command examples, and Tauri command references, read CONTRIBUTING.md.

## License

MIT

## Credits

Built with [Tauri](https://tauri.app), [Svelte 5](https://svelte.dev), [RevoGrid](https://rv-grid.com), [shadcn-svelte](https://shadcn-svelte.com), and [SQLite](https://sqlite.org).

---
**Built for archivists who care about cultural heritage**

@Rules Cleanup
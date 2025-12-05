# qrate - Digital Archival Workspace

A local-first, high-performance workspace designed to streamline the cataloging of cultural heritage materials.

## About

qrate replaces the fragmented toolchain archivists currently use—bouncing between file browsers, image viewers, spreadsheets, and task trackers—with a unified, high-performance environment for managing cultural heritage metadata.

### The Problem

Archivists often deal with thousands of images (e.g., near-duplicate photos of the same event). Managing description metadata across these files typically involves:

1. Opening an image in a separate viewer
2. Typing data into an Excel spreadsheet
3. Checking external websites (Library of Congress, Getty) for standardization
4. Manually tracking progress across multiple tools

This **context switching** increases errors ("naming drift") and severely limits throughput. When working with large collections, the tools themselves become bottlenecks—traditional CSV editors crash or freeze with datasets over 10,000 rows.

### The Solution

qrate unifies these tasks into a single, high-performance flow:

- **Headless Document Model**: Treats CSVs as databases (SQLite), enabling instant loading of massive datasets without memory bloat
- **Unified Interface**: View media directly beside the metadata grid with resizable panels
- **Archival Standards**: Built with support for standards like RAD (Rules for Archival Description) and MODS
- **AI-Assisted Review**: Uses AI to draft metadata and flag inconsistencies (e.g., spelling errors, date format mismatches), which the human archivist validates
- **Local-First**: All data stored locally in open formats (CSV/SQLite) to ensure long-term access and sustainability

## Key Features

### Performance
- **Instant Load Times**: Open 1GB+ files in under 1 second
- **Memory Efficient**: Uses ~100MB RAM regardless of file size via virtual scrolling
- **Crash-Resistant**: ACID transactions protect your data
- **Auto-Save**: Every edit instantly persisted to disk
- **Thumbnail Pipeline**: Background processing generates optimized thumbnails for fast previews

### Workspace
- **Resizable Panels**: Left sidebar, right details panel, and bottom panel with persistent sizes
- **Row Details Panel**: View and edit field values inline with double-click or Alt+click
- **Image Viewer**: Preview images with optional full-resolution loading
- **Multiple Views**: Switch between spreadsheet and files grid views
- **Dark/Light Themes**: System-aware theme with manual override

### Archival Workflow
- **Rich Metadata**: Column widths and settings persist across sessions
- **Standards Support**: Ready for RAD, MODS, and other archival standards
- **Inline Editing**: Edit cells directly with Ctrl+Enter to save
- **Spellcheck**: Built-in spelling validation with custom dictionary support
- **Annotations**: Add notes and comments to cells

### Performance Comparison

| Operation | Traditional CSV Editors | qrate |
|-----------|------------------------|-------|
| Open 1GB file | 30-60 seconds | < 1 second |
| Memory usage | ~2-4GB | ~50-100MB |
| Data safety | Lost on crash | ACID protected |
| Column metadata | Lost on close | Persisted |
| Scale limit | ~50K rows | Millions of rows |

## Quick Start

### Opening Files

1. **Import CSV** - Convert existing CSV to .qrate format
   - Click "Import CSV" in sidebar
   - Select your CSV file
   - Choose where to save the .qrate file

2. **Open .qrate File** - Open previously created database
   - Click "Open .qrate File"
   - Select a .qrate file

3. **Create New** - Start with blank file
   - Click "New .qrate File"
   - Choose save location

### Working with Data

- **Edit Cells**: Double-click any cell (changes save automatically)
- **Resize Columns**: Drag column borders (width persists)
- **Navigate**: Scroll smoothly through millions of rows
- **View Images**: Select a row to preview associated media files

### Keyboard Shortcuts

| Action | Shortcut |
|--------|----------|
| Toggle Details Panel | Ctrl+K |
| Save Edit | Ctrl+Enter |
| Cancel Edit | Escape |
| Alt+Click Field | Edit field in details panel |

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

qrate uses a **Headless Document Model** where the frontend is a thin viewport and all data lives in the Rust backend:

```
┌─────────────────────────────────────────────────────────────┐
│                     Svelte 5 Frontend                       │
├──────────────┬──────────────┬──────────────┬───────────────┤
│ WorkbenchLayout │ RevoGrid   │ RowDetailsPanel │ ImageViewer │
│ (Resizable)     │ (Virtual)  │ (Inline Edit)   │ (Thumbnails)│
├─────────────────┴────────────┴─────────────────┴────────────┤
│                    Reactive Stores                          │
│  qrateStore │ layoutStore │ appSettings │ globalSettings    │
├─────────────────────────────────────────────────────────────┤
│                    Tauri IPC (JSON)                         │
├─────────────────────────────────────────────────────────────┤
│                     Rust Backend                            │
├──────────────┬──────────────┬──────────────┬───────────────┤
│ AppState     │ LayoutManager │ ThumbnailPipeline │ Settings │
│ (Connections)│ (Persistence) │ (Image Processing)│ (Schema) │
├──────────────┴──────────────┴──────────────┴───────────────┤
│                 SQLite (rusqlite + WAL)                     │
├─────────────────────────────────────────────────────────────┤
│                    .qrate File                              │
└─────────────────────────────────────────────────────────────┘
```

### Data Flow

1. **Virtual Scrolling**: Frontend requests only visible rows (~100 at a time)
2. **Immediate Persistence**: Cell edits invoke Rust command → SQLite transaction → success/error response
3. **Layout Persistence**: Panel sizes saved on drag-end to separate layout database
4. **Thumbnail Pipeline**: Background thread generates compressed previews, cached on disk

## Configuration

### Project Settings (per .qrate file)

| Setting | Description |
|---------|-------------|
| Files Folder | Base folder containing referenced files |
| File Path Pattern | Pattern for locating files (e.g., `{files_folder}/{file_column}`) |
| File Column Name | Column containing filenames |
| Use Thumbnails Only | Load compressed previews instead of full images |
| Row Limit | Number of rows to load per batch |

### Global Settings

| Setting | Description |
|---------|-------------|
| Theme | System / Light / Dark |
| Default Row Limit | Default batch size for new projects |
| Default File Path Pattern | Default pattern for new projects |

## Roadmap

### Completed
- [x] SQLite backend with ACID transactions
- [x] Virtual scrolling with RevoGrid
- [x] CSV import functionality
- [x] Auto-save on edit
- [x] Column metadata persistence
- [x] Dark/light theme support
- [x] Image viewer panel with thumbnails
- [x] Resizable layout with persistence
- [x] Inline field editing in details panel
- [x] Spellcheck integration
- [x] Annotations system

### In Progress
- [ ] Undo/Redo support
- [ ] Server-side sorting and filtering
- [ ] Export to CSV
- [ ] Batch operations (multi-row edit)

### Planned
- [ ] AI-powered metadata suggestions
- [ ] RAD/MODS templates
- [ ] LCSH/TGM vocabulary integration
- [ ] Thumbnail grid view
- [ ] EXIF metadata extraction

## Philosophy

- **Throughput**: Batch operations allow updating multiple related items simultaneously
- **Quality**: Inline validation against controlled vocabularies
- **Sustainability**: Open data formats ensure long-term access
- **Human-in-the-Loop**: AI assists but doesn't replace the archivist's expertise

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

MIT

## Credits

Built with [Tauri](https://tauri.app), [Svelte 5](https://svelte.dev), [RevoGrid](https://rv-grid.com), [shadcn-svelte](https://shadcn-svelte.com), and [SQLite](https://sqlite.org).

---

**Built for archivists who care about cultural heritage**
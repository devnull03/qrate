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
- **Unified Interface**: View media directly beside the metadata grid
- **Archival Standards**: Built with support for standards like RAD (Rules for Archival Description) and MODS
- **AI-Assisted Review**: Uses AI to draft metadata and flag inconsistencies (e.g., spelling errors, date format mismatches), which the human archivist validates
- **Local-First**: All data stored locally in open formats (CSV/SQLite) to ensure long-term access and sustainability

## Key Features

### Performance
- **Instant Load Times**: Open 1GB+ files in under 1 second
- **Memory Efficient**: Uses ~100MB RAM regardless of file size
- **Crash-Resistant**: ACID transactions protect your data
- **Auto-Save**: Every edit instantly persisted to disk
- **Virtual Scrolling**: Smooth performance with millions of rows

### Archival Workflow
- **Rich Metadata**: Column widths and settings persist across sessions
- **Standards Support**: Ready for RAD, MODS, and other archival standards
- **Batch Operations**: Update multiple records simultaneously
- **Validation**: Built-in support for controlled vocabularies
- **Media Viewer**: View images inline while editing metadata

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

## Roadmap

### Completed
- [x] SQLite backend with ACID transactions
- [x] Virtual scrolling with RevoGrid
- [x] CSV import functionality
- [x] Auto-save on edit
- [x] Column metadata persistence
- [x] Dark/light theme support
- [x] Image viewer panel

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

Built with [Tauri](https://tauri.app), [Svelte](https://svelte.dev), [RevoGrid](https://rv-grid.com), and [SQLite](https://sqlite.org).

---

**Built for archivists who care about cultural heritage**
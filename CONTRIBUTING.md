# Contributing to qrate

## Development Setup

### Prerequisites

- Node.js 18+
- Rust 1.77+
- pnpm (recommended) or npm/yarn

### Getting Started

```bash
# Clone the repository
git clone <repository-url>
cd qrate

# Install dependencies
pnpm install

# Run development server
pnpm tauri dev

# Build for production
pnpm tauri build
```

## Architecture

qrate uses a "Headless Document Model" where:

- Frontend is a **viewport** displaying only visible rows (~100 at a time)
- Backend manages all data in **SQLite** with indexed queries
- **Virtual scrolling** loads data on-demand
- **.qrate files** are SQLite databases with structured schema

```
┌─────────────────────┐
│   Svelte 5 + UI     │  ← Viewport (100 rows in memory)
├─────────────────────┤
│   RevoGrid          │  ← Virtualized data grid
├─────────────────────┤
│   Tauri IPC         │  ← JSON over native pipes
├─────────────────────┤
│   Rust Backend      │  ← Connection pool + business logic
├─────────────────────┤
│   SQLite (rusqlite) │  ← ACID transactions + indexes
├─────────────────────┤
│   .qrate File       │  ← Database on disk
└─────────────────────┘
```

## Technology Stack

### Frontend

- **Svelte 5** - Reactive UI framework with runes
- **SvelteKit** - Application framework
- **RevoGrid** - High-performance virtualized data grid
- **TailwindCSS** - Utility-first styling
- **TypeScript** - Type-safe development
- **shadcn-svelte** - UI component library

### Backend

- **Tauri v2** - Secure native application runtime
- **Rust** - Memory-safe systems language
- **rusqlite** - SQLite database driver
- **dashmap** - Concurrent connection pool
- **image** - Image loading and resizing

## Project Structure

```
qrate/
├── src/                          # Frontend source
│   ├── lib/
│   │   ├── components/
│   │   │   ├── ui/              # shadcn-svelte components
│   │   │   ├── viewers/         # Media viewer components
│   │   │   ├── grid/            # RevoGrid wrapper
│   │   │   └── layout/          # Layout components
│   │   ├── stores/              # Svelte stores
│   │   ├── services/            # API services
│   │   └── models/              # TypeScript types
│   └── routes/                  # SvelteKit routes
├── src-tauri/                   # Backend source
│   └── src/
│       ├── lib.rs               # Tauri commands (IPC)
│       ├── database.rs          # SQLite operations
│       ├── app_state.rs         # Connection pool manager
│       ├── settings.rs          # Settings management
│       └── main.rs              # Entry point
└── static/                      # Static assets
```

## Database Schema

### _meta (Workspace Settings)

```sql
CREATE TABLE _meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### _columns (Column Definitions)

```sql
CREATE TABLE _columns (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    width INTEGER NOT NULL,
    hidden INTEGER NOT NULL
);
```

### _settings (Project Settings)

```sql
CREATE TABLE _settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### data (Content)

```sql
CREATE TABLE data (
    row_id INTEGER PRIMARY KEY AUTOINCREMENT,
    [col_0] TEXT,
    [col_1] TEXT,
    ...
);
```

## API Reference

### Tauri Commands

```typescript
// File operations
invoke('create_qrate_file', { path })
invoke('open_qrate_file', { path })
invoke('close_qrate_file', { path })
invoke('import_csv_to_qrate', { qratePath, csvPath })

// Data operations
invoke('get_rows', { path, limit, offset })
invoke('update_cell', { path, rowId, columnId, value })
invoke('insert_row', { path, values })
invoke('delete_row', { path, rowId })

// Column operations
invoke('add_column', { path, column })
invoke('update_column', { path, column })

// Media
invoke('load_image', { filePath, maxWidth, maxHeight })

// Settings
invoke('get_project_settings', { path })
invoke('set_project_setting', { path, key, value })
```

## Troubleshooting

### "Failed to open database"

- Ensure file path is absolute
- Check .qrate extension
- Verify file permissions

### Grid not rendering

- Ensure SSR is disabled (`+page.ts`)
- Check browser console for errors

### Slow performance

- Reduce viewport size (default: 100 rows)
- Close unused files

### Image not loading

- Verify files folder is configured in settings
- Check file path pattern matches your file structure
- Ensure image files exist at the resolved path

## Code Style

- Use TypeScript for all frontend code
- Follow Rust conventions for backend code
- Use Prettier for formatting (run `pnpm format`)
- Use ESLint for linting (run `pnpm lint`)

## Pull Request Process

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run `pnpm check` to verify types
5. Submit a pull request

## License

MIT
# Contributing to qrate

## Development Setup

### Prerequisites

- Node.js 18+
- Rust 1.77+
- pnpm (required)

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

# Format code
pnpm format

# Lint
pnpm lint

# Type check
pnpm check
```

## Architecture

qrate uses a **Headless Document Model** where the frontend is a thin viewport displaying only visible data, while the Rust backend manages all data in SQLite.

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

1. **Virtual Scrolling**: Frontend requests only visible rows (~100 at a time) via `get_rows`
2. **Immediate Persistence**: Cell edits → Tauri IPC → Rust command → SQLite transaction → response
3. **Layout Persistence**: Panel sizes saved on drag-end to separate layout database
4. **Thumbnail Pipeline**: Background thread generates compressed previews, cached on disk

## Technology Stack

### Frontend

| Technology | Purpose |
|------------|---------|
| Svelte 5 | Reactive UI with runes ($state, $derived, $effect) |
| SvelteKit | Application framework (SSR disabled for Tauri) |
| RevoGrid | High-performance virtualized data grid |
| TailwindCSS | Utility-first styling |
| shadcn-svelte | UI component library (Button, Resizable, Dialog, etc.) |
| paneforge | Resizable panel primitives |
| TypeScript | Type-safe development |

### Backend

| Technology | Purpose |
|------------|---------|
| Tauri v2 | Secure native application runtime |
| Rust | Memory-safe systems language |
| rusqlite | SQLite database driver with connection pooling |
| dashmap | Concurrent hashmap for connection pool |
| image | Image loading, resizing, and format conversion |
| hunspell-rs | Spellcheck integration |

## Project Structure

```
qrate/
├── src/                              # Frontend source
│   ├── lib/
│   │   ├── components/
│   │   │   ├── ui/                   # shadcn-svelte components
│   │   │   ├── viewers/              # ImageViewer, etc.
│   │   │   ├── grid/                 # RevoGrid wrapper
│   │   │   ├── layout/               # WorkbenchLayout, sidebars, panels
│   │   │   ├── chat/                 # AI chat components
│   │   │   └── projects/             # Project browser
│   │   ├── stores/
│   │   │   ├── qrateStore.svelte.ts  # Main data store (rows, columns, selection)
│   │   │   ├── layoutStore.svelte.ts # Panel visibility and sizes
│   │   │   ├── appSettings.ts        # Project settings
│   │   │   └── globalSettings.ts     # Global app settings
│   │   ├── services/
│   │   │   ├── menu/                 # Menu bar integration
│   │   │   ├── annotations/          # Cell annotations
│   │   │   ├── settings/             # Settings UI logic
│   │   │   └── thumbnails/           # Thumbnail URL resolution
│   │   ├── models/                   # TypeScript types
│   │   └── utils/                    # Utility functions
│   └── routes/                       # SvelteKit routes
│       ├── +page.svelte              # Main editor view
│       ├── projects/                 # Project browser window
│       ├── settings/                 # Settings window
│       └── chat/                     # Detached chat window
│
├── src-tauri/                        # Backend source
│   └── src/
│       ├── lib.rs                    # Tauri command registration
│       ├── main.rs                   # Entry point
│       ├── app_state.rs              # Connection pool (DashMap)
│       ├── database.rs               # SQLite operations
│       ├── settings.rs               # Settings schema and validation
│       ├── layout_state.rs           # Layout state management
│       ├── file/                     # File operations module
│       │   └── commands.rs           # create, open, import, get_rows, update_cell
│       ├── layout/                   # Layout persistence module
│       │   ├── commands.rs           # get_layout, save_layout, toggle_region
│       │   ├── manager.rs            # LayoutManager (SQLite-backed)
│       │   └── persistence.rs        # Layout database path
│       ├── compression/              # Thumbnail pipeline module
│       │   ├── commands.rs           # start_thumbnail_processing, get_thumbnail_path
│       │   ├── pipeline.rs           # Async processing pipeline
│       │   ├── processor.rs          # Image resize/compress
│       │   └── cache.rs              # Disk cache management
│       ├── window/                   # Window management module
│       │   ├── commands.rs           # create_window, show_settings_window
│       │   └── manager.rs            # WindowManager
│       ├── checks/                   # Validation module
│       │   └── spellcheck.rs         # Hunspell integration
│       └── annotations/              # Annotations module
│           └── commands.rs           # CRUD for cell annotations
│
└── static/                           # Static assets
```

## Database Schema

### _meta (Workspace Metadata)

```sql
CREATE TABLE _meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Stores: version, created_at, etc.
```

### _columns (Column Definitions)

```sql
CREATE TABLE _columns (
    id TEXT PRIMARY KEY,      -- e.g., "col_0", "col_1"
    name TEXT NOT NULL,       -- Display name
    type TEXT NOT NULL,       -- "text", "number", etc.
    width INTEGER NOT NULL,   -- Pixel width
    hidden INTEGER NOT NULL   -- 0 or 1
);
```

### _settings (Project Settings)

```sql
CREATE TABLE _settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Stores: filesFolder, filePathPattern, fileColumnName, etc.
```

### _annotations (Cell Annotations)

```sql
CREATE TABLE _annotations (
    id TEXT PRIMARY KEY,
    row_id INTEGER NOT NULL,
    column_id TEXT NOT NULL,
    type TEXT NOT NULL,       -- "comment", "note", etc.
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
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
CREATE INDEX idx_data_rowid ON data(row_id);
```

## Tauri Commands Reference

### File Operations

```typescript
// Create new .qrate file
invoke('create_qrate_file', { path: string })

// Open existing .qrate file
invoke('open_qrate_file', { path: string }): Promise<FileOpenResponse>

// Close file and release connection
invoke('close_qrate_file', { path: string })

// Import CSV into .qrate format
invoke('import_csv_to_qrate', { qratePath: string, csvPath: string })

// Preview CSV before import
invoke('preview_csv', { path: string }): Promise<PreviewResponse>

// Fetch rows with pagination
invoke('get_rows', { path: string, limit: number, offset: number }): Promise<DataResponse>

// Update single cell
invoke('update_cell', { path: string, rowId: number, columnId: string, value: string })

// Insert new row
invoke('insert_row', { path: string, values: Record<string, string> })

// Delete row
invoke('delete_row', { path: string, rowId: number })
```

### Column Operations

```typescript
invoke('add_column', { path: string, column: ColumnDef })
invoke('update_column', { path: string, column: ColumnDef })
```

### Layout Operations

```typescript
invoke('get_layout', { windowId: string }): Promise<WindowLayout>
invoke('save_layout', { layout: WindowLayout })
invoke('update_region_size', { windowId: string, region: string, size: number })
invoke('toggle_region', { windowId: string, region: string })
```

### Thumbnail Operations

```typescript
invoke('start_thumbnail_processing', { files: string[], cacheDir: string })
invoke('cancel_thumbnail_processing')
invoke('get_thumbnail_path', { filePath: string, cacheDir: string }): Promise<string>
```

### Settings Operations

```typescript
invoke('get_global_settings_schema'): Promise<SettingDef[]>
invoke('get_project_settings_schema'): Promise<SettingDef[]>
invoke('get_project_settings', { path: string }): Promise<Record<string, string>>
invoke('set_project_setting', { path: string, key: string, value: string })
invoke('set_project_settings', { path: string, settings: Record<string, string> })
```

### Spellcheck Operations

```typescript
invoke('check_spelling', { text: string }): Promise<SpellCheckResult[]>
invoke('get_suggestions', { word: string }): Promise<string[]>
invoke('add_to_dictionary', { word: string })
invoke('ignore_word', { word: string })
```

### Annotations Operations

```typescript
invoke('get_annotations', { path: string, rowId?: number, columnId?: string })
invoke('create_annotation', { path: string, annotation: Annotation })
invoke('update_annotation', { path: string, annotation: Annotation })
invoke('delete_annotation', { path: string, id: string })
```

## Frontend Stores

### qrateStore

Main data store managing file state, rows, columns, and selection.

```typescript
class QrateStore {
    currentFilePath: string | null
    columns: ColumnDef[]
    rows: Record<string, any>[]
    totalRows: number
    selectedRowId: number | null
    isLoading: boolean
    
    async openFile(path: string): Promise<void>
    async loadRows(offset: number, limit: number): Promise<void>
    async updateCell(rowId: number, columnId: string, value: string): Promise<void>
}
```

### layoutStore

Manages panel visibility and sizes with persistence.

```typescript
class LayoutStore {
    layout: WindowLayout | null
    
    async loadLayout(windowId: string): Promise<void>
    async toggleRegion(region: string): Promise<void>
    async updateRegionSize(region: string, size: number): Promise<void>
}
```

## Adding New Features

### Adding a New Tauri Command

1. Define the command in Rust (`src-tauri/src/<module>/commands.rs`):
```rust
#[tauri::command]
pub async fn my_command(path: String) -> Result<MyResponse, String> {
    // Implementation
}
```

2. Register in `lib.rs`:
```rust
.invoke_handler(tauri::generate_handler![
    // ...existing commands
    my_module::commands::my_command,
])
```

3. Call from frontend:
```typescript
const result = await invoke<MyResponse>('my_command', { path });
```

### Adding a New Setting

1. Add to Rust settings schema (`src-tauri/src/settings.rs`):
```rust
SettingDef {
    key: "myNewSetting",
    scope: SettingScope::Project,
    setting_type: SettingType::Boolean,
    default_value: "true",
    label: "My New Setting",
    description: "Description here",
    category: "General",
    min: None,
    max: None,
    options: None,
},
```

2. Use in frontend via `appSettings.ts` or `subscribeToSettings()`.

### Adding a New UI Component

1. Use existing shadcn-svelte components from `$lib/components/ui/`
2. Follow Svelte 5 patterns: `$state`, `$derived`, `$effect`
3. Use `onclick` not `on:click` (Svelte 5 syntax)
4. Prefer Tailwind classes over custom CSS

## Code Style

- **Frontend**: TypeScript with Svelte 5 runes
- **Backend**: Rust with standard conventions
- **Formatting**: Prettier (frontend), rustfmt (backend)
- **No unnecessary comments**: Code should be self-documenting
- **No defensive try/catch**: Only catch where errors are expected

## Troubleshooting

### "Failed to open database"
- Ensure file path is absolute
- Check .qrate extension
- Verify file permissions
- Check if file is locked by another process

### Grid not rendering
- Ensure SSR is disabled in `+page.ts`: `export const ssr = false`
- Check browser console for errors

### Slow performance
- Reduce row limit in settings
- Close unused files
- Check if thumbnail processing is running

### Image not loading
- Verify Files Folder is configured in settings
- Check File Path Pattern matches your structure
- Ensure image files exist at resolved path
- Try "Load Full Image" button to bypass thumbnail

### Layout not persisting
- Check that drag completed (not cancelled)
- Verify layout database exists in app data directory

## Pull Request Process

1. Fork the repository
2. Create a feature branch from `main`
3. Make changes following code style guidelines
4. Run `pnpm check` and `pnpm lint`
5. Test with `pnpm tauri dev`
6. Submit PR with clear description

## License

MIT
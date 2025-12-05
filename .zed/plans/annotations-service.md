# Annotations Service: Comments, Problems, and AI Items for BottomPanel

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Reference: This document must be maintained in accordance with PLANS.md principles (self-contained, novice-guiding, outcome-focused).


## Purpose / Big Picture

After implementing this feature, users will be able to attach textual comments to specific cells, rows, or columns in their spreadsheet. These comments will appear in the BottomPanel as a list, grouped by location (sheet/row/column). Clicking an item in the BottomPanel will navigate the grid to that location and highlight the relevant cell or range.

The architecture is designed to be abstract so the same service can surface comments, problems (validation errors, schema violations), and eventually AI-generated suggestions. The pattern follows VS Code's approach: a single-source-of-truth service that aggregates items from multiple providers, emits change events, and exposes accessors for update/dispose.

Observable outcomes after implementation:
1. Users can right-click a cell and choose "Add Comment" to create a comment attached to that cell.
2. The BottomPanel shows a "Comments" tab listing all comments, grouped by row.
3. Clicking a comment in the BottomPanel scrolls the grid to that cell and selects it.
4. Comments are persisted in the .qrate database and survive reload.
5. The status bar shows a count of unresolved comments.
6. The same service infrastructure can be extended for "Problems" and "AI Suggestions" tabs.


## Progress

- [x] (2025-01-15) Milestone 1: Database schema for annotations — Added `_annotations` table to `database.rs` in both `init_database` and `open_database` functions with proper indexes.
- [x] (2025-01-15) Milestone 2: Rust backend annotations module with Tauri commands — Created `src-tauri/src/annotations/` module with types.rs, commands.rs, mod.rs. Registered 5 commands in lib.rs: get_annotations, create_annotation, update_annotation, delete_annotation, get_annotations_at.
- [x] (2025-01-15) Milestone 3: Frontend annotations service (TypeScript/Svelte) — Created `src/lib/models/annotations.ts` with types and `src/lib/services/annotations/` with reactive Svelte service using $state/$derived patterns. Integrated loading into qrateStore.
- [x] (2025-01-15) Milestone 4: BottomPanel tabs and comments list UI — Refactored BottomPanel.svelte to support tabs (Comments/Problems). Created CommentsPanel.svelte and ProblemsPanel.svelte in panels/ directory.
- [x] (2025-01-15) Milestone 5: Cell decoration indicators — Added cellProperties callback in RevoGrid.svelte that applies `.has-annotation` class. Added CSS for triangular corner indicator on annotated cells.
- [x] (2025-01-15) Milestone 6: Context menu integration for adding comments — Added contextmenu handler to RevoGrid with DropdownMenu. Created AddCommentDialog.svelte using Sheet component for comment input.
- [x] (2025-01-15) Milestone 7: Status bar integration — Added comments count to StatusBar.svelte with clickable button to toggle BottomPanel.
- [ ] Milestone 8: End-to-end validation — Pending manual testing.


## Surprises & Discoveries

- RevoGrid's cellProperties callback receives a data object with model containing row_id, enabling dynamic cell decoration based on annotation state.
- The DropdownMenu component from shadcn-svelte can be positioned absolutely using inline styles for context menu behavior, though it requires manually managing the open state.
- The Sheet component works well for the add comment dialog with bottom positioning.
- svelte-check reported an a11y warning for div with contextmenu handler; resolved by adding role="application".


## Decision Log

- Decision: Use a single `_annotations` table with a `provider` column to distinguish comments vs problems vs AI items.
  Rationale: This mirrors VS Code's approach where DiagnosticsService stores items with provider metadata. Allows filtering by provider in the UI and keeps the schema simple.
  Date/Author: (plan creation)

- Decision: Store cell references as (row_id, column_id) pairs rather than A1-style notation.
  Rationale: The existing database uses `row_id` (integer primary key) and `column_id` (string). Using these directly avoids parsing/formatting logic and ensures references remain valid after row insertions.
  Date/Author: (plan creation)

- Decision: Frontend service uses Svelte 5 runes ($state, $derived) following the existing statusBarService pattern.
  Rationale: Consistency with existing codebase. The statusBarService in `src/lib/services/statusbar/statusbar.svelte.ts` demonstrates the accessor pattern and reactive state management.
  Date/Author: (plan creation)


## Outcomes & Retrospective

(To be completed at end of implementation.)


## Context and Orientation

This section describes the current state of the qrate codebase relevant to this feature.

Project structure (key paths):
- `qrate/src-tauri/src/` — Rust backend with Tauri commands
- `qrate/src/lib/` — Frontend Svelte/TypeScript code
- `qrate/src/lib/services/` — Singleton service pattern (statusbar, menu, thumbnails)
- `qrate/src/lib/stores/` — Svelte 5 reactive stores (qrateStore, layoutStore)
- `qrate/src/lib/components/layout/BottomPanel.svelte` — Currently shows a hardcoded "Problems" tab with placeholder content
- `qrate/src/lib/models/statusbar.ts` — Type definitions for status bar entries and accessor pattern

Database:
- Each .qrate file is a marker; the actual SQLite database lives in a hidden folder `.<filename>.qrate/data.db`
- Existing tables: `_meta`, `_settings`, `_columns`, `data`
- The `database.rs` module provides functions like `get_columns`, `get_rows`, `update_cell`, etc.
- Row data uses `row_id` (INTEGER PRIMARY KEY AUTOINCREMENT) as the unique identifier
- Column definitions use `id` (TEXT PRIMARY KEY) as the unique identifier

Backend command pattern:
- Commands are defined in module files like `src-tauri/src/file/commands.rs`
- They use `#[tauri::command]` attribute and take `State<AppState>` for database connections
- Re-exported in `src/lib.rs` via the `invoke_handler!` macro

Frontend service pattern (from statusBarService):
- A factory function creates the service with reactive state using `$state` and `$derived`
- An accessor pattern returns `{ update, show, hide, dispose, getEntry }` objects
- Subscribers can react to state changes via exported reactive properties

Grid component:
- Uses `@revolist/svelte-datagrid` (RevoGrid)
- Cell selection tracked via `qrateStore.selectedRowId` and `qrateStore.selectedColumnId`
- Located in `src/lib/components/grid/RevoGrid.svelte`

Terms of art:
- Annotation: A piece of data (comment, problem, AI suggestion) attached to a cell/row/column reference
- Provider: The source of an annotation (e.g., "user-comment", "validation", "ai-suggestion")
- Accessor: An object returned when registering an item, providing update/dispose methods
- Reference: The location an annotation points to (cell, row, or column)


## Plan of Work


### Milestone 1: Database Schema for Annotations

Add a new `_annotations` table to the SQLite schema. This table stores all annotation types (comments, problems, AI items) distinguished by a `provider` column.

Schema:

    CREATE TABLE IF NOT EXISTS _annotations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider TEXT NOT NULL,
        row_id INTEGER,
        column_id TEXT,
        severity TEXT DEFAULT 'info',
        message TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at TEXT NOT NULL DEFAULT (datetime('now')),
        resolved INTEGER NOT NULL DEFAULT 0,
        metadata TEXT
    )

Fields explained:
- `id`: Unique annotation identifier
- `provider`: String identifying source ("user-comment", "validation", "ai")
- `row_id`: NULL for column-level annotations, or the data row's row_id
- `column_id`: NULL for row-level annotations, or the column id
- `severity`: One of "info", "warning", "error" (for problems/AI, "info" for comments)
- `message`: The annotation text content
- `created_at`/`updated_at`: ISO timestamps
- `resolved`: 0 or 1, for marking comments as resolved
- `metadata`: JSON blob for provider-specific data (author, thread replies, etc.)

When both `row_id` and `column_id` are set, the annotation targets a specific cell.
When only `row_id` is set, it targets an entire row.
When only `column_id` is set, it targets an entire column.

Files to modify:
- `qrate/src-tauri/src/database.rs`: Add `init_annotations_table` function called from `init_database` and `open_database`

Validation: After this milestone, opening a .qrate file should create the `_annotations` table. Verify with:

    sqlite3 .test.qrate/data.db ".schema _annotations"

Expected output shows the CREATE TABLE statement.


### Milestone 2: Rust Backend Annotations Module

Create a new module `qrate/src-tauri/src/annotations/` with:
- `mod.rs`: Module exports
- `types.rs`: Rust structs for annotations
- `commands.rs`: Tauri commands for CRUD operations

Types to define (in `types.rs`):

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct Annotation {
        pub id: i64,
        pub provider: String,
        pub row_id: Option<i64>,
        pub column_id: Option<String>,
        pub severity: String,
        pub message: String,
        pub created_at: String,
        pub updated_at: String,
        pub resolved: bool,
        pub metadata: Option<serde_json::Value>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct CreateAnnotation {
        pub provider: String,
        pub row_id: Option<i64>,
        pub column_id: Option<String>,
        pub severity: Option<String>,
        pub message: String,
        pub metadata: Option<serde_json::Value>,
    }

Commands to implement (in `commands.rs`):
- `get_annotations(path: String, provider: Option<String>) -> Vec<Annotation>`: List all annotations, optionally filtered by provider
- `create_annotation(path: String, annotation: CreateAnnotation) -> Annotation`: Create and return new annotation with generated id
- `update_annotation(path: String, id: i64, message: String, resolved: Option<bool>) -> Annotation`: Update annotation text or resolved state
- `delete_annotation(path: String, id: i64) -> ()`: Remove annotation

Register commands in `src-tauri/src/lib.rs` invoke_handler.

Validation: Use Tauri's invoke from browser console:

    await __TAURI__.core.invoke('create_annotation', {
      path: '<current-file-path>',
      annotation: { provider: 'user-comment', row_id: 1, column_id: 'col_0', message: 'Test comment' }
    })

Should return the created annotation object with an id.


### Milestone 3: Frontend Annotations Service

Create a new service following the statusBarService pattern.

Files to create:
- `qrate/src/lib/models/annotations.ts`: TypeScript types
- `qrate/src/lib/services/annotations/index.ts`: Re-exports
- `qrate/src/lib/services/annotations/annotations.svelte.ts`: Service implementation

Types (in `annotations.ts`):

    export type AnnotationSeverity = 'info' | 'warning' | 'error';

    export interface AnnotationReference {
      rowId: number | null;
      columnId: string | null;
    }

    export interface Annotation {
      id: number;
      provider: string;
      reference: AnnotationReference;
      severity: AnnotationSeverity;
      message: string;
      createdAt: string;
      updatedAt: string;
      resolved: boolean;
      metadata?: Record<string, unknown>;
    }

    export interface IAnnotationAccessor {
      update(message: string): Promise<void>;
      resolve(): Promise<void>;
      unresolve(): Promise<void>;
      delete(): Promise<void>;
      getAnnotation(): Annotation | undefined;
    }

Service implementation (in `annotations.svelte.ts`):

The service maintains a reactive Map of annotations keyed by id. It provides:
- `annotations`: Reactive array of all annotations
- `byProvider(provider: string)`: Derived array filtered by provider
- `byReference(ref: AnnotationReference)`: Annotations at a specific location
- `add(opts: CreateAnnotationOpts): Promise<IAnnotationAccessor>`: Create annotation, call backend, return accessor
- `get(id: number): Annotation | undefined`
- `load(path: string, provider?: string): Promise<void>`: Fetch from backend and populate state
- `clear()`: Reset state

The service calls Tauri commands (`create_annotation`, `get_annotations`, `update_annotation`, `delete_annotation`) and keeps local state in sync.

Update `qrate/src/lib/services/index.ts` to export the new service.

Validation: Import the service in a Svelte component and call `annotationsService.load(path)`. Log `annotationsService.annotations` and verify it updates reactively.


### Milestone 4: BottomPanel Tabs and Comments List UI

Transform `BottomPanel.svelte` from a single hardcoded "Problems" view to a tabbed interface.

Changes to `qrate/src/lib/components/layout/BottomPanel.svelte`:
1. Add a `tabs` prop or internal state for tab definitions (Comments, Problems)
2. Render tab buttons in the header bar
3. Track `activeTab` state
4. Render different content based on activeTab

Create `qrate/src/lib/components/layout/panels/CommentsPanel.svelte`:
- Subscribe to `annotationsService.byProvider('user-comment')`
- Group annotations by row (or column if row is null)
- Render a virtualized list (or simple list for MVP)
- Each item shows: location (e.g., "Row 5, Column Name"), message snippet, timestamp
- Clicking an item calls a navigation function

Create `qrate/src/lib/components/layout/panels/ProblemsPanel.svelte`:
- Placeholder for future implementation
- Shows "No problems detected" message

Navigation on click:
- When user clicks a comment item, emit an event or call a function that:
  1. Sets `qrateStore.selectedRowId` and `qrateStore.selectedColumnId`
  2. Scrolls the RevoGrid to that row (may require exposing a method on the grid component)

Validation: Open a file with comments (created via console invoke). The BottomPanel should show a "Comments" tab. Clicking a comment should select the cell in the grid.


### Milestone 5: Cell Decoration Indicators

Show a visual indicator on cells that have annotations. This is analogous to VS Code's gutter markers or squiggly underlines.

Approach: RevoGrid supports custom cell rendering via the `cellProperties` callback. We can add a CSS class to cells that have annotations.

Changes:
1. In `RevoGrid.svelte`, import the annotations service
2. Create a derived set of "annotated cell keys" (e.g., `${rowId}-${columnId}`)
3. In `convertColumns`, add a `cellProperties` function that checks if the cell key is in the set and returns `{ class: 'has-annotation' }` if so
4. Add CSS for `.has-annotation` (e.g., small colored dot in corner, subtle background)

Alternatively, if RevoGrid's cellProperties doesn't support dynamic updates well, we can use an overlay approach or accept that decorations refresh on grid refresh.

Validation: Create a comment on a cell. The cell should display a subtle visual indicator (e.g., small triangle in corner). Hovering could show a tooltip with the comment (future enhancement).


### Milestone 6: Context Menu Integration

Allow users to right-click a cell and choose "Add Comment" to create a new comment.

RevoGrid emits a `beforecellfocus` or similar event. We need to:
1. Listen for right-click (contextmenu event) on the grid container
2. Determine which cell was clicked (may need to store last focused cell or use event coordinates)
3. Show a context menu with "Add Comment" option
4. On selection, open a comment input dialog or inline editor
5. On submit, call `annotationsService.add(...)` with the cell reference

For simplicity in MVP:
- Use a browser-native prompt or a simple modal
- Later iterations can use a rich inline editor

Changes:
- Add contextmenu handler to grid container in `RevoGrid.svelte`
- Create a simple `AddCommentDialog.svelte` component using shadcn-svelte dialog/sheet
- Wire up the flow: right-click → show menu → click "Add Comment" → show dialog → submit → create annotation

Validation: Right-click a cell, choose "Add Comment", enter text, submit. The comment should appear in BottomPanel and the cell should show a decoration.


### Milestone 7: Status Bar Integration

Show a count of unresolved comments in the status bar.

Changes:
- In `qrate/src/routes/+page.svelte` (or wherever status bar entries are registered), add a reactive status bar entry for comments
- Use `statusBarService.addEntry(...)` with text showing count
- Subscribe to annotations service and update the entry when count changes

Entry configuration:

    statusBarService.addEntry({
      id: 'comments-count',
      text: `$(comment) ${unresolvedCount}`,
      tooltip: `${unresolvedCount} unresolved comment(s)`,
      alignment: 'right',
      priority: 50,
      command: 'workbench.panel.comments.toggle',
    });

(The `$(comment)` syntax assumes icon support; otherwise use plain text like "Comments: N".)

Register a command that toggles the BottomPanel visibility and switches to Comments tab.

Validation: Create comments. The status bar should show the count. Clicking it should open the BottomPanel to the Comments tab.


### Milestone 8: End-to-End Validation

Run through the complete user flow:
1. Open or create a .qrate file
2. Import CSV data or add rows/columns manually
3. Right-click a cell → Add Comment → Enter text → Submit
4. Verify comment appears in BottomPanel
5. Verify cell shows decoration indicator
6. Verify status bar shows count
7. Click comment in BottomPanel → verify grid navigates to cell
8. Close and reopen file → verify comments persist
9. Mark a comment as resolved (via future UI or console) → verify count updates


## Concrete Steps

Working directory for all commands: `qrate`

Step 1: Add annotations table to database schema.

    In qrate/src-tauri/src/database.rs, add after the existing table creations in init_database():

        conn.execute(
            "CREATE TABLE IF NOT EXISTS _annotations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                row_id INTEGER,
                column_id TEXT,
                severity TEXT DEFAULT 'info',
                message TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                resolved INTEGER NOT NULL DEFAULT 0,
                metadata TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_annotations_provider ON _annotations(provider)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_annotations_location ON _annotations(row_id, column_id)",
            [],
        )?;

    Also add the same statements to open_database() migration path or call a shared function.

Step 2: Create annotations module directory.

    mkdir qrate/src-tauri/src/annotations

Step 3: Create qrate/src-tauri/src/annotations/types.rs with the Rust structs.

Step 4: Create qrate/src-tauri/src/annotations/commands.rs with Tauri commands.

Step 5: Create qrate/src-tauri/src/annotations/mod.rs exporting types and commands.

Step 6: Update qrate/src-tauri/src/lib.rs to include the annotations module and register commands.

Step 7: Test backend with:

    cd qrate
    pnpm tauri dev

    In browser console:
    const path = '<your .qrate file path>';
    await __TAURI__.core.invoke('create_annotation', { path, annotation: { provider: 'user-comment', rowId: 1, columnId: 'col_0', message: 'Hello' }});
    await __TAURI__.core.invoke('get_annotations', { path });

Step 8: Create frontend types in qrate/src/lib/models/annotations.ts.

Step 9: Create qrate/src/lib/services/annotations/ directory and files.

Step 10: Update qrate/src/lib/services/index.ts to export annotationsService.

Step 11: Modify BottomPanel.svelte to support tabs.

Step 12: Create panels/ directory and CommentsPanel.svelte.

Step 13: Test tab switching and comments display.

Step 14: Add cell decorations in RevoGrid.svelte.

Step 15: Add context menu and AddCommentDialog.

Step 16: Add status bar entry.

Step 17: Full end-to-end testing.


## Validation and Acceptance

Acceptance criteria (behavior a human can verify):

1. Database schema: After opening a .qrate file, running `.schema _annotations` in sqlite3 shows the table.

2. Backend commands: Invoke commands from browser console successfully creates, lists, updates, and deletes annotations.

3. Frontend service: Importing and calling `annotationsService.load(path)` populates `annotationsService.annotations` array reactively.

4. BottomPanel tabs: Panel shows "Comments" and "Problems" tabs. Clicking switches content.

5. Comments list: Comments appear in list with location, message, and timestamp. List updates reactively when annotations change.

6. Cell navigation: Clicking a comment navigates to and selects the referenced cell in the grid.

7. Cell decoration: Cells with comments show a visual indicator (small colored marker or background tint).

8. Context menu: Right-clicking a cell shows a menu with "Add Comment". Selecting it opens a dialog to enter comment text.

9. Persistence: Comments survive closing and reopening the file.

10. Status bar: Shows unresolved comment count. Clicking toggles BottomPanel to Comments tab.


## Idempotence and Recovery

- The `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS` statements ensure the schema changes are idempotent.
- The frontend service's `load()` function replaces state, so calling it multiple times is safe.
- If a step fails partway, the database transaction model ensures partial writes don't corrupt data.
- Backend commands return errors as strings; the frontend should display them to the user without crashing.


## Artifacts and Notes

Example annotation object returned from backend:

    {
      "id": 1,
      "provider": "user-comment",
      "row_id": 5,
      "column_id": "col_2",
      "severity": "info",
      "message": "Check this value with client",
      "created_at": "2025-01-15T10:30:00Z",
      "updated_at": "2025-01-15T10:30:00Z",
      "resolved": false,
      "metadata": null
    }

Example BottomPanel tab structure:

    <div class="tabs">
      <button class:active={activeTab === 'comments'} onclick={() => activeTab = 'comments'}>
        Comments ({commentsCount})
      </button>
      <button class:active={activeTab === 'problems'} onclick={() => activeTab = 'problems'}>
        Problems ({problemsCount})
      </button>
    </div>


## Interfaces and Dependencies

Rust crate dependencies (already present in Cargo.toml):
- serde, serde_json: Serialization
- rusqlite: SQLite access
- tauri: Command framework

Frontend dependencies (already present):
- @tauri-apps/api: Invoke backend commands
- Svelte 5: Reactive state with runes
- shadcn-svelte: UI components (button, dialog)

Key interfaces to implement:

In `qrate/src-tauri/src/annotations/types.rs`:

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct Annotation { ... }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct CreateAnnotation { ... }

In `qrate/src/lib/models/annotations.ts`:

    export interface Annotation { ... }
    export interface IAnnotationAccessor { ... }

In `qrate/src/lib/services/annotations/annotations.svelte.ts`:

    export function createAnnotationsService() {
      // Returns object with: annotations, byProvider, byReference, add, get, load, clear
    }
    export const annotationsService = createAnnotationsService();

In `qrate/src/lib/components/layout/BottomPanel.svelte`:

    interface Tab {
      id: string;
      label: string;
      icon?: Component;
      count?: number;
    }

This plan follows VS Code's architecture: centralized service, accessor pattern, separation of data from presentation, and modular panel hosting.
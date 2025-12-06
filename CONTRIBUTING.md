# Contributing to qrate

Thank you for your interest in contributing to qrate. This document gives a practical, up-to-date guide for getting started, the development workflow, coding conventions, how to add features, and the PR checklist. We standardize on `pnpm` and a small set of repository conventions to keep the codebase consistent and reviewable.

Table of contents
- Development quick start (pnpm-first)
- Prerequisites
- Local development commands (concrete)
- Project layout overview
- Tauri command examples (invoke + Rust)
- Coding standards & frontend rules
- Backend / Rust guidelines
- Database & schema notes
- Testing & manual QA
- Branching, commits & PR process
- Troubleshooting and common issues
- Onboarding checklist for new contributors
- Appendix: Useful commands & references
- Footer: repository rules & @Rules Cleanup

IMPORTANT: This repository prefers centralized documentation. Do NOT add new `.md` files unless explicitly requested. When you finish any change, include an `@Rules Cleanup` note in the commit message or the PR description indicating you reviewed the repository rules.

---

Development quick start (pnpm-first)
- We use `pnpm` for package management. Do not use `npm` or `yarn` to install or run scripts unless a CI job or a maintainer explicitly instructs otherwise.
- The repository's frontend is Svelte 5 + Vite + Tailwind. The native shell is built with Tauri (Rust).

Prerequisites
- Node.js (LTS recommended; test against your environment)
- pnpm (install globally: `npm i -g pnpm` or your preferred method)
- Rust toolchain (stable) + Tauri CLI for building native artifacts
  - On Windows, ensure the MSVC toolchain (Visual Studio Build Tools) is installed if you target MSVC.
  - You may need to add target triples for other platforms when packaging.
- Git (to make branches and PRs)
- (Optional) A modern terminal and an editor with Svelte + TypeScript support (VSCode recommended)

Concrete local commands
- Clone:
  - git clone <repo-url>
  - cd qrate
- Install dependencies (root):
  - pnpm install
  - If you use workspace filtering: pnpm -w install or use workspace filters as your environment requires.
- Run frontend dev server (Vite):
  - pnpm dev
  - This calls the `dev` script defined in `package.json` (runs `vite dev`).
- Build production web artifacts:
  - pnpm build
- Run Tauri (native) dev (hot reload of Tauri + frontend):
  - pnpm run tauri dev
  - NOTE: the repo has a `tauri` script; you can pass additional args (`pnpm run tauri dev`) to start Tauri in dev mode.
- Build Tauri production bundle:
  - pnpm run tauri build
  - Follow the Tauri docs for platform-specific packaging requirements.
- Type checking:
  - pnpm run check
- Typecheck in watch:
  - pnpm run check:watch
- Preview production build (Vite preview):
  - pnpm run preview

If your environment expects `pnpm` shorthand you can also use `pnpm dev` instead of `pnpm run dev`. When invoking `tauri`, use `pnpm run tauri` to forward args like `dev` or `build`.

Project layout (high level)
- src/ - Frontend source (Svelte + TypeScript)
  - lib/components/ui/ - shadcn-svelte UI components (reuse these first)
  - lib/stores/ - qrateStore, layoutStore, appSettings, globalSettings
  - lib/services/ - thumbnails, annotations, settings, menu
  - routes/ - SvelteKit routes (+page.svelte is the main editor)
- src-tauri/ - Tauri/Rust backend
  - src/ - Rust commands and modules
    - lib.rs - command registration
    - file/ - file operations: open/create/import, get_rows, update_cell
    - compression/ - thumbnail pipeline
    - checks/ - spellcheck integration
    - layout/ - layout persistence
- static/ - static assets
- package.json - top-level scripts & deps (source of truth for local commands)

Tauri commands & frontend invoke examples
- The frontend calls Rust backend logic through Tauri `invoke`. Example usages:

- Create a new .qrate file (frontend)
```typescript
import { invoke } from '@tauri-apps/api';

await invoke('create_qrate_file', { path: '/abs/path/to/myproject.qrate' });
```

- Open an existing .qrate file
```typescript
const resp = await invoke('open_qrate_file', { path: '/abs/path/to/myproject.qrate' });
// resp: { success: boolean, metadata: {...} }
```

- Import CSV into .qrate format
```typescript
await invoke('import_csv_to_qrate', { qratePath: '/path/project.qrate', csvPath: '/path/data.csv' });
```

- Get rows (pagination)
```typescript
const rows = await invoke('get_rows', { path: '/path/project.qrate', limit: 200, offset: 0 });
```

- Update a single cell
```typescript
await invoke('update_cell', {
  path: '/path/project.qrate',
  rowId: 123,
  columnId: 'col_5',
  value: 'New value'
});
```

- Thumbnail processing
```typescript
await invoke('start_thumbnail_processing', { files: ['/path/img1.jpg','/path/img2.jpg'], cacheDir: '/tmp/qrate-thumbs' });
const thumb = await invoke('get_thumbnail_path', { filePath: '/path/img1.jpg', cacheDir: '/tmp/qrate-thumbs' });
```

Rust-side command pattern (reference)
- Add a new Tauri command in Rust:
```rust
#[tauri::command]
pub async fn my_command(path: String) -> Result<MyResponse, String> {
    // implementation
}
```
- Register the command in `src-tauri/src/lib.rs`:
```rust
tauri::generate_handler![
    // existing commands...
    my_module::commands::my_command,
]
```

Coding standards & frontend rules (enforced)
Follow these rules exactly to maintain consistency.

1. pnpm-first
   - Always use `pnpm` for dependency management, scripts, and CI workflows.

2. Tailwind-first (minimal usage)
   - Prefer Tailwind utility classes over adding new CSS files.
   - Keep class lists minimal — prefer composition and small utility helpers.
   - Avoid creating large bespoke CSS files unless absolutely necessary.

3. Component reuse
   - Reuse shadcn-svelte UI components in `src/lib/components/ui/` when possible.
   - Do not create a new button/element variant unless the existing components cannot be adapted.

4. Svelte event syntax
   - In this codebase prefer `onclick={...}` (Svelte 5 conventions) where applicable. Do not use `on:click` if the project uses `onclick` consistently.
   - Use Svelte 5 runes for reactive state: `$state`, `$derived`, `$effect`.

5. DOM structure
   - Avoid unnecessary nested `div`s. Keep markup flat and semantic when possible.

6. Accessibility
   - Ensure components are keyboard-accessible and use appropriate ARIA attributes where necessary.

7. Keep PRs small & reviewable
   - Prefer diffs for edits — make 1-2 well-scoped changes per PR.
   - Avoid large refactors in the same PR as feature changes.

8. Documentation & .md rules
   - Do not create new `.md` files unless asked. Add critical updates to `README.md` or this `CONTRIBUTING.md`.
   - Append a short "Documentation changes" note in the PR description when you change behavior that affects setup or developer workflows.

TypeScript/Svelte style (summary)
- Use TypeScript everywhere in the frontend.
- Prefer `const` over `let` where possible.
- Keep components single-responsibility and small.
- Add JSDoc comments for non-trivial functions and public stores.
- Run the type check before pushing: `pnpm run check`.

Rust style (summary)
- Follow community Rust idioms.
- Format with `rustfmt`.
- Return `Result<T, String>` with helpful error descriptions for Tauri commands.
- Use the shared `AppState` connection pool (DashMap) for database connections.

Database & schema notes
- The `.qrate` file is a SQLite DB stored in a hidden folder structure:
  - project.qrate (marker)
  - .project.qrate/
    - data.db
    - data.db-wal
    - data.db-shm
- Key tables:
  - `_meta` - workspace metadata (version, created_at)
  - `_columns` - column definitions (id, name, type, width, hidden)
  - `_settings` - project-specific settings
  - `_annotations` - cell annotations and comments
  - `data` - content rows (row_id, col_* fields)
- Prefer operating on rows using `row_id` and `column_id` pairs rather than A1-style notation.

Testing & manual QA
- Manual test workflows to run locally:
  - Open a small dataset (a few rows) and exercise editing, saving, layout persistence.
  - Open a medium dataset (thousands of rows) and confirm virtual scrolling behaves.
  - Open a large dataset (tens of thousands) to surface performance issues.
  - Test image viewer and thumbnail caching across file types.
- Suggested quick sequence before PR:
  - pnpm install
  - pnpm dev
  - Open the app and reproduce the feature flow
  - pnpm run check
  - (If linting/format scripts are present in your environment) pnpm run lint && pnpm run format

Branching, commits & PR process
- Branch naming
  - feature/<short-description>
  - fix/<short-description>
  - chore/<short-description>
- Commit messages
  - Use short, meaningful messages. Optionally follow Conventional Commits:
    - feat: add CSV export
    - fix: handle null file paths
    - docs: update README for pnpm
  - Include `@Rules Cleanup` in the PR description and final commit if you validated the repository rules.
- PR checklist (required for all PRs)
  - [ ] All type checks pass: `pnpm run check`
  - [ ] Local manual smoke test performed (brief steps added in PR description)
  - [ ] Change is limited in scope and reviewable
  - [ ] Documentation updated if setup or public behavior changed
  - [ ] Reused existing components from `lib/components/ui` when applicable
  - [ ] Added tests where reasonable (unit or integration)
  - [ ] CI (if present) passes
  - [ ] `@Rules Cleanup` included in final commit message/PR description

PR review process
- Maintainers will review within a few days. Expect iterative feedback. Address requested changes by pushing additional commits to the same branch.
- For non-trivial changes, add a short design note in the PR describing alternatives considered and reasons for the chosen approach.

Troubleshooting & common issues
- "Grid not rendering" — Ensure `ssr` is disabled for the route that uses Tauri (`export const ssr = false`), and confirm RevoGrid is initialized correctly.
- "Tauri command not found" — Confirm the command is exported in Rust `lib.rs` and signatures match frontend invokes.
- "Database locked" — Ensure there are no stale processes holding the DB open and close connections when done; prefer `close_qrate_file` command.
- "Thumbnail generation slow" — Check compression settings and that processing runs in background pipeline (not blocking main thread).
- Windows-specific Tauri notes:
  - Install Visual Studio Build Tools for MSVC or configure Rust to use GNU toolchain if desired.
  - Ensure correct Rust target and `TAURI_PLATFORM` config for packaging.

Onboarding checklist for new contributors
- [ ] Read this CONTRIBUTING.md and README.md
- [ ] Setup environment: Node, pnpm, Rust, Tauri (if building native)
- [ ] Run `pnpm install` and `pnpm dev`
- [ ] Find a `good first issue` or ask maintainers for guidance
- [ ] Open a draft PR early to discuss the approach (small diffs preferred)
- [ ] Add `@Rules Cleanup` in the final commit/PR when ready

Appendix: Useful commands summary
- pnpm install
- pnpm dev
- pnpm build
- pnpm run tauri dev
- pnpm run tauri build
- pnpm run check
- pnpm run check:watch

Security & sensitive data
- Do not commit secrets (API keys, credentials) to the repository.
- If you must use environment variables locally, add them to `.env` and update `.gitignore`. Ask maintainers for secure vaulting practices for production secrets.

Contact & getting help
- Open an issue on GitHub for bugs and feature requests.
- For architecture questions or design discussions, open a discussion or a draft PR.
- Tag maintainers in PRs or use the team communication channel (specify channel here if applicable).

Footer: repository rules & final note
- Do not create new `.md` files unless explicitly requested.
- Always use `pnpm` for package management.
- Prefer Tailwind, minimal classes, no unnecessary div nesting.
- Prefer `onclick` for Svelte 5 event handlers; reuse shadcn components under `src/lib/components/ui`.
- After completing work, include `@Rules Cleanup` in your final commit message and PR description confirming you followed these rules.

Thanks for contributing to qrate — your changes help make this a more robust tool for archivists and cultural heritage professionals.
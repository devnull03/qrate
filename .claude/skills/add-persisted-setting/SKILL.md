---
name: add-persisted-setting
description: Add or change a persisted setting in qrate — a user-wide (AppSettings) or per-project (.qrate file) preference, optionally with a toggle/field in the Settings window. Use when asked to "add a setting", "persist X", "save X to the project", "save X app-wide", "make X configurable", "add a settings toggle", or wire a value into global/project persistence. Covers the scope switcher, the read/write API, and the effective-value resolver.
---

# Add a persisted setting (global or project scope)

qrate has **two persistence stores**. This skill is the map so you don't re-derive it each time.

| Scope | Backing store | On disk | Code |
|---|---|---|---|
| **User** (app-wide) | `AppSettings` global — one JSON blob | `%LOCALAPPDATA%\qrate\settings.sqlite3`, table `settings_kv`, row key `app_settings_v1` | `crates/settings/src/lib.rs`, `crates/settings/src/db.rs` |
| **Project** (per `.qrate`) | `CurrentProject.data.values` cache → `__settings` KV table | inside the open project's `.qrate` file | `crates/settings/src/project.rs` |

Both stores hold `HashMap<String, Val>` where `Val = Text(SharedString) | Bool(bool)` (`Val::bool()` coerces `"true"/"false"` text, so bools survive as text rows). Both writers are **debounced (450ms)** and flushed synchronously on app quit. Paths below are relative to the repo root.

## The 4 pieces of any setting

1. **A key constant** — `pub const FOO_KEY: &str = "foo";`. Put it in the crate that *consumes* the value, not in `settings` (e.g. `table::TABLE_STRIPES_KEY` in `crates/table/src/lib.rs`). The Settings-window page references the same constant.
2. **A UI field** (optional) — a `Setting::{Switch,Text,Dropdown}` entry in `crates/app/src/app_settings/mod.rs::build_pages()`. This automatically routes to whichever scope the Settings window's User/Project tab has active — you write it **once**, it works in both scopes. `FilePicker`/`DirPicker` are user-only by design (a machine path isn't project data).
3. **A consumer read** — call `settings::effective_bool(FOO_KEY, cx)` (or read the store directly) where the value is used. `effective_bool` = **project value wins, else user default, else false**.
4. **A repaint trigger** — the consuming view must `cx.observe_global::<settings::AppSettings>(..)` (user-scope changes) and, if it reads project values, `cx.observe_global::<settings::project::CurrentProject>(..)` (project-scope changes; writing a project setting mutates that global, which auto-notifies). See `crates/table/src/panel.rs`.

## The read/write API (already exists — reuse it)

**User scope** (`crates/settings/src/lib.rs`):
- `AppSettings::get(cx).values.get(key).map(|v| v.bool())` — read
- `AppSettings::set_bool(key, val, cx)` / `AppSettings::set_text(key, val, cx)` — write (debounced)

**Project scope** (`crates/settings/src/project.rs`):
- `CurrentProject::get_bool(key)` — read from the in-memory cache (cheap; safe in `render`)
- `CurrentProject::set_bool(key, val, cx)` / `set_text(key, val, cx)` — updates the cache **and** queues a debounced `.qrate` write; mutating the global fires observers so readers repaint
- Low-level (rarely needed directly): `project::read_setting(path, key)` (opens a fresh RO connection — **never call per-render**), `project::queue_write(path, key, val, cx)`

**Scope-agnostic resolver** for consumers:
- `settings::effective_bool(key, cx)` — project-overrides-user fallback. This is what a feature should read; it ignores the Settings-window's active tab.

## The scope switcher (already built)

`crates/settings/src/lib.rs` owns `SettingsScope { User, Project }` + the `CurrentSettingsScope` global, and `SettingsWindow::render` shows the small text tabs (Zed-style: `foreground`+semibold when selected, `muted_foreground` otherwise) fixed at the top of the content section. The Project tab is disabled when no project is open. `Setting::Switch`/`Text`/`Dropdown` go through `scoped_bool`/`scoped_text` helpers that read/write the active scope. **You do not touch any of this to add a normal setting** — it's automatic. Only touch it if you're adding a *new field kind* or changing scoping rules.

## Worked example — a bool, both scopes, with a UI toggle

This is exactly what "table row stripes" did (see `git log` for the commit). To add `my_flag`:

```rust
// 1. key, in the consuming crate (e.g. crates/table/src/lib.rs)
pub const MY_FLAG_KEY: &str = "my_flag";

// 2. UI field, in crates/app/src/app_settings/mod.rs build_pages()
SettingGroup::new().title("Appearance").item(
    Setting::Switch {
        key: crate_that_owns::MY_FLAG_KEY,
        label: "My Flag",
        description: "What it does.",
    }
    .into(),
)

// 3. consumer read, where it's used (a Render impl, etc.)
let on = settings::effective_bool(crate::MY_FLAG_KEY, cx);

// 4. repaint trigger, in that view's constructor
let _settings_sub =
    cx.observe_global::<settings::AppSettings>(|_this: &mut Self, cx| cx.notify());
// store `_settings_sub: Subscription` on the struct so it stays alive
```

That's the whole feature. No new persistence plumbing — `set_scoped_bool` and the debounced writers already move the bytes.

## Adding a NEW field kind or a typed field (deeper)

- **A new `Setting` variant** (e.g. a slider): add the enum arm in `lib.rs`, then an arm in `impl From<Setting> for SettingItem` that builds a `SettingField::<kind>(getter, setter)`. Route getter/setter through the `scoped_*` helpers (not `AppSettings::*` directly) so it respects the User/Project tab. `SettingField` closures run at render/click time, so they can consult the scope global then.
- **A typed value that isn't a bare `Val`** (like `MainWindowBounds`): serialize to JSON and store as a `Val::Text`. It rides the existing `values` blob for user scope, or a `__settings` row for project scope — **no schema change** to `db.rs`/`project.rs`. See `SETTINGS_WINDOW_BOUNDS_KEY` (user scope) and `MAIN_WINDOW_BOUNDS_KEY` (project scope) for the pattern.
- Only touch `db.rs`'s `PersistSettings` / `SETTINGS_SCHEMA_VERSION` if you add a **dedicated struct field** on `AppSettings` (like `main_window_bounds`). Storing under `values` needs none of that.

## Verify

Run the harness after any persistence change — it's the layer these changes actually touch (SQLite round-trip: create → write `__settings` → reload → read back, plus fmt + clippy):

```bash
bash .claude/skills/add-persisted-setting/verify.sh
```

Expected tail: `test result: ok. 5 passed` then `ALL GREEN`. If you added a new project-scoped key and want a regression test, copy `load_project_file_caches_settings_values` in `crates/settings/src/project.rs` — it round-trips a `__settings` row into `ProjectData.values` with no gpui context needed.

For a UI field, also compile the app: `cargo check -p app` (gpui **won't build on Linux** — do this on Windows/macOS). There is no headless GUI harness; the manual check is: open Settings, toggle under both User and Project tabs, confirm the consumer repaints live, reopen the project to confirm the project value persisted in the `.qrate` file.

## Gotchas (things that bite)

- **Never call `project::read_setting` in `render`** — it opens a fresh SQLite connection each call. Read from the cached `CurrentProject.data.values` (via `get_bool`) instead. That cache is populated once in `load_project_file` and kept current by `set_bool`/`set_text`.
- **`effective_bool` vs `scoped_bool`**: consumers use `effective_bool` (project-overrides-user). The Settings-window checkbox uses `scoped_bool` (reflects the active tab's own store). Don't mix them up — a consumer using `scoped_bool` would flip based on which tab is open.
- **Stale Project scope**: the setters fall back to User scope if `Project` is active but no `CurrentProject` global exists, so a closed-project-with-Project-tab-selected can't panic. Keep that guard if you add new scoped setters.
- **`global_mut` auto-notifies**: gpui's `cx.global_mut::<T>()` pushes a global-observer notification, so mutating `CurrentProject`/`AppSettings` repaints observers for free — no manual plumbing.
- **`.when(..)` needs `use gpui::prelude::FluentBuilder as _;`** and `cx.theme()` needs `use gpui_component::ActiveTheme as _;` — both in `lib.rs` already.
- **User store is one JSON blob, project store is one row per key.** Different shapes; don't assume `values.len()` maps to rows for the user scope.

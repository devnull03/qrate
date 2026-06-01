# qrate

A desktop application built with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).

## Workspace

| Crate | Description |
|-------|-------------|
| `crates/app` | Main application binary — window setup, menus, workspace, status bar |
| `crates/settings` | Persisted settings: generic key-value store backed by SQLite, settings window shell, path picker widgets |
| `crates/window-wrapper` | GPUI title bar, status bar, and window-level utilities (`WindowLock`, `OpenBrowser`) |

## Development

```sh
cargo build
cargo run
```

## Stack

- **Rust** — 2024 edition
- **GPUI** — GPU-accelerated UI framework
- **gpui-component** — component library (inputs, selects, settings pages, etc.)
- **rusqlite** — settings persistence

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

## Licensing

Copyright © 2026 Arnav Mehta.

qrate is free software under the [GNU Affero General Public License v3.0](LICENSE.md).
You may download, run, study, modify, and share it at no cost — including inside an archive,
library, museum, or university. Using qrate to catalogue your collection triggers no obligation
whatsoever.

The AGPL asks one thing in return: if you distribute a modified qrate, or offer a modified qrate
to others over a network, those users get your changes under the same license. It exists to keep
qrate open, not to restrict the institutions it is built for.

**Commercial licensing.** If the AGPL does not fit — you want to build qrate into a closed
product, or ship it under terms of your own — I hold the copyright and can grant a separate
commercial license. Get in touch.

**Bundled third-party material** and its separate licenses are recorded in [NOTICES](NOTICES).

**Contributing.** Contributions are welcome, and they need a signed Contributor License Agreement
before they can be merged. This is not a formality: without one, the ability to offer the
commercial license above disappears the moment someone else's copyright enters the codebase.

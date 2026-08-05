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

**Contributing.** Contributions are welcome, with no paperwork. Open a pull request and it is
understood to be offered under the AGPL, the same license the project already carries —
contributors keep the copyright in what they write. There is no Contributor License Agreement and
no copyright assignment.

**Commercial licensing.** If the AGPL does not fit your situation, ask. Anything I hold the
copyright in I can license separately; where a part of qrate was written by someone else, that
needs their agreement too.

**Bundled third-party material** and its separate licenses are recorded in [NOTICES](NOTICES).

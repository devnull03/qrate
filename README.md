# qrate

qrate is an open-source desktop application for collection catalogs. It uses a spreadsheet grid, project metadata, linked files, validation, and exports. Archives, libraries, museums, and researchers can keep control of their data.

qrate uses [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and Rust.

## Features

- Create a blank project, import a CSV file and its folder, or start from a Google Sheet link.
- Store each project in one portable `.qrate` SQLite file.
- Edit records in a spreadsheet grid.
- Search, replace, copy, paste, undo, redo, and filter records.
- Add, remove, rename, freeze, and reorder rows and columns.
- Link records to files and photos, by exact filename or by your own pattern.
- Switch between the grid and a gallery of thumbnails.
- View images, documents, audio, and video in the details panel, and open one fullscreen to zoom, pan, page, and search inside it.
- Add cell notes and read validation problems in the Problems panel.
- Check spelling in over 60 languages, date formats, file links, and headings against LCSH, GeoNames, and Wikidata.
- Apply a suggested correction from the cell's Fixes menu.
- Set column types, descriptions, authority lists, and spell-check options for each project.
- Export data as CSV, JSON-LD, CSL-JSON, or a ZIP archive.
- Export to a new Google Sheet, or sync an existing one, after you switch it on.
- Let an AI agent that you run read the open project, and review what it read in the Agent panel.
- Use local Luau plugins for validation, column configuration, and bar items.

> **Early release:** qrate is currently `0.2.0-alpha.1`. Keep backups of important collections. Report problems with steps that reproduce the problem.

## Quick start

### Run from source

qrate uses the Rust stable toolchain. Complete these steps:

1. Install Rust with `rustup`.
2. Install the `rustfmt` and `clippy` components.
3. Clone this repository.
4. Run the application:

   ```sh
   cargo run
   ```

The first build downloads Rust dependencies. Later builds use Cargo's build cache.

The launcher can create a blank project, import a CSV file and its folder, or read a Google Sheet. The [`sample/`](sample) directory has a sample collection and photos.

### Optional preview tools

Common image formats work without extra tools. PDF previews use PDFium. Video frame previews use ffmpeg.

qrate looks beside its executable for these tools. It then looks on your system `PATH`. If qrate cannot find a tool, it shows a file-type icon.

To get the supported development binaries, run:

```sh
./scripts/fetch-binaries.sh
```

The script puts the binaries beside the executable that `cargo run` uses. See [`docs/dev/SETUP.md`](docs/dev/SETUP.md) for platform requirements and release instructions.

## Project data and privacy

A `.qrate` file is a project. It contains the collection grid, settings, notes, and other project metadata. Linked media stays in its current location. Moving a project does not copy its linked files.

qrate does not require an account or a hosted service. Google Sheets export is off until you switch it on in **Settings ▸ Google**; until then the menu does not show it. Sign-in happens on your own machine, and qrate reaches only the sheets it made or you picked. Plugins run locally. Install plugins only from sources that you trust.

## Agent panel

qrate can run its bundled Pi agent in a restricted terminal, using OpenRouter's free router by
default after you sign in. Pi can read the open project and stage findings you review in the
Problems panel; the qrate bridge can never change a cell. See
[`docs/agent-panel.md`](docs/agent-panel.md) for how to read the panel, and
[`AGENTS.md`](AGENTS.md) for the protocol.

## Development

The workspace uses Rust edition 2024. The main application crate is `crates/app`. Run these commands from the repository root:

```sh
cargo run
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings -A dead_code
```

The last three commands match the project quality checks. CI runs them on Windows, macOS, and Linux.

### Workspace

| Crate | Responsibility |
| --- | --- |
| `app` | Application setup, windows, menus, themes, logging, and exports |
| `agent-runtime` | Bundled Pi discovery, isolated profile, and restricted terminal session |
| `table` | Spreadsheet grid, editing, history, filters, notes, and file links |
| `workspace` | Panels, docks, the gallery view, and the fullscreen viewer |
| `window-wrapper` | Title bar, status bar, and window registry |
| `settings` | User settings and the `.qrate` project store |
| `project-wizard` | Launcher, recent projects, and project creation and import |
| `data-exchange` | CSV, JSON-LD, CSL-JSON, ZIP, and Google Sheets data exchange |
| `diagnostics` | Validation, spelling checks, corrections, and problems panel |
| `checks` | Date formats and authority lookups |
| `spellcheck` | Dictionaries for diagnostics |
| `plugin-host` | Luau runtime and local plugin loading |
| `plugin-api` | Types that plugins use |
| `preview` | Thumbnails, format checks, and native media preview tools |
| `ai` | Planned interfaces for AI review and embeddings |

### Plugins

qrate plugins are local folders. qrate does not download plugin packages at run time. The [qrate plugin template](https://github.com/devnull03/qrate-plugin-template) contains the host API and type definitions. Plugin authors should use that template. Do not depend on internal Rust crates.

## Documentation

User docs live in [`docs/`](docs), starting at [`docs/index.md`](docs/index.md), and cover
projects, the grid, diagnostics, columns, export, the Agent panel, and plugins.

Contributor docs live in [`docs/dev`](docs/dev):

- [`docs/dev/SETUP.md`](docs/dev/SETUP.md) — local setup, CI, and release instructions
- [`docs/dev/REPLICATION-GUIDE.md`](docs/dev/REPLICATION-GUIDE.md) — project environment setup
- [`docs/dev/plugin-systems-and-lsp.md`](docs/dev/plugin-systems-and-lsp.md) — plugin system design notes
- [`NOTICES`](NOTICES) — third-party material and notices

## Contributing

Contributions are welcome. Complete these steps before you submit code:

1. Open an issue or pull request with a clear description.
2. Run the formatting check.
3. Run Clippy.
4. Run the test suite.

When you contribute, you license your work under the AGPL-3.0. Contributors keep copyright in their work. qrate does not use a Contributor License Agreement or copyright assignment.

## License

Copyright © 2026 Arnav Mehta.

qrate is free software under the [GNU Affero General Public License v3.0](LICENSE.md). You can download, run, study, modify, and share qrate without payment. This includes use in an archive, library, museum, or university. Using qrate to catalog a collection creates no obligation.

The AGPL applies when you distribute a modified qrate. It also applies when you offer a modified qrate over a network. In both cases, give users your changes under the same license. This license keeps qrate open. It does not restrict the institutions that use it.

**Commercial licensing.** Ask if the AGPL does not fit your use case. The copyright holder can offer a separate license for their work. Other copyright holders must agree to license their work separately.

**Bundled third-party material** and its licenses are in [NOTICES](NOTICES).

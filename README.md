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

The script puts the binaries beside the executable that `cargo run` uses. See [`docs/SETUP.md`](docs/SETUP.md) for platform requirements and release instructions.

## Project data and privacy

A `.qrate` file is a project. It contains the collection grid, settings, notes, and other project metadata. Linked media stays in its current location. Moving a project does not copy its linked files.

qrate does not require an account or a hosted service. Google Sheets export is off until you switch it on in **Settings ▸ Google**; until then the menu does not show it. Sign-in happens on your own machine, and qrate reaches only the sheets it made or you picked. Plugins run locally. Install plugins only from sources that you trust.

## Agent panel

An external AI agent that you run yourself can read the project open in qrate. qrate permits this by default. To stop it, open **Settings ▸ Agent** and switch off **Allow agents to read this app**; the port closes immediately, and no relaunch is necessary. See [`AGENTS.md`](AGENTS.md) for the protocol.

qrate listens on your own machine only, behind a token that changes at every launch. A program that could reach this connection could already read your `.qrate` file directly, so the bridge does not widen what a local program can see. It does show unsaved edits, which the file does not.

The **Agent** panel in the right dock lists everything that happened on that connection. The agent cannot change a cell. It can only read data and stage findings that you accept or ignore.

### How to read an entry

An entry has up to six parts:

| Part | What it tells you |
| --- | --- |
| `+2:07` | Time since the first entry of this session, in minutes and seconds. It is not a clock time. |
| `claude-code` | The name the agent gave for itself. See *Names are not proof* below. |
| `rows` | The method the agent called, or `connected` / `disconnected`. |
| `3 row(s)` | What the agent asked for. This line is absent for a method that takes no parameters. |
| `3 rows` | What qrate answered, or why it refused. |
| `4ms` | How long qrate took to answer. |

### The three kinds of entry

**An answered call** shows its result in grey. The result is a size, never your data: `1893 rows × 32 columns`, `3 rows`, `12 diagnostics`. qrate does not put cell contents in this list.

**A refused call** shows the reason in red. Read these first. Common reasons:

| Reason | What happened |
| --- | --- |
| `forbidden` | The caller sent a wrong token or no token. qrate makes a new token at each launch. |
| `malformed_request` | The caller sent a method or a parameter that the protocol does not have. |
| `project_unavailable` | No project is open. |
| `too_many_rows`, `invalid_search_limit`, `too_many_findings` | The caller asked for more than one call permits. |

**A connect or disconnect** shows in blue. The protocol has no session: each call is one request, one answer, and a closed socket. qrate therefore infers both events. `connected` is the first call from a name that passes the token check. `disconnected` is one minute of silence from that name.

### Staged findings

`stage_findings` is the only method that changes what you see. Its result reads `2 staged, 1 stale`.

- **Staged** findings go to the Problems panel, beside your own validators' findings. A finding that proposes a new value also adds it to that cell's right-click **Fixes** menu.
- **Stale** findings are dropped. A finding is stale when the cell no longer holds the text the agent read. This prevents a correction to text that nobody reviewed.

Staged findings are never written to the `.qrate` file. They are gone when you close the project. A proposal changes a cell only after you click it in the Fixes menu.

### Names are not proof

The name in an entry is a label that the caller chose, in an `X-Agent` header. qrate cannot verify it. Anything that holds the token can claim any name. Use the name to tell two of your own agents apart, not to decide whether to trust a caller.

### Copy an entry

Right-click an entry. **Copy** copies that one line. **Copy all** copies the full list. Both give tab-separated text, which pastes into a spreadsheet as columns and into a bug report as a readable line.

The list reads top to bottom, oldest first, and follows new entries as they arrive. It holds the most recent 200 entries. It is in memory only. It is never written to your project, and it is gone when you quit.

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

- [`docs/SETUP.md`](docs/SETUP.md) — local setup, CI, and release instructions
- [`docs/REPLICATION-GUIDE.md`](docs/REPLICATION-GUIDE.md) — project environment setup
- [`docs/plugin-systems-and-lsp.md`](docs/plugin-systems-and-lsp.md) — plugin system design notes
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

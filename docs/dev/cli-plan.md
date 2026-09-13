# qrate CLI plan

Status: implementation in progress. The packaged `qrate` launcher, `open`, `--wait`, `version`,
help, project-path forwarding, and `qrate://` forwarding exist. App-control and direct-file commands
remain proposed.

## Decision

Use `qrate` as the public command. Rename the packaged desktop executable to the private
`qrate-app` name.

The desktop binary uses the Windows GUI subsystem in release builds. Windows does not attach that
binary to a terminal. A separate executable gives scripts reliable standard input, output, and exit
codes. The desktop shortcut starts `qrate-app`, so it does not show a console window.

The public command has two project modes:

1. **Open-project mode** is the default. The command starts or contacts `qrate-app` and operates on
   its active project.
2. **Direct-file mode** uses `--use <PATH>`. It opens that `.qrate` file without the desktop app.

Use a small local control interface for open-project mode. This interface is an application API, not
an agent bridge. A future MCP server can wrap the CLI without becoming part of the first release.

Direct-file commands must refuse writes when another process has the project open. Each write must
use one SQLite transaction. Read commands can run concurrently.

### App control boundary

The desktop app publishes a versioned local endpoint and a per-launch token in its application-data
directory. The endpoint accepts qrate commands only. It does not accept agent programs, findings, or
arbitrary SQL.

The app executes reads against its current in-memory project snapshot. It executes writes through the
same edit, undo, validation, and persistence paths as the grid. This makes command changes visible in
the open window.

If no project is active, a project command exits with code 1 and tells the user to run `qrate
<PROJECT.qrate>`. It does not silently select a recent project.

The app-control protocol is private and versioned with qrate. The CLI and app ship together. A future
MCP wrapper calls the CLI instead of depending on this protocol.

### Project storage boundary

The extraction does not move user settings or combine the two SQLite databases.

The `settings` crate currently has two responsibilities:

1. It owns the shared user-settings database and GPUI settings state.
2. Its `project.rs` module owns the `.qrate` schema, reads, writes, notes, and project settings.

Move only the pure `.qrate` file operations into a GUI-free `project-store` crate. This includes
`ProjectData`, `ProjectSpec`, schema constants, migrations, dataset access, columns, and stored notes.

Keep `AppSettings`, `CurrentProject`, debounced GPUI writers, and settings-window behavior in
`settings`. These types call `project-store` instead of issuing their own SQL.

The extraction is not required for commands that use the open app. The app can execute those
commands through its local control interface. The extraction becomes necessary when `--use` gains
write commands. It prevents the app and CLI from implementing different schema and migration rules.

## Command conventions

Use this form:

```text
qrate [GLOBAL OPTIONS] <COMMAND> [COMMAND OPTIONS]
qrate
qrate <PROJECT.qrate|qrate://URL>
```

Project commands use the active desktop project by default. `--use <PATH>` selects direct-file mode.
A positional `.qrate` path or `qrate://` URL launches the desktop app.

### Global options

| Option | Purpose |
|---|---|
| `--use <PATH>` | Operate directly on a `.qrate` file instead of the active desktop project. |
| `--format <human|json|jsonl>` | Select the standard-output format. The default is `human` on a terminal and `json` in a pipe. |
| `--color <auto|always|never>` | Control terminal colors. |
| `-q, --quiet` | Hide progress and non-error status messages. |
| `-v, --verbose` | Add diagnostic detail. Repeat for more detail. |
| `--no-input` | Refuse prompts. Use this option in CI. |
| `--lock-timeout <DURATION>` | Set the maximum wait for a project lock. The default is zero. |
| `--config <PATH>` | Use a different user settings file. |
| `-h, --help` | Show help. |
| `-V, --version` | Show the CLI version. |

Commands that change data also accept these options:

| Option | Purpose |
|---|---|
| `--dry-run` | Validate and show the proposed change without a write. |
| `--yes` | Accept a destructive or bulk change without a prompt. |
| `--backup <PATH>` | Write a SQLite backup before the change. |

Use `-` as standard input or standard output where a command accepts a data path. Send data only to
standard output. Send progress, warnings, and errors to standard error.

### Exit codes

| Code | Meaning |
|---:|---|
| `0` | The command completed. |
| `1` | The operation failed. |
| `2` | The command syntax was invalid. |
| `3` | Validation reached the `--fail-on` level. |
| `4` | A lock, stale revision, or other conflict stopped a write. |
| `5` | Authentication or a remote service failed. |

JSON errors use one object with `code`, `message`, and optional `details` fields. Stable scripts must
use `code`, not parse `message`.

## Full proposed command list

The phases show implementation order. They are not part of the command names.

### Phase 1: inspect, query, and export

These commands give immediate value and do not change project data.

#### `open`

```text
qrate
qrate [PROJECT.qrate|qrate://URL]
qrate open [PROJECT.qrate|qrate://URL] [--wait] [--new-window]
```

- With no arguments, open or focus the desktop app.
- Open the project or URL in the desktop app. The command starts the app when necessary.
- A positional project or URL is shorthand for `qrate open`.
- `--wait` waits until the desktop process closes.
- `--new-window` requests a new window instead of focusing an open project.
- Pass `qrate://` URLs to the app unchanged. The app validates and handles supported routes.

The operating-system URL handler should start `qrate-app` directly. This avoids a console flash on
Windows. A terminal invocation such as `qrate qrate://...` uses the launcher and forwards the URL.

`--use` accepts filesystem paths only. It rejects `qrate://` URLs because direct-file mode cannot
resolve app routes.

#### `app`

```text
qrate app status
qrate app launch [--wait]
qrate app path
qrate app quit [--force]
```

- `status` shows whether the app runs and which project is active.
- `launch` starts the desktop app without opening a project.
- `path` shows the resolved private `qrate-app` executable.
- `quit` asks the app to close. `--force` is for an unresponsive process and requires confirmation.

#### `project`

```text
qrate project info [--show-settings]
qrate project verify [--quick]
qrate project backup <OUTPUT> [--overwrite]
qrate project compact
qrate project recent [--limit <N>]
```

- `info` shows the schema version, project name, source, row count, columns, notes, and file links.
- `verify` checks the SQLite header, qrate application ID, schema, foreign keys, and database integrity.
- `backup` uses the SQLite backup API. It does not copy a live database with a file copy.
- `compact` runs `VACUUM`. It requires exclusive offline access.
- `recent` reads the desktop app's recent-project list.

#### `row`

```text
qrate row list [QUERY OPTIONS]
qrate row get <ROW_ID> [--columns <NAMES>]
qrate row search <TEXT> [QUERY OPTIONS] [--case-sensitive]
qrate row count [--where <EXPR>...]
qrate row distinct <COLUMN> [--where <EXPR>...] [--limit <N>]
```

Query options:

| Option | Purpose |
|---|---|
| `--columns <NAMES>` | Select comma-separated columns. |
| `--where <EXPR>` | Filter rows. Repeat to combine filters with AND. |
| `--order-by <COLUMN[:asc|desc]>` | Set row order. Repeat for secondary order. |
| `--limit <N>` | Limit results. |
| `--offset <N>` | Skip results. |
| `--null <TEXT>` | Set the human or CSV representation for an empty value. |

Use stable row IDs for `get` and all later write commands. Do not use the visible row number as an
identity. The first filter grammar should support `=`, `!=`, `<`, `<=`, `>`, `>=`, `contains`,
`starts-with`, `ends-with`, `is-empty`, and `is-not-empty`.

#### `column`

```text
qrate column list
qrate column get <NAME>
qrate column types
```

- `list` shows order, type, notes, and diagnostic counts.
- `get` shows all stored column configuration.
- `types` lists registered data types and their validators.

#### `note`

```text
qrate note list [--row <ROW_ID>] [--column <NAME>] [--source <SOURCE>]
qrate note get <NOTE_ID>
```

These commands include stored notes only. Computed diagnostics belong to `validate`.

#### `file`

```text
qrate file list [--missing] [--row <ROW_ID>] [--absolute]
qrate file resolve <ROW_ID> [--column <NAME>]
qrate file check [--hash] [--fail-missing]
```

- `list` shows linked files without opening them.
- `resolve` applies the same project-folder and link-method rules as the desktop app.
- `check` reports missing, unreadable, and duplicate targets.
- `--hash` adds content hashes. It is opt-in because it can read many large files.

#### `export`

```text
qrate export csv <OUTPUT> [EXPORT OPTIONS] [--delimiter <CHAR>]
qrate export json <OUTPUT> [EXPORT OPTIONS] [--pretty]
qrate export jsonld <OUTPUT> [EXPORT OPTIONS] [--pretty]
qrate export csl-json <OUTPUT> [EXPORT OPTIONS] [--pretty] [--mapping <PATH>]
qrate export zip <OUTPUT> [EXPORT OPTIONS] [--include-files] [--missing <error|skip>]
```

Export options:

| Option | Purpose |
|---|---|
| `--columns <NAMES>` | Export selected columns. |
| `--where <EXPR>` | Export matching rows. Repeat for AND. |
| `--order-by <SPEC>` | Set export order. |
| `--include-notes` | Include stored notes when the format supports them. |
| `--overwrite` | Replace an output file. |

Format names match the desktop Export menu. The CLI must call the same `data-exchange` functions.

#### `validate`

```text
qrate validate run [--checks <NAMES>] [--skip <NAMES>] [--fail-on <info|warning|error>]
qrate validate list
qrate validate explain <CHECK>
```

- `run` returns human, JSON, JSON Lines, or SARIF with `--format sarif`.
- `--fail-on` changes the exit code, not which diagnostics appear.
- `list` shows available validators and whether each validator supports headless use.
- `explain` shows the rule, accepted values, and source.

Headless validation requires extraction from GPUI globals. Do not start a hidden desktop app.

### Phase 2: create and edit projects

Start this phase after the app and CLI share a GUI-free project-storage crate.

#### `project create` and migration

```text
qrate project create <PATH> [--name <NAME>] [--from <INPUT>] [IMPORT OPTIONS]
qrate project migrate [--to <VERSION>] [--backup <PATH>]
```

- `create` makes a blank project unless `--from` supplies data.
- `migrate` shows the planned schema steps before confirmation.
- The app and CLI must use the same migrations.

Import options:

| Option | Purpose |
|---|---|
| `--input-format <csv|tsv|xlsx|xlsm|xlsb|xls|ods|google-sheet>` | Override format detection. |
| `--sheet <NAME_OR_INDEX>` | Select a workbook sheet. |
| `--delimiter <CHAR>` | Set a delimited-text separator. |
| `--headers <yes|no|auto>` | Control header detection. |
| `--column-config <PATH>` | Load qrate column configuration. |
| `--files-folder <PATH>` | Set the linked-files base folder. |
| `--link-method <filename|relative-path>` | Set file matching. |

#### `import`

```text
qrate import rows <INPUT> [IMPORT OPTIONS] [--mode <append|replace|upsert>]
                  [--key <COLUMN>...] [--on-conflict <error|keep|replace>]
qrate import notes <INPUT> [--format <csv|json>]
```

- `append` adds rows.
- `replace` replaces the dataset in one transaction and requires `--yes`.
- `upsert` requires one or more key columns.
- Import first produces a preview and a schema plan. `--no-input` requires `--yes` to apply it.

#### Row changes

```text
qrate row add [--set <COLUMN=VALUE>...] [--from-json <JSON_OR_@PATH>]
qrate row update <ROW_ID> --set <COLUMN=VALUE>... [--if <COLUMN=VALUE>...]
qrate row delete <ROW_ID>... [--yes]
qrate row delete --where <EXPR>... --yes
```

`--if` supplies optimistic checks. The command exits with code 4 if a current value differs.

#### Column changes

```text
qrate column add <NAME> [--type <TYPE>] [--notes <TEXT>] [--after <NAME>]
qrate column rename <NAME> <NEW_NAME>
qrate column set <NAME> [--type <TYPE>] [--notes <TEXT>|--clear-notes]
qrate column move <NAME> (--before <NAME>|--after <NAME>)
qrate column delete <NAME> [--yes]
```

Column deletes show the count of non-empty cells before confirmation.

#### Note changes

```text
qrate note add --row <ROW_ID> [--column <NAME>] --message <TEXT> [--source <SOURCE>]
qrate note update <NOTE_ID> --message <TEXT>
qrate note delete <NOTE_ID>... [--yes]
```

### Phase 3: fixes, settings, and Google Sheets

#### `fix`

```text
qrate fix list [VALIDATE OPTIONS]
qrate fix apply <FIX_ID>...
qrate fix apply --all [--source <SOURCE>] [--severity <LEVEL>] --yes
```

Each fix must include its expected current cell value. A stale fix exits with code 4.

#### Plugins: deferred

Do not define plugin CLI commands in this plan. The plugin marketplace branch changes installation,
updates, package identity, and command discovery. Add this section after that branch establishes the
shared plugin service.

The first CLI release does not inspect, install, remove, update, or run plugins.

#### `settings`

```text
qrate settings list [--scope <user|project|effective>]
qrate settings get <KEY> [--scope <user|project|effective>]
qrate settings set <KEY> <VALUE> --scope <user|project>
qrate settings unset <KEY> --scope <user|project>
qrate settings path [--scope <user|project>]
```

The default read scope is `effective`. Write commands require an explicit scope. Secret values show
only their presence and never appear in JSON output.

#### `google`

```text
qrate google status
qrate google login [--config-endpoint <URL>]
qrate google logout
qrate google destination show
qrate google destination choose
qrate google destination clear
qrate google export [--new|--destination <SHEET_ID>]
```

Use `export`, not two-way `sync`, in the command name. The current app writes project values to a
sheet but does not merge remote changes into an existing project. `login` and `choose` can open a
browser unless `--no-input` is set.

### Phase 4: external automation

The CLI itself is the supported automation boundary. Do not add agent-specific commands or reuse
bridge protocol 2.

A future MCP server can execute `qrate` commands and convert their JSON results into MCP tools. Keep
that wrapper in its own crate or repository. Do not make the first CLI release depend on MCP.

### Phase 5: shell support

```text
qrate completion <bash|zsh|fish|powershell|elvish>
qrate man <OUTPUT_DIR>
```

Generate these files from the parser. Do not keep hand-written completion files.

## Installation and `PATH`

Package the public `qrate` launcher with the private `qrate-app` desktop executable. The launcher
uses the sibling desktop executable, so it does not depend on an installation registry.

### Windows

- Install `qrate.exe` beside `qrate-app.exe`.
- Point Start Menu and desktop shortcuts to `qrate-app.exe`.
- Add an installer option named **Add qrate to PATH**.
- For a per-user install, add the install directory to the user `PATH`.
- For an all-users NSIS or MSI install, add it to the machine `PATH`.
- Remove only the exact installer entry during uninstall.
- Do not change `PATH` for the portable ZIP.

The default per-user path remains `%LOCALAPPDATA%\qrate`. The all-users path remains
`%ProgramFiles%\qrate`.

### macOS

- Put the launcher at `/Applications/qrate.app/Contents/MacOS/qrate`.
- Put the desktop binary at `/Applications/qrate.app/Contents/MacOS/qrate-app`.
- The DMG cannot safely change `PATH` during drag-and-drop installation.
- Add a desktop action that creates a symlink in `/usr/local/bin` after user confirmation.
- A Homebrew cask can expose the bundled binary with its `binary` stanza.
- Document the full bundle path as the fallback.

Do not use `/usr/bin`. System Integrity Protection owns that directory.

### Linux

- Put `qrate` and `qrate-app` in the release tarball.
- An install script or package places both files in `/usr/local/bin` or `/usr/bin`.
- A per-user install uses `$HOME/.local/bin`.
- The tarball itself does not change `PATH`.

Flatpak isolates host commands. A Flatpak build should keep live desktop integration separate from a
host-installed CLI.

### Source installs

```text
cargo install --path crates/cli
```

Cargo installs `qrate` in `$CARGO_HOME/bin`. The user must add that directory to `PATH`.

## Architecture sequence

1. Add the parser crate and approve this command list.
2. Rename the packaged desktop target to `qrate-app` and implement `qrate open`.
3. Add the local app-control interface for commands against the active project.
4. Implement project, row, column, file, and export commands through the open app.
5. Extract `.qrate` storage into a GUI-free crate for `--use`.
6. Extract headless validators and add `validate`.
7. Add direct-file locking and transaction-based write commands.
8. Add Google commands through shared libraries.
9. Add each binary to release packages and installer `PATH` options.
10. Generate shell completions and manual pages from the final parser.

Each phase must add parser tests, JSON snapshot tests, exit-code tests, and real `.qrate` fixtures.
Installer tests must confirm install, upgrade, uninstall, and exact `PATH` cleanup.

## Research notes

Research date: 2026-09-07.

### Tropy

Tropy accepts project paths and supported URLs as positional arguments. Its current parser also
accepts runtime options such as `--data`, `--cache`, `--logs`, `--extensions`, `--scale`, `--dev`,
`--renderer-preference`, `--disable-hardware-acceleration`, `--verbose`, `--trace`, and `--port`.
The `--port` option starts an HTTP API for an open desktop project.

Tropy is primarily a desktop launcher with an API switch. It does not provide a complete offline
task CLI. qrate should copy its useful positional file and URL launch behavior. qrate adds commands
for the active project and keeps direct file access behind `--use`.

Sources:

- [Tropy argument parser](https://github.com/tropy/tropy/blob/main/src/main/args.js)
- [Tropy API server](https://github.com/tropy/tropy/blob/main/src/main/api.js)
- [Tropy repository and installation methods](https://github.com/tropy/tropy)

### calibre

calibre ships separate GUI and command-line programs. `calibredb` can use a local library path or a
running Content server. macOS keeps command tools inside the application bundle.

qrate should use the same separate-executable model. Offline project access and live app access can
then share one command vocabulary.

Sources:

- [calibre command-line interface](https://manual.calibre-ebook.com/generated/en/cli-index.html)
- [`calibredb` manual](https://manual.calibre-ebook.com/generated/en/calibredb.html)

### OpenRefine and orcli

OpenRefine runs a local HTTP server. The third-party `orcli` tool starts temporary workspaces and
provides import, transform, and export pipelines. It accepts files, URLs, and standard input.

qrate should support pipeline input and structured output. It should not require a desktop server
for offline batch jobs.

Sources:

- [OpenRefine runtime model](https://openrefine.org/docs/manual/running)
- [OpenRefine API](https://openrefine.org/docs/technical-reference/openrefine-api)
- [`orcli` batch interface](https://github.com/opencultureconsulting/orcli)

### ExifTool

ExifTool accepts many files, directories, recursion, and standard input. It supports machine-readable
CSV and JSON. Its default write behavior preserves originals.

qrate should use the same pipeline conventions and safe write defaults. qrate should use SQLite
transactions and backups instead of ExifTool-style sibling files.

Source: [ExifTool application documentation](https://exiftool.org/exiftool_pod.html)

# Command reference

This page lists every `qrate` command. The program has the same text built in: type
`qrate help`, or `qrate help` and a command name, to read it in the terminal.

```text
qrate [PROJECT]
qrate <COMMAND> [OPTIONS]
```

| Command | What it does |
|---|---|
| [`qrate`](#open-qrate) | Open the desktop app |
| [`qrate open`](#open-qrate) | Open a project, or show the launcher |
| [`qrate app status`](#qrate-app-status) | Show whether qrate is running and which project is open |
| [`qrate app path`](#qrate-app-path) | Print the path of the desktop app |
| [`qrate app launch`](#qrate-app-launch) | Start qrate without a project |
| [`qrate project info`](#qrate-project-info) | Show a summary of the open project |
| [`qrate agent`](#qrate-agent) | Read the open project as an agent |
| [`qrate completion`](#qrate-completion) | Print a completion script for a shell |
| [`qrate man`](#qrate-man) | Write the manual pages into a folder |
| [`qrate version`](#qrate-version) | Print the version |
| `qrate help` | Show help for `qrate` or for one command |

## Open qrate

```sh
qrate
qrate catalog.qrate
qrate open catalog.qrate
qrate "qrate://plugin/install/example"
```

With no project, `qrate` opens the desktop app. If qrate is already running, it shows the
project launcher.
With a project, qrate opens that project. `qrate open` does the same and exists so that a
script can say what it means.

The project is a `.qrate` file or a `qrate://` link. If the file does not exist, or is not a
`.qrate` file, the command stops with exit code `2` and does not start the app.

If qrate is already running, the project opens in that window. qrate first saves the project
that was open.

| Option | Meaning |
|---|---|
| `--wait` | Wait until the desktop app closes, then exit with its exit code. Without it, the command returns at once. |

## qrate app status

```sh
qrate app status
```

Shows whether qrate is running and the path of the open project. This command exits with
`0` in both cases.

In a terminal:

```text
running
project: C:\Collections\catalog.qrate
```

When qrate is running with no project, the second line is `project: none`. When qrate is not
running, the output is one line, `stopped`.

In a pipe, or with `--format json`:

```json
{"running":true,"project":"C:\\Collections\\catalog.qrate"}
```

`project` is `null` when no project is open or qrate is not running.

| Option | Meaning |
|---|---|
| `--format human` or `--format json` | Choose the output. The default is `human` in a terminal and `json` in a pipe. |

## qrate app path

```sh
qrate app path
```

Prints the full path of the desktop app that `qrate` starts. Use it to check which install
the command belongs to. It does not need qrate to be running.

## qrate app launch

```sh
qrate app launch
```

Starts the desktop app with no project. It accepts `--wait`, with the same meaning as for
[`qrate open`](#open-qrate).

## qrate project info

```sh
qrate project info
```

Shows a summary of the project that is open in qrate.

| Field | In JSON | Meaning |
|---|---|---|
| name | `name` | The project name |
| path | `path` | The `.qrate` file |
| source | `source` | How the project was started, when qrate recorded it |
| created | `created_at` | When the project was made, as Unix time (seconds since 1970), when qrate recorded it |
| link method | `link_method` | How rows are linked to files |
| files folder | `files_folder` | The folder that holds the linked files |
| rows | `row_count` | The number of rows |
| columns | `column_count` | The number of columns |

A value that the project does not have is `null` in JSON.

If qrate has no project open, the command exits with `1`. In a terminal it prints a message
on standard error. With JSON output it prints `{"error":"no_active_project"}` on standard
output. If qrate is not running, the command exits with `3`.

| Option | Meaning |
|---|---|
| `--format human` or `--format json` | Choose the output. The default is `human` in a terminal and `json` in a pipe. |

## qrate agent

```sh
qrate agent overview
qrate agent query < query.json
qrate agent program-save < program.json
qrate agent program-run < run.json
qrate agent thumbnails < thumbnails.json
qrate agent stage-findings < findings.json
qrate agent stage-column-rewrite < rewrite.json
qrate agent stage-renames < renames.json
qrate agent edit < edit.json
qrate agent delete < delete.json
```

These commands let an AI agent, or a script, read the open project. Each command but
`overview` reads one JSON object on standard input. Each command prints one JSON object on
standard output. [Agents and skills](agents.md) explains how to use them and what they are
allowed to do.

| Command | Input | What it returns |
|---|---|---|
| `overview` | None | The project, its columns, the selection, counts of problems, and a `revision` number |
| `query` | A source, the fields to read, filters, and a limit | Rows or problems, 20 by default and 50 at most |
| `program-save` | `{"source": "..."}` | Checks and saves a small Luau program. It does not run it. |
| `program-run` | `{"revision": ..., "args": ...}` | Runs the saved program one time on a copy of the data |
| `thumbnails` | `{"items": [{"row": ..., "page": ...}]}` | Up to four small PNG pictures of linked files |
| `stage-findings` | `{"revision": ..., "findings": [...]}` | Puts draft findings in the Problems panel |
| `stage-column-rewrite` | `{"revision": ..., "column": "...", "values": [[row, "value"], ...], "note": "..."}` | Stages new values for one column as one draft. `unchanged` lists the rows that already hold the value. |
| `stage-renames` | `{"revision": ..., "renames": [{"row": ..., "from": "...", "to": "..."}]}` | Stages renames of linked files as one draft |
| `edit` | `{"revision": ..., "op": "...", ...}` | Changes the table's shape at once: adds rows or a column, renames, moves or describes a column, moves, indents, outdents or duplicates a row, or filters |
| `delete` | `{"revision": ..., "rows": [...], "reason": "..."}` or `"column": "..."` | Asks you to delete, with the agent's reason, and waits for your answer |

The three `stage-` commands change nothing. Each one puts a draft in the Problems panel and
marks the changed cells in the table, and you apply or discard it there.

### Stage a column rewrite

`stage-column-rewrite` is for a change to many cells in one column, such as new identifiers
from a list. It takes up to 50,000 pairs of a row number and the whole new value. Each row
can appear one time. The `revision` must be the one from the last `overview`.

```sh
qrate agent overview --agent me          # read "revision", for example 12
echo '{"revision":12,"column":"Digital ID","values":[[0,"AD-001"],[1,"AD-002"]],"note":"ids from mapping.csv"}' \
  | qrate agent stage-column-rewrite --agent me
```

qrate answers `{"revision":12,"result":"staged_rewrite","accepted":2,"unchanged":[]}`. The
Problems panel now shows one entry, **Rewrite Digital ID: 2 rows (ids from mapping.csv)**.
Right-click it to preview, apply, or discard. If you stage the same column again, the new
rewrite replaces the old one.

### Stage file renames

`stage-renames` is for the files that rows link to. Each rename gives the row and two paths.
The paths are relative to the folder that holds the `.qrate` file, and they must stay in
that folder.

```sh
echo '{"revision":12,"renames":[{"row":0,"from":"photos/1.jpg","to":"photos/AD-001.jpg"}]}' \
  | qrate agent stage-renames --agent me
```

qrate refuses the whole list if one rename is wrong: a path that leaves the project folder,
a `from` that is not the file its row links, two renames with the same name, or a `to` that
already exists. Two files can swap names in one list.

### Edit the table

`edit` changes the shape of the table at once. Each change is one step: one undo reverses
it, and the History panel names the agent. Name the change with `op`:

```sh
echo '{"revision":12,"op":"add_rows","rows":[{"Digital ID":"AD-005","Photographer":"Ralph M. Aderman"}]}' \
  | qrate agent edit --agent me
echo '{"revision":13,"op":"add_column","name":"Year","after":"Photographer"}' | qrate agent edit --agent me
```

The other operations are `rename_column`, `move_column`, `describe_column` (a data type, a
description, or both), `move_row`, `indent_row`, `outdent_row`, `duplicate_row` and `filter`.
`edit` does not change the value of a cell that already exists. For that, stage a rewrite.

### Ask to delete

`delete` never deletes by itself. qrate shows you a dialog with the agent's reason and what it
wants to delete, and the command waits until you choose **Delete** or **Keep**:

```sh
echo '{"revision":14,"rows":[3,7],"reason":"Rows 3 and 7 repeat row 2"}' | qrate agent delete --agent me
```

If you choose **Keep**, the command exits with `1` and prints `delete_declined`. After ten
minutes with no answer it gives up with `delete_timed_out`.

| Option | Meaning |
|---|---|
| `--agent <NAME>` | The name that qrate shows in the Agent panel for this call. If you do not give it, qrate uses the `QRATE_AGENT` environment variable. |

A refusal exits with `1` and prints JSON on standard output. For example, a query that asks
for too many rows prints `{"error":{"code":"invalid_query_limit"}}`, and every command prints
`{"error":"agent_access_off"}` when **Settings ▸ Agent ▸ Allow agents to read this app** is
off. If you run one of these commands in a terminal with no input, it stops with exit code
`2` and does not wait for you to type.

## qrate completion

```sh
qrate completion bash
```

Prints a completion script for `bash`, `zsh`, `fish`, `powershell`, or `elvish`. See
[Shell completion](index.md#shell-completion) for where to save it.

## qrate man

```sh
qrate man ~/.local/share/man/man1
```

Writes the manual pages into the folder that you name. qrate makes the folder if it does not
exist. See [Manual pages](index.md#manual-pages).

## qrate version

```sh
qrate version
qrate --version
```

`qrate version` prints the version number only, which is easier for a script to read.
`qrate --version` prints `qrate` and the version number.

### Agent file access and attachments

`qrate agent request-access --agent NAME` reads `{"path":"/path/to/folder","reason":"Read scans to create linked rows"}` on stdin. qrate asks the archivist to allow or deny read-only access to that file or folder. Relative paths are resolved against the project folder. A folder grant covers descendants, and a file grant covers only that file. qrate resolves canonical paths so a symlink cannot expand a grant. Grants belong to the named agent and current project, last until qrate closes, and can be revoked through **Revoke file access** in the Agent panel's session menu (right-click the new-session button).

`qrate agent check-access` reads `{"path":"/path/to/file"}` and checks existing access without showing a prompt. The project folder and configured files folder remain readable. `qrate agent forget-access` reads `{}` and revokes this agent's explicit grants in the current project. These commands return `file_access` or `file_access_revoked`; a refusal includes `file_access_required`, `file_access_denied`, `file_access_pending`, or `file_access_timed_out`. Approval requests expire after ten minutes. The agent name is a display label, not authentication.

`qrate agent import-files` reads `{"revision":12,"paths":["/approved/scans"],"recursive":true}`. Use the revision from the latest overview. Read access must already be granted. qrate uses its existing file importer to create linked rows and folder hierarchy, fills declared Title/Filename columns, and skips existing components. Originals are linked in place. It never copies or moves them or changes an existing row's values. One import adds at most 1,000 components, is one undo step, and is credited to the agent in History. `recursive` defaults to `false`. The `imported_files` response includes `added`, `skipped`, new source `rows`, and up to 20 path warnings. A revision change while planning refuses the import; read a new overview before retrying.

Pi's built-in read, ls, find, and grep tools check qrate's grants on every call. Read grants do not authorize writing files or shell commands, which keep their separate approval prompts. The confined Luau program sandbox is unchanged.

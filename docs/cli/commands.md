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

# The qrate command

qrate installs a command-line program named `qrate` beside the desktop app. Use it in a
terminal, in a script, or from an AI agent. It does three things:

- It opens the desktop app, with or without a project.
- It tells you whether qrate is running and which project is open.
- It lets an agent read the project that is open, and stage findings for you to review.

The command talks to the qrate that is running on your computer. It reads the project on
screen, so it sees edits that you have not saved yet. No command changes a cell.

## In this section

- [Command reference](commands.md): every command, its options, and its output
- [Agents and skills](agents.md): let an AI agent read the open project, and the skill files that teach it how

## Check that it works

Open a new terminal and type:

```sh
qrate version
```

qrate prints its version number. If the terminal says that it cannot find `qrate`, see
[Put qrate on your PATH](#put-qrate-on-your-path).

## A short tour

Open a project. The command returns at once, and qrate opens in its own window:

```sh
qrate catalog.qrate
```

Ask whether qrate is running:

```sh
qrate app status
```

```text
running
project: C:\Collections\catalog.qrate
```

Read a summary of the open project:

```sh
qrate project info
```

```text
name: catalog
path: C:\Collections\catalog.qrate
source: Spreadsheet + folder
created: 1790407337
link method: exact filename
files folder: C:\Collections\scans
rows: 1893
columns: 32
```

Get help for any command:

```sh
qrate help
qrate help agent query
```

## Put qrate on your PATH

How you get the `qrate` command depends on how you installed qrate.

| Install | Where the command is | What to do |
|---|---|---|
| Windows installer (`setup.exe`) | `bin\qrate.exe` in the install folder | Nothing. The installer adds `bin` to your `PATH`. Open a new terminal. |
| Windows MSI | `C:\Program Files\qrate\bin\qrate.exe` | Nothing. The MSI adds `bin` to the system `PATH`. Open a new terminal. |
| Windows portable ZIP | `bin\qrate.exe` in the extracted folder | Add that `bin` folder to your `PATH` yourself. |
| Microsoft Store | Not available | The Store package does not put `qrate` on your `PATH` yet. The Agent panel works without it. |
| macOS | `/Applications/qrate.app/Contents/MacOS/qrate-cli` | Make a link to it. See below. |
| Linux archive | `bin/qrate` in the extracted folder | Add that `bin` folder to your `PATH`, or make a link to it. |

On macOS, make the link in a folder that you own:

```sh
mkdir -p "$HOME/.local/bin"
ln -sf /Applications/qrate.app/Contents/MacOS/qrate-cli "$HOME/.local/bin/qrate"
```

On Linux, do the same with the `bin/qrate` file in the folder you extracted:

```sh
mkdir -p "$HOME/.local/bin"
ln -sf /path/to/qrate/bin/qrate "$HOME/.local/bin/qrate"
```

Then add `$HOME/.local/bin` to your shell's `PATH` if it is not there.

On Windows, the install folder has two programs named `qrate.exe`. The one in `bin` is the
command-line program. The other one is the desktop app, which has no terminal output. Only
`bin` goes on your `PATH`, so `qrate` in a terminal is always the command-line program.

## Output for scripts

`qrate app status` and `qrate project info` print plain text in a terminal and JSON in a
pipe. To choose, add `--format human` or `--format json`:

```sh
qrate app status --format json
```

```json
{"running":true,"project":"C:\\Collections\\catalog.qrate"}
```

The `qrate agent` commands always print JSON.

## Exit codes

A script can read the exit code to learn what happened.

| Code | Meaning | Where to look |
|---|---|---|
| `0` | The command completed. | Standard output |
| `1` | qrate refused the request, or the command failed. | A refusal is JSON on standard output. Other failures are on standard error. |
| `2` | The command line or its input was not valid. | Standard error |
| `3` | qrate is not running, or the command could not reach it. | Standard error |

`qrate app status` exits with `0` when qrate is not running, because it answered your
question. Read its output, not its exit code.

## Shell completion

`qrate completion` prints a completion script for `bash`, `zsh`, `fish`, `powershell`, or
`elvish`. Save the script where your shell loads completions. For example:

```sh
# bash
qrate completion bash > ~/.local/share/bash-completion/completions/qrate

# zsh, with a folder that is in your fpath
qrate completion zsh > ~/.zfunc/_qrate

# fish
qrate completion fish > ~/.config/fish/completions/qrate.fish
```

```powershell
# PowerShell: add this line to your profile
qrate completion powershell | Out-String | Invoke-Expression
```

## Manual pages

`qrate help` works on every platform. On macOS and Linux you can also read the manual with
`man`. The Linux archive includes the pages in `share/man`, and `man qrate` finds them when
the archive's `bin` folder is on your `PATH`.

To write the pages yourself, give `qrate man` a folder:

```sh
qrate man "$HOME/.local/share/man/man1"
man qrate
man qrate-agent-query
```

There is one page for `qrate` and one page for each command.

## Privacy

The command talks only to the qrate on your own computer. It does not use the network. A
program that can run `qrate` as you can already read your `.qrate` file, so the command does
not give a local program more than it has. To stop agents from reading the open project, open
**Settings ▸ Agent** and switch off **Allow agents to read this app**. `qrate app status` and
`qrate project info` keep working, because they do not read your data.

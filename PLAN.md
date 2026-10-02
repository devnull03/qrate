# qrate command-line interface

The full design is `docs/dev/cli-plan.md`. This file tracks the branch: what ships, in what order,
and what is left.

## Purpose

`qrate` is the one public surface for scripts and agents. The desktop app keeps a single private
control endpoint; everything outside the app — shells, skills, the embedded Pi, a future MCP
server — goes through the CLI and its JSON contract.

```
                       ┌─ Pi extension      (spawns $QRATE_CLI, parses JSON)
running qrate ◀─IPC── qrate-cli agent … ─┼─ shell / skills / Claude Code / Codex
  (private)            (public JSON)      └─ qrate mcp  (later; same handlers, for MCP-only clients)
```

The loopback agent bridge (`agent-bridge.json`, bridge protocol 2, `X-Agent`) is removed, not
deprecated. Its request types (`ai::agent`), live adapter (`table/src/agent.rs`), program runner
(`plugin-host/src/agent_program.rs`) and the Agent panel log stay; only the transport changes.

## Decisions

| Decision | Why |
|---|---|
| Pi calls the CLI, not MCP and not the private IPC | Pi has no native MCP. The IPC is private to one release; the CLI JSON is the stable contract, so the extension repo never tracks app internals |
| qrate starts Pi with `QRATE_CLI` and `QRATE_AGENT=pi` | No discovery, and the bundled CLI always matches the running app |
| The CLI forwards agent JSON verbatim | The app already validates `ai::agent::Request`; the CLI stays free of `ai` (and its `reqwest`) and of GPUI |
| One private endpoint: `app-control.json`, loopback TCP, bearer token | Replaces two near-identical servers. Socket I/O runs on its own threads; only the snapshot touches the main thread |
| The extension is pinned, not followed | The agent component's version is read from the pin (`scripts/package-component.sh`), so a release must name the exact extension it ships. `scripts/bump-pi-extension.sh` moves the pin in one command |
| `--format` and `--wait` belong to the commands that use them | A global flag shows up in every command's help and man page, including the agent commands it does nothing for |
| Help, completions and man pages come from the parser | `qrate help`, `qrate completion <shell>` and `qrate man <dir>` cannot drift from the commands; nothing hand-written to maintain |
| MCP is deferred | Every current agent client can run a command. `qrate mcp` is a thin adapter to add when one cannot |

## Private control endpoint (`app_control_protocol: 1`)

Unreleased, so still protocol 1. Shipped only between an app and the CLI from the same release.

- `GET /v1/status` → `{ "project": <path> | null }`
- `GET /v1/project/info` → project summary, or `409 {"error":"no_active_project"}`
- `POST /v1/agent` with `X-Agent: <name>` and an `ai::agent::Request` body → the `ai::agent`
  response, or `400 {"error": …}` for a refusal, or `403 {"error":"agent_access_off"}`

Rules: accepted sockets are blocking with a 2-second I/O timeout; each connection gets its own
thread, so a slow client can hold only that thread; the token comes from the OS RNG; the
descriptor is owner-only on Unix; the `agent_access` setting gates `/v1/agent` only.

## Public commands

```
qrate [PROJECT]                     qrate app status|path|launch
qrate open [PROJECT] [--wait]       qrate project info
qrate completion <SHELL>            qrate man <DIR>
qrate version                       qrate help [COMMAND]

qrate agent overview
qrate agent query           < query.json
qrate agent program-save    < {"source": "..."}
qrate agent program-run     < {"revision": 3, "args": {...}}
qrate agent thumbnails      < {"items": [...]}
qrate agent stage-findings  < {"revision": 3, "findings": [...]}
```

- `app status` and `project info` take `--format human|json`: human on a terminal, JSON in a pipe.
- For `agent`, stdin is the request's `params` object; stdout is the response JSON.
- `--agent <name>` or `QRATE_AGENT` names the caller in the Agent panel. A label, not proof.
- Exit codes: `0` answered, `1` refused (the refusal JSON is on stdout) or failed, `2` usage, `3`
  qrate is not running or unreachable (message on stderr).

## Phases

### 1. Fix the audit findings — done

- [x] Linux tarball keeps the updater's flat install root; `bin/qrate` is a symlink to the CLI
- [x] Windows `bin\qrate.exe` is a copy of `qrate-cli.exe`, not a C shim (argument quoting bug gone)
- [x] CLI resolves the desktop from its canonical path: sibling `qrate` that is not itself, else the
      parent of `bin/`
- [x] NSIS edits PATH through the registry without `EnvVarUpdate.nsh` or its 1024-character limit
- [x] A second launch with a project hands the project to the running app
- [x] Unknown startup arguments are ignored with a warning; a project that fails to open falls back
      to the launcher instead of exiting
- [x] New projects are recorded in recents once

### 2. One private endpoint — done

- [x] `app_control` serves status, project info and agent calls off the main thread
- [x] `agent_bridge.rs`, `agent-bridge.json`, and the bridge setting are deleted; `agent_access`
      replaces the setting
- [x] Agent panel records answered and refused calls with the `X-Agent` name

### 3. CLI agent commands — done

- [x] `qrate agent …` with the exit codes above
- [x] Unit tests for request framing and exit-code mapping
- [x] A terminal on stdin is a usage error instead of a silent wait

### 4. Pi first-class — done

- [x] `agent-runtime` sets `QRATE_CLI` and `QRATE_AGENT=pi`; drops `QRATE_AGENT_ENDPOINT`
- [x] `agent-runtime` reports a missing `qrate-cli` instead of starting a Pi whose tools all fail
- [x] `qrate-pi-extension` 0.3.0: tools spawn `$QRATE_CLI agent …` (separate repository)
- [x] `scripts/fetch-agent-runtime.*` pin 0.3.0 and its checksum; `scripts/bump-pi-extension.sh`
      repins both from a release

### 5. Rebase onto 0.6.0 — done

- [x] Store MSIX is built from `qrate.exe` (the binary was `app.exe` when that step was written)
- [x] Store MSIX carries `qrate-cli.exe`, which its on-demand agent component needs
- [x] Component packaging can read the extension pin again
- [x] Dev scripts and setup notes name `target\debug\qrate.exe`

### 6. Help, completions, man pages — done

- [x] `qrate help` with examples and exit codes; per-command help states each stdin shape
- [x] `--format human|json` on `app status` and `project info`
- [x] `qrate completion <bash|zsh|fish|powershell|elvish>`
- [x] `qrate man <DIR>`; the Linux tarball ships the pages in `share/man/man1`

### 7. Documentation — done

- [x] `docs/cli/` — the command, the command reference, agents and skills — listed in
      `docs/index.md`, so the site sidebar and the wiki pick it up
- [x] `skills/qrate-cli`, listed in `skills/README.md` and `docs/cli/agents.md`
- [x] `docs/agent-panel.md` describes the commands, not the bridge
- [x] `AGENTS.md`, `skills/qrate-live-review`, `.claude/skills/qrate-live-review`,
      `docs/dev/agent-runtime.md`, `docs/dev/cli-plan.md`, `CLAUDE.md`, `README.md`

## Left for the release

These cannot be finished on this branch.

- [ ] Run the release workflow once as a pre-release. The MSIX, tarball man pages and component
      packaging steps only run there.
- [ ] Homebrew cask: add `binary "#{appdir}/qrate.app/Contents/MacOS/qrate-cli", target: "qrate"`
      to `devnull03/homebrew-tap` **after** the first stable release that ships the CLI. Before
      that, the stanza fails every `brew install`, because the binary is not in the DMG.
- [ ] Microsoft Store: `qrate` is not on PATH. It needs an `AppExecutionAlias` for
      `qrate-cli.exe`, a sideloaded test, and a certification pass.
- [ ] After the merge: `bun run sync-docs` on the `site` branch, and update its hand-written
      `ai-tools.md` to link the new CLI pages.
- [ ] `README.md` still shows `docs/assets/final-report/agent-bridge.png`, whose caption is right
      but whose filename is not.

## Non-goals on this branch

- MCP server.
- Offline commands against a saved `.qrate` file (`cli-plan.md` later phases).
- Any command that changes a cell.
- `qrate app quit`.

## Definition of done

- No loopback server other than `app_control` exists in the app.
- The embedded Pi and an external agent reach the same live data through `qrate agent …`.
- `./scripts/ci.sh` passes.

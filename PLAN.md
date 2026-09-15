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
| MCP is deferred | Every current agent client can run a command. `qrate mcp` is a thin adapter to add when one cannot |

## Private control endpoint (`app_control_protocol: 1`)

Unreleased, so still protocol 1. Shipped only between an app and the CLI from the same release.

- `GET /v1/status` → `{ "project": <path> | null }`
- `GET /v1/project/info` → project summary, or `409 {"error":"no_active_project"}`
- `POST /v1/agent` with `X-Agent: <name>` and an `ai::agent::Request` body → the `ai::agent`
  response, or `400 {"error": …}` for a refusal, or `403 {"error":"agent_access_off"}`

Rules: accepted sockets are blocking with a 2-second I/O timeout; each connection gets its own
thread, so a slow client can hold only that thread; the token comes from the OS RNG; the descriptor is owner-only on Unix; the
`agent_access` setting gates `/v1/agent` only.

## Public agent commands

```
qrate agent overview
qrate agent query           < query.json
qrate agent program-save    < {"source": "..."}
qrate agent program-run     < {"revision": 3, "args": {...}}
qrate agent thumbnails      < {"items": [...]}
qrate agent stage-findings  < {"revision": 3, "findings": [...]}
```

- stdin is the request's `params` object; stdout is the response JSON.
- `--agent <name>` or `QRATE_AGENT` names the caller in the Agent panel. A label, not proof.
- Exit codes: `0` answered, `1` refused (the refusal JSON is on stdout), `2` usage, `3` qrate is
  not running or unreachable (message on stderr).

## Phases

### 1. Fix the audit findings — done when every item below is checked

- [x] Linux tarball keeps the updater's flat install root; `bin/qrate` is a symlink to the CLI
- [x] Windows `bin\qrate.exe` is a copy of `qrate-cli.exe`, not a C shim (argument quoting bug gone)
- [x] CLI resolves the desktop from its canonical path: sibling `qrate` that is not itself, else the
      parent of `bin/`
- [x] NSIS edits PATH through the registry without `EnvVarUpdate.nsh` or its 1024-character limit
- [x] A second launch with a project hands the project to the running app
- [x] Unknown startup arguments are ignored with a warning; a project that fails to open falls back
      to the launcher instead of exiting
- [x] New projects are recorded in recents once

### 2. One private endpoint

- [x] `app_control` serves status, project info and agent calls off the main thread
- [x] `agent_bridge.rs`, `agent-bridge.json`, and the bridge setting are deleted; `agent_access`
      replaces the setting
- [x] Agent panel records answered and refused calls with the `X-Agent` name

### 3. CLI agent commands

- [x] `qrate agent …` with the exit codes above
- [x] Unit tests for request framing and exit-code mapping

### 4. Pi first-class

- [x] `agent-runtime` sets `QRATE_CLI` and `QRATE_AGENT=pi`; drops `QRATE_AGENT_ENDPOINT`
- [ ] `qrate-pi-extension`: tools spawn `$QRATE_CLI agent …`; release and repin in
      `scripts/fetch-agent-runtime.*` (separate repository)

### 5. Documentation

- [x] `AGENTS.md`, `skills/qrate-live-review`, `.claude/skills/qrate-live-review`,
      `docs/dev/agent-runtime.md`, `docs/dev/cli-plan.md`, `CLAUDE.md`, `README.md`

## Non-goals on this branch

- MCP server.
- Offline commands against a saved `.qrate` file (`cli-plan.md` later phases).
- Any command that changes a cell.

## Definition of done

- No loopback server other than `app_control` exists in the app.
- The embedded Pi and an external agent reach the same live data through `qrate agent …`.
- `./scripts/ci.sh` passes.

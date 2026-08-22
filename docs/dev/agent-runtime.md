# Embedded Pi agent

qrate embeds Pi 0.84.2 in the Agent panel. The terminal starts that pinned executable directly
through a PTY; it does not expose PowerShell, cmd, bash, or another general-purpose shell.

The Rust ownership boundary is `crates/agent-runtime`: it resolves the bundle, seeds the isolated
profile, and owns the Pi PTY/session. `workspace` only renders that state in the Agent panel, `app`
only initializes it, and `ai` remains the provider- and transport-neutral qrate tool contract.

The qrate-specific package lives in the public
[`devnull03/qrate-pi-extension`](https://github.com/devnull03/qrate-pi-extension) repository. qrate
pins its v0.1.0 tag and checksum in the runtime-fetch scripts. The package supplies the system
prompt, live-review skill, typed bridge tools, and permission gates. Keep those concerns there so
they can be tested against Pi without rebuilding the desktop app.

## Why the extension still uses the bridge

| Integration | Strength | Cost | Decision |
|---|---|---|---|
| Current loopback bridge | Already reads qrate's unsaved in-memory state; revision and `expected` guards are proven | Callers assemble JSON and receive no Pi-native schemas or permission UI | Keep as the canonical app boundary |
| MCP server | Standard discovery and usable by many agent clients | Adds another transport and process lifecycle while duplicating the bridge contract | Defer until ASNT-98 or another non-Pi client needs discovery |
| Pi extension over the bridge | Typed native tools, qrate prompt/skill loading, and Pi confirmation UI; independent release cadence | Coupled to Pi's extension API and must track its pinned version | Primary embedded integration |

The extension is an adapter, not a second source of truth. It rereads `agent-bridge.json` on every
call and sends the same requests as any external agent. The endpoint now includes
`"bridge_protocol": 1`; additions must remain backward compatible within that protocol version.

## Provider and credential ownership

qrate sets both the startup provider and the only model in Pi's model picker to `openrouter` and
`openrouter/free`. On first use, the terminal tells the user to run `/login openrouter`. Pi performs
that login and stores the resulting user-controlled credential in its own `auth.json` under
qrate's isolated Pi profile. qrate never reads, copies, logs, or stores the key.

The profile is under qrate's application-data directory at `pi-agent`. qrate owns and refreshes
`SYSTEM.md`; it seeds `settings.json` only when absent. Pi owns `auth.json` and the session files.
Sessions use a qrate-only session directory and Pi's `--continue` behavior, with the open project's
directory as the working directory.

## Tool permissions

Pi keeps its coding tools, but the extension gates risky calls before execution:

- `bash` always asks for confirmation.
- `write` and `edit` always ask for confirmation.
- reads and searches outside the open project ask for confirmation.
- a missing confirmation UI or unresolved path denies the call.

The qrate bridge remains read-only. `qrate_stage_findings` only publishes drafts for the archivist
to accept in qrate.

## Updating the pinned runtime

Update the versions and SHA-256 values in both `scripts/fetch-agent-runtime.ps1` and
`scripts/fetch-agent-runtime.sh`. Then run the extension's type-check/tests and load it with the new
standalone Pi binary before changing qrate's pin. Release packaging fetches:

- Windows x64 Pi for the portable zip, NSIS installer, and MSI.
- Linux x64 Pi for the tarball.
- Both macOS binaries and merges them into one universal Pi executable for the universal app.

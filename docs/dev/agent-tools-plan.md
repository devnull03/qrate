# Agent tools and Islandora Workbench

Status: plan. Revised 2026-10-02 after auditing the shipped CLI branch (#122). No code yet.

## Purpose

Let a plugin declare tools an agent can call through `qrate agent`, and prove the contract with an
Islandora Workbench preflight in the Islandora plugin. An agent can check and explain a Workbench
ingest. It cannot start one.

## What the CLI branch already gives us

The audit of `feature/qrate-cli` found one seam that this work plugs into, and nothing to rebuild:

| Need | Already there |
|---|---|
| Transport | `POST /v1/agent` in `crates/app/src/app_control.rs` takes any `ai::agent::Request`. Tools are two more variants, not a new endpoint. |
| CLI surface | `AgentCommand` in `crates/cli/src/main.rs` maps a subcommand to a method name and frames stdin JSON. Two more variants. |
| Kill switch | The `agent_access` setting already gates every agent request. Tools ride it. |
| Audit trail | Every agent call lands in the Agent panel (`AgentEntry`) under the caller's `--agent` name. Tool calls get this for free. |
| Exit codes | `0` answer, `1` refusal (JSON on stdout), `2` usage, `3` unreachable. Tool errors are refusals. |
| Off-UI execution | Plugin calls already run on the background executor with a compute deadline and a per-run HTTP budget. |

Things the plan has to work around:

- Plugins have one permission, `net`, and `qrate.http` is GET only. A plugin cannot write to a
  server or start a process. Islandora Workbench is a Python program, so no plugin can run it.
- `docs/dev/cli-plan.md` says the first CLI release "does not run plugins". This work changes that.
  That section points here.
- Anonymous JSON:API on isle.dvnl.work serves nodes and taxonomy terms, but not `field_config`.
  A preflight reads field names from a published node of the target content type.

## Decisions (changed from the first plan)

1. **Tools are read-only. There are no permission categories.** The first plan had read-only, local
   mutation and external side effect tiers. A tool function gets no way to write. The host drops any
   settings writes it returns, and `http` is GET only. Anything with a side effect is a plugin menu
   command, which only a person can click. The confirmation rule becomes structural: there is no
   tool path to an ingest to confirm. A tier system with one populated tier is scaffolding.
2. **No availability metadata.** A tool is listed when its plugin is enabled and its declared
   permission is granted. Nothing else.
3. **Input schema, no output schema.** Pi needs a JSON Schema to register a tool. The host checks a
   flat subset itself (object, `properties` of `string|number|integer|boolean` or an array of those,
   `required`) and rejects any other schema at load. This avoids a new dependency. The output is any
   JSON value, capped like `agent_program::MAX_OUTPUT`.
4. **Workbench lives in the Islandora plugin**, not a new plugin. It shares the site URL, the
   account, and `api.lua`.
5. **qrate does not run Workbench.** The plugin writes Workbench's input. The person runs
   `workbench --check` and then the ingest. Direct ingest is out of scope (see Open question).

## Plugin contract

An optional descriptor field. It is additive, so `API_VERSION` stays 2.

```lua
tools = {
  {
    name = "workbench_preflight",          -- [a-z0-9_]+, unique per plugin
    description = "Check the selected rows against the Islandora site before a Workbench ingest.",
    input = { type = "object", properties = { scope = { type = "string" } } },
  },
},
tool = function(name, input, scope) ... return { ... } end,
```

- `tools` and `tool` are declared together, the same rule `exports`/`export` follow.
- `scope` is what `validate` receives: rows, columns, settings, and the selection.
- An agent addresses a tool as `<plugin id>.<name>`.

## Phases

Each phase stops for review before the next. Phase 1 is required before any code.

1. **Land #122, then retarget #123 to `main`.** This branch is #122 plus this plan. #122 is green
   on all three platforms and mergeable.
2. **Host** (`crates/plugin-host`). Parse `tools`, validate the schema subset, and add `call_tool`.
   It checks input, runs on the background executor under `EXPORT_DEADLINE`, caps output, and
   discards writes. Tests: an unknown tool, a bad input, a timeout, oversized output, and dropped
   writes.
3. **Contract** (`crates/ai/src/agent.rs`, `app_control.rs`, `crates/cli`). Add
   `Request::Tools` and `Request::ToolCall { tool, input }`, and the commands `qrate agent tools`
   and `qrate agent tool-call`. Update `AGENTS.md`, `docs/cli/commands.md`, `docs/cli/agents.md`,
   the `qrate-live-review` skill, and the plugin section of `cli-plan.md`.
4. **Types, in three repos.** `qrate-plugin-template/types/qrate.lua` first, then
   `plugins/islandora/types/qrate.lua`, then both READMEs. Run
   `cargo test -p plugin-host -- --ignored`.
5. **Pi extension** (`qrate-pi-extension`). At session start, run `qrate agent tools` and call
   `registerTool` once per tool, in addition to the six fixed tools. Release it, then pin it here
   with `scripts/bump-pi-extension.sh`.
6. **Islandora plugin.** Set `api_version = 2`.
   - Exports: "Workbench CSV" (`id`, `title`, `field_*` columns, `file`, `parent_id`) and
     "Workbench config" (`task: create`, host, input paths, no password).
   - Tool `workbench_preflight`, also on a menu command for people. Checks the site and the rows:
     unique `id`, title length up to 255, column names against a sample node's fields, vocabulary
     terms (reuses `match.lua`), `field_member_of` and `parent_id` targets exist, `field_model`
     terms exist, and each row has its file. Returns `{ ok, rows_checked, problems = [{ row,
     column, message }] }`.
   - The same problems feed `validate`, so the Problems panel and the agent agree.
7. **Docs.** Add the ingest workflow to the plugin README: export, preflight, `workbench --check`,
   ingest. Include what the preflight cannot see without credentials (unpublished parents,
   field permissions).

## Open question

Should qrate ingest directly later? Both routes need a new host capability that has an external
side effect: a process runner for Workbench, or `http.post` to reimplement Workbench in Luau. Neither
has a consumer until phases 1 to 7 are in use. This plan defers it. If it comes back, it is a menu
command with its own confirmation dialog, never a tool.

## Non-goals

- A generic tool catalogue with no plugin behind it.
- Tools that write cells, settings, or servers.
- Agent-initiated ingest.

## Definition of done

- `qrate agent tools` lists only the tools of enabled plugins with granted permissions.
- `qrate agent tool-call` rejects bad input with exit `1` and a JSON refusal. It returns the tool's
  JSON on success. Every call shows in the Agent panel.
- The preflight runs without credentials and changes nothing.
- The README documents the Workbench workflow from export to ingest.

# Agent capabilities plan

Status: in progress on `feature/qrate-cli`.

## Purpose

Two goals:

1. **A feature built for the archivist reaches the agent with no extra wiring.** Today each agent
   ability is written about eight times. This plan replaces that with one trait a feature implements
   once. The trait's required items force the developer to decide how an agent may use the feature,
   the same way the required `Origin` makes every mutation say who made it.
2. **A stronger agent.** This covers ten new abilities (listed below) and an upgrade of the bundled
   Pi from 0.84.2 to 1.x. Pi 1.x ships codemode, tool search, structured tool results and MCP built
   in.

Every step keeps the rules the agent contract already has:

- Cell values and file names are only staged.
- Shape edits happen at once, as one credited undo step.
- Deleting always waits for the archivist.
- Nothing leaves the machine unless the archivist allows it.

## How a feature reaches the agent today

Take `rename_column`. To expose it, a developer touches:

| Layer | File | What is written by hand |
|---|---|---|
| Table operation | `crates/table/src/lib.rs` | `Structural::RenameColumn` and its `structural()` arm |
| Contract | `crates/ai/src/agent.rs` | `TableEdit::RenameColumn`, its errors and result shape |
| Adapter | `crates/table/src/agent.rs` | `edit_table` arm: name→index, validation, History label |
| Transport | `crates/app/src/app_control.rs` | `summarize`/`describe` arms, error `wire_name` |
| CLI | `crates/cli/src/main.rs` | subcommand, help text, method name, parse test |
| Pi tool | `qrate-pi-extension/extensions/qrate.ts` | a TypeBox schema that copies the Rust one |
| Prompt | `qrate-pi-extension/SYSTEM.md` | when to use it |
| Docs | `AGENTS.md`, `docs/cli/*.md`, `skills/qrate-cli` | the same facts again |

Two things go wrong with this:

- **Copies drift.** CLAUDE.md already warns that nothing in CI checks `AGENTS.md`, and the
  TypeScript schema can drift from the Rust one silently.
- **Safety rules live in each adapter arm.** Checking the revision, crediting the agent, producing
  exactly one undo step, and asking before a delete are re-implemented per request. A new arm that
  forgets one is a bug that no test is forced to catch.

## Part A — the capability layer

### The trait

Add a crate, `capability`, that depends on `gpui`, `serde`, `schemars` and `settings`. Each feature
the agent may use is one type:

```rust
pub trait Capability: 'static {
    /// Wire name, e.g. "table.rename_column". Stable: it is public through the CLI.
    const NAME: &'static str;
    /// No default, so every feature has to decide. See `Exposure`.
    const EXPOSURE: Exposure;
    /// No default either. The dispatcher enforces it, not the feature.
    const EFFECT: Effect;

    /// Doc comments on these types become the schema descriptions the agent reads.
    type Input: DeserializeOwned + JsonSchema;
    type Output: Serialize + JsonSchema;

    /// What History and the confirm dialog say, e.g. "Rename column Date to Created".
    fn label(input: &Self::Input, cx: &App) -> String;

    /// Runs only after the dispatcher has done everything `EFFECT` requires.
    fn run(input: Self::Input, call: &Invocation, cx: &mut App) -> Task<Result<Self::Output, Refusal>>;
}

pub enum Exposure {
    Agent,
    /// Only the archivist may use it. The reason appears in the generated catalog, so a
    /// withheld feature is a reviewed decision rather than something nobody thought about.
    ArchivistOnly { why: &'static str },
}

pub enum Effect {
    /// Changes nothing.
    Read,
    /// Publishes a draft that only the archivist can apply.
    Stage,
    /// Changes the project at once, as exactly one undo step credited to the caller.
    Edit,
    /// Like `Edit`, but an agent's call waits for the archivist to confirm it in a dialog.
    Confirm,
    /// Leaves the project: reads files outside it, or reaches a network host.
    Reach(Reach),
}
```

`Invocation` carries:

- the caller: `Origin::Archivist`, `Origin::Agent { agent, .. }` or `Origin::Plugin`;
- the revision the caller read;
- a cancellation signal.

`Refusal` replaces the growing `RequestError` enum with a stable `code` plus a message. The CLI
documents the codes, and a test lists them so none is renamed by accident.

### Why the required items matter

`EXPOSURE` and `EFFECT` have no default value. A developer adding a capability cannot compile it
without choosing both. This is how the required `Origin` argument works: the safe path is the only
path that builds.

The second half is reachability. The table's mutation entry points (`structural`, `arrange`,
`describe_column`, the cell-write paths) become `pub(crate)`. Code outside `table` changes the table
only by invoking a capability. The menu, keyboard shortcuts, plugins and agents all go through:

```rust
capability::invoke::<RenameColumn>(input, Origin::Archivist, cx)
```

A selection-based command such as "Insert Row Above" first turns the selection into explicit
targets, then invokes the capability.

Since every feature is a capability and every capability declares an exposure, the agent gets each
new feature unless someone writes down why it should not.

### The dispatcher owns the safety rules

One function, `capability::dispatch(name, json, invocation, cx)`, enforces each effect before and
after `run`. Feature code cannot skip these steps.

| Effect | Before `run` | After `run` |
|---|---|---|
| `Read` | Check the revision when the input names rows. | Cap the result size and record usage. |
| `Stage` | Check the revision. | Store the draft under the caller's name. The yellow cell markers, Apply all and stale-cell skipping come from one place. |
| `Edit` | Check the revision. Open a History transaction labelled `label()` and credited to the caller. | Check that exactly one step was pushed; roll back and refuse if not. Count against the agent's edit budget. |
| `Confirm` | Agents: show the dialog with `label()` and the agent's `reason`, and wait (up to 10 minutes, one dialog at a time). The archivist: no dialog, because they chose it from a menu. | Same as `Edit`. |
| `Reach` | Check for an existing grant. If there is none, ask the archivist, naming the path or host. | Log the access with its host or path. |

This takes about 250 lines of per-arm validation and crediting out of `crates/table/src/agent.rs`,
and it gives existing behaviour one test suite per effect instead of one per request.

### One catalog, generated everywhere

The registry builds the catalog: every capability's name, exposure, effect, input and output JSON
Schema, and description. Everything else is generated from it:

- **Transport.** `ai::agent::Request` becomes `{ "method": <name>, "params": <json> }`, and
  `app_control` dispatches by name. Its `summarize`/`describe` arms go away, replaced by `label()`.
- **CLI.**
  - `qrate agent capabilities [--json]` prints the catalog.
  - `qrate agent call <name> < params.json` calls any capability.
  - The existing subcommands (`stage-findings`, `edit`, `delete`, …) stay as aliases with the same
    JSON and exit codes, because they are the public contract.
- **Pi.** The extension reads the catalog when a session starts and registers one tool per
  capability with the catalog's JSON Schema; Pi 1.x validates plain JSON Schema. A new capability
  needs no TypeScript.
- **Docs.** `docs/cli/capabilities.md` is generated from the catalog. A test regenerates it and fails
  on any difference, which closes the "nothing in CI checks AGENTS.md" gap for everything but the
  prose.

### Migration (behaviour unchanged)

Each step is its own commit, and the existing agent tests stay green throughout:

1. Add the crate, the trait, the registry and `dispatch`, with tests per effect against a mock
   capability.
2. Port the read calls (`overview`, `query`, `program_*`, `thumbnails`), then `stage_*`, then each
   `TableEdit` variant, then `delete` and the file-access calls. Delete the old `match` arms as their
   capabilities land.
3. Make the table's mutation entry points `pub(crate)` and route menus and shortcuts through
   `invoke`.
4. Add `capabilities` and `call` to the CLI and turn the old subcommands into aliases. Generate the
   docs.

## Part B — Pi upgrades

Pi 1.x, released after the version we pin, adds the features that do the most for an agent in a
large table:

- **Codemode:** the model writes one JavaScript script that calls many tools and returns only the
  output, instead of one tool call per step. Scripts run in QuickJS with no Node APIs, file system,
  network or timers.
- **Tool search:** tools marked `deferred` are found only when needed, so they cost nothing in the
  prompt until then.
- **Tool exposure and namespaces:** tools can be marked `direct`, `codemode` or `deferred`.
- **Structured tool results:** `outputSchema` plus `structuredContent`, so a codemode script gets
  JSON back instead of text.
- **MCP:** built-in support for MCP servers.

This is the same pattern as Anthropic's Tool Search Tool and programmatic tool calling, and
Cloudflare's Code Mode.

Security finding that makes this safe to adopt: Pi's docs say tool calls made from a codemode script
go through the same `tool_call` handlers as calls the model makes. Our permission gate in
`permissions.ts` and qrate's own grants therefore still apply inside scripts. A script cannot get
around them.

### B1. Upgrade the bundled Pi to 1.6.x

1. Confirm the 1.6 release still ships the same per-platform bundles that
   `fetch-agent-runtime.sh` downloads, then pin it by checksum as before.
2. Work through the breaking changes between 0.84 and 1.6 that affect us:
   - The theme schema moved. `write_pi_theme` must add the new `$schema` and keep colours under
     `vars`; test it against Pi's published schema.
   - `before_agent_start` is now a union.
   - Tool arguments are validated by `pi-schema` instead of TypeBox.
   - Component `render()` returns a read-only array.
3. Settings to seed in qrate's isolated profile:
   - **Codemode on.**
   - **The built-in MCP extension off.** An archivist's project folder is not a trusted place to
     load servers from. A server the archivist adds on purpose in their own Pi profile is their
     choice.
   - **Project trust off.** It would read `.pi/` files from the collection folder.
4. Re-run the Xvfb walkthrough: theme, Tab, sign-in links, Ctrl+C.

### B2. Tools generated from the catalog, sized for codemode

- Every capability becomes a tool in a `qrate` namespace. The namespace's `instructions` hold what
  `SYSTEM.md` says today, so scripts can read it with `describeNamespace("qrate")`.
- Exposure:
  - `qrate.overview` and `qrate.query`: `direct`.
  - Other `Read` and `Stage` capabilities: `codemode`.
  - Rare ones (authority lookups, export check): `deferred`.
- Each tool declares `outputSchema` from the capability's `Output`, so a script can
  `await tools.qrate_query(...)` and loop over rows. Only the script's summary reaches the model's
  context, which is where most of the token savings in a 10,000-row table come from.
- Codemode and qrate's Luau programs do different jobs:
  - Codemode coordinates calls (query, compute, stage).
  - `program_run` does bulk computation inside qrate, next to the data.
  - A script may call `program_run`.

### B3. Modes built from effects

The catalog tells the extension each tool's effect. Add `/qrate-mode`:

| Mode | Active effects |
|---|---|
| `review` (default for a project opened the first time) | `Read`, `Stage` |
| `edit` | adds `Edit`, `Confirm` |
| `connected` | adds `Reach` |

This is done with `pi.setActiveTools`, and the dispatcher checks the same rule, so the mode cannot be
bypassed from a script. The Agent panel shows the current mode.

### B4. Sandbox Pi's shell on Linux and macOS

Pi's `sandbox` example wraps `bash` with bubblewrap on Linux and `sandbox-exec` on macOS. Adopt it in
the extension:

- Writes allowed only to Pi's temp directory, not the project.
- No network unless granted.
- `~/.ssh`, `~/.aws` and similar paths denied.

Windows has no equivalent, so there `permissions.ts` stays the only guard, and the docs say so.

### B5. Optional, decide later

- **Reviewer subagents** (Pi's `subagent` example): split a large table into parallel read-only
  reviewers, each in `review` mode with its own context.
- **Classifier models** (`models.classify` in codemode): classify photos or subjects at lower cost
  than a chat model. This uses the archivist's provider credentials and costs money, so it stays off
  until they turn it on.
- **Community packages:** don't adopt any. We bundle Pi and pin it by checksum; an unpinned
  third-party package is a supply-chain risk, and the built-in features cover the same ground.

## Part C — the ten capabilities

Each is listed with its effect and the security points beyond what the dispatcher already enforces.

| # | Capability | Effect | Security beyond the effect |
|---|---|---|---|
| 1 | `checks.adopt_program` makes a saved Luau program a project check that runs with the others | Confirm | Runs in the same read-only sandbox with its 2 s deadline, now per check run. The dialog shows the source. The check is stored with a source hash and asks again if the source changes. Each finding names its check, which can be disabled from the Problems panel. |
| 2 | `stage.patch` stages rows × several columns, each cell with an optional `expected` value | Stage | Generalises `stage_column_rewrite`, which becomes a one-column patch. Limits: 50,000 cells and 64 KB per value. A cell is skipped if it changed. |
| 3 | `values.variants` returns the `clustering` crate's variant groups for one column | Read | Group count and size limits. The agent fixes variants with #2. |
| 4 | `history.list` and `notes.list` | Read | Paged. Results include author names, which is fine locally; the catalog says these results are personal data. |
| 5a | `media.image` returns one linked image at up to 1568 px | Read | Only files the project links, or paths covered by a read grant. Limits on size and images per minute. |
| 5b | `visual.similar` searches by an image or by text | Read | Uses the existing CLIP index. If there is no index, it refuses with `index_missing`. Building the index is its own capability, `visual.build_index` (Confirm), because it downloads model weights. |
| 6 | `qrate agent watch` streams selection, revision, and staged-change applied or rejected events | Read | Gives the agent feedback on which of its drafts were accepted. It never starts a turn by itself: the extension passes events in only while the archivist has `/qrate-follow` on, and only with `deliverAs: "nextTurn"`. |
| 7 | `authority.search` looks up a term in Wikidata, VIAF, Getty or id.loc.gov | Reach(Network) | qrate makes the request, not Pi. The hosts are fixed in code, the query shape is fixed, and results are cached. The first use per project and source asks the archivist. Results are marked as untrusted data, and URIs are only staged through #2. |
| 8 | `export.check` dry-runs CSV, Excel, JSON-LD or CSL-JSON and reports what would fail | Read | Writes no file. |
| 9 | `stage.changeset` combines patch, renames, added rows or columns and deletes into one draft | Stage | Applying it is one `Step::Batch`. Any delete in it gets the same confirmation with the agent's reason when the archivist clicks Apply, so "deleting always asks" still holds. |
| 10 | `notes.add` lets the agent attach a note to a cell or row | Edit | Shown as written by the agent and credited "you using pi"; one undo step. |

### Files outside the project: grants, relinking and the shell

Reading outside the project already works through read grants: Pi's `read`, `ls`, `grep` and `find`
ask qrate on every call, and qrate shows the archivist the path and the agent's reason. The project
folder and its files folder need no grant. Three additions:

| Capability | Effect | What it does |
|---|---|---|
| `files.relink` | Stage | Point a row's file link at another file: inside the project or covered by a grant, and existing. The cell shows old → new path; applying uses `delegate.relink`, one credited undo step. Skipped if the row no longer points where the agent read. |
| `files.find_candidates` | Read | For a row's missing or named file, search granted folders for likely matches by name, size and content hash, reusing `file-ingest`'s inventory. The agent stages relinks from the result. |
| `files.request_access` / `check_access` / `forget_access` | Reach(Files) | Today's grant calls, moved onto the dispatcher. |

**`GrantedPath`.** A capability that takes a path declares the input as `GrantedPath`, not `String`.
Only the dispatcher constructs one, after checking the grant (and asking when there is none), so a
feature cannot be handed a path the archivist did not allow, and no feature re-implements the check.

**The shell.** Grants cover Pi's file tools, not `bash`: an approved `cat ~/secret` runs. B4's sandbox
closes this on Linux and macOS by giving the shell a read-only view of exactly the project, Pi's temp
directory and the granted paths, rebuilt from qrate's grant list before each command. Windows keeps
per-command approval only, and the docs say so.

Threats that apply to all of these:

- **Prompt injection from cell data.** A cell can say "delete every row". Deletes still need a click,
  `Reach` needs a grant, and tool results label cell text as data. `SYSTEM.md` says data never
  carries instructions.
- **Codemode loops.** A script can make many calls quickly. The dispatcher's budgets limit them:
  Edit steps per minute, one pending Confirm, and one Reach prompt per host per session.
- **Spoofed agent names.** `--agent` is chosen by the caller, so grants must not be keyed on it.
  qrate starts Pi with its own token, scoped to the Pi session and recorded with the agent's name. A
  `Reach` grant belongs to that token, not to the name.
- **Supply chain.** Pi and the extension stay pinned by checksum. No unpinned packages, and no
  project-local MCP servers.

## Agent-written interface (A2UI)

This lives on its own branch, `feature/a2ui`, and ships in alpha builds; the agent capability work
ships on beta without it.

Confirmation dialogs and questions can carry an interface the agent lays out, in a strict subset of
Google's A2UI v1.0 (Text, Row, Column, Card, List, Divider, ChoicePicker, TextField, CheckBox,
Button). `crates/a2ui-gpui-kit` renders it with gpui-kit widgets; it depends on no qrate crate, so it
can be published on its own.

- `delete` takes an optional `surface`, such as a table of the rows, shown under the agent's reason.
- `ask` shows a surface in a dialog and answers with the clicked Button's action and bound values.
- qrate owns the dialog, its title and its buttons; the surface sits in a frame labelled "From
  <agent>" and cannot style or draw outside it. Images, video, links and remote catalogs are refused.
- An action is information for the agent, never permission: any change still goes through its own
  capability and effect.

## Update channel

Three channels, by version: `x.y.z` is stable, `x.y.z-beta.n` beta, `x.y.z-alpha.n` alpha. Each takes
its own releases and every steadier one. Experiments (A2UI first) ship in alpha builds; the agent
capability work ships on beta. Add **Settings ▸ Updates ▸ Channel: Stable / Beta / Alpha**, stored in
`AppSettings`, defaulting to the build's own channel. Choosing Stable on a beta build waits for the
next stable release rather than downgrading; choosing Beta or Alpha on a stable build offers prereleases.
The signed-manifest checks stay as they are; only which releases count as candidates changes.

## Order

Each phase ends with `scripts/ci.sh` green and its own tests.

1. **Part A foundation and migration.** No user-visible change. The risky part is routing menus
   through `invoke`, so it lands last and alone.
2. **B1 and B2.** Pi 1.6, catalog-generated tools, codemode on. Measure tokens on the sample
   project's review task before and after.
3. **C2, C3, C4 and C10.** Read and stage only; cheap.
4. **B3 and B4, then C1, C8 and C5.**
5. **C6, then C9.**
6. **C7.** The only phase that adds network access, so it comes last and gets its own review.

## Decisions

- Plugins do not register capabilities in this round: it changes the plugin API and its three
  repositories. The trait is shaped so a plugin-backed capability can be added later.
- History is not grouped per agent turn: each step stays one undo, as today.
- `review` is the default mode for a project the first time Pi opens it; the archivist's choice is
  remembered per project.

## Sources

- Pi codemode, MCP and settings docs, at `earendil-works/pi` `packages/coding-agent/docs/`
  (`codemode.md`, `mcp.md`, `settings.md`, `extensions.md` "Tool Exposure" and "Calling Other
  Tools").
- Anthropic, "Introducing advanced tool use" (Tool Search Tool, programmatic tool calling):
  https://www.anthropic.com/engineering/advanced-tool-use
- Cloudflare, "Code Mode: give agents an entire API in 1,000 tokens":
  https://blog.cloudflare.com/code-mode-mcp/

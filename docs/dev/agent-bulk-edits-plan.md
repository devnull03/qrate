# Agent bulk edits plan

Status: plan. Work lands on `feature/qrate-cli`.

## Purpose

An archivist should be able to say "swap every identifier using this list" or "rename the files to
match the new ids" and get it done in one review-and-apply step. Today the agent can compute the
change (`program-run` reading a mapping file through `read_text`). It can only stage it as findings,
though: at most 200 per call (`MAX_FINDINGS`), and the person applies each one by hand from the
Fixes menu. Files can't be renamed at all, except through a raw `mv` in Pi's bash, and qrate never
sees that.

## Rule this plan keeps

Tools that write cells are a non-goal of the agent contract. That stays true here: the agent and CLI
only stage. The archivist applies, through a menu action, as one undoable step. No request, CLI
command, or Pi tool may write a cell or touch a file.

## What exists to build on

- `crates/table/src/agent.rs`: `StagedFix`, `StagedFixes`, `stage_findings`, `offer_staged`.
  Re-staging replaces an agent's previous batch, and stale-revision fixes are dropped. Keep both
  behaviours.
- `crates/diagnostics/src/fixes.rs`: `OfferGroupFixes::register` and `OfferSourceActions::register`
  / `with_source_actions` already provide per-source menu actions. Hang "apply all" there instead
  of adding new UI.
- `crates/table/src/history.rs` and `delegate.rs` (`undo`/`redo` returning `Vec<Change>`): one
  `Step` can hold many `Change`s. Bulk apply has to be one step.
- `crates/table/src/delegate.rs`: the regex replace already produces batched cell writes in source
  coordinates. Copy that shape.
- `crates/plugin-host/src/agent_program.rs`: read-only Luau, 2s deadline, 16KB output. Don't loosen
  it.

## Phases

Stop for review after each phase. Each phase is its own commit, and
`cargo clippy --workspace -- -D warnings` plus `cargo test --workspace` must pass.

### 1. Apply all staged fixes (UI only, no protocol change)

- Register a source action for each agent source: "Apply all N fixes from <agent>", plus "Discard
  all".
- Apply checks the revision first. Fixes whose cell changed since staging are skipped and reported
  ("12 applied, 3 skipped as stale").
- All writes go in one history `Step`. A single undo reverts the whole batch, and redo restores it.
- Clear applied fixes from `StagedFixes` and their findings from the Problems panel.
- Tests:
  - 500 fixes apply in one step
  - one undo restores every cell
  - a stale cell is skipped and counted
  - discarding leaves the cells untouched
  - applying with a filter or sort active writes the right source rows (follow
    `a_filter_does_not_move_the_row_indices_an_agent_is_given`)

### 2. Column rewrite request

Findings are the wrong shape for 2,000 identical edits. Add:

```rust
Request::StageColumnRewrite {
    revision: TableRevision,
    column: String,
    values: Vec<(usize, String)>, // (source row, new value)
    note: Option<String>,         // shown once in the menu, e.g. "ids from mapping.csv"
}
```

- Validation: column exists. Row indices are in range and unique. Cap at 50,000 entries. Each value
  ≤ 64KB. Add `RequestError` variants for each case; `app_control.rs::wire_name` derives their wire
  names.
- These go into the same `StagedFixes` store. They show as one grouped entry, not 2,000 Problems
  rows. The menu reads "Rewrite <column>: N rows (<note>)" with Preview / Apply / Discard. Preview
  can be a simple before→after list of the first 50 rows for now.
- Apply goes through the phase 1 path (one step, stale rows skipped).
- Add `ResultSet::Staged` reuse or a new `StagedRewrite { accepted, stale }` result.
- CLI: `qrate agent stage-column-rewrite --agent <name> < rewrite.json`, with help text and JSON
  output matching `stage-findings`.
- Pi extension (`devnull03/qrate-pi-extension`): register a matching tool. Update `SYSTEM.md` to say
  that bulk column edits go through this tool and are never done cell by cell.
- Tests:
  - round trip through the agent contract
  - over-cap input is rejected
  - duplicate rows are rejected
  - an unknown column is rejected
  - re-staging replaces the previous rewrite for the same agent and column

### 3. Staged file renames

- `Request::StageRenames { revision, renames: Vec<{ row, from, to }> }`. Paths are relative to the
  project dir.
- Validation:
  - `to` stays inside the project. Reuse the logic from `permissions.ts` `outsideProject` and port
    it to Rust.
  - No two renames share a target.
  - No target exists, unless it's also a `from` in the batch. Detect cycles (a→b, b→a) and apply
    them through temp names.
  - `from` exists and matches what the row currently points at.
- Apply happens host-side in `file-ingest`: rename on disk, then update the row's file/path column,
  all in one history `Step`.
  - Undo renames the files back.
  - If any filesystem rename fails partway through, roll back the ones already done and report.
    Don't leave the batch half-applied.
- Add `qrate agent stage-renames` to the CLI and a matching Pi tool.
- In `permissions.ts`, auto-deny bash commands containing `mv`, `rename`, `Move-Item`, or `ren` that
  touch the project dir. The deny reason should point to the `stage_renames` tool.
- Tests:
  - simple rename + undo
  - swap cycle
  - target collision rejected
  - path escape rejected
  - injected failure on the 3rd of 5 rolls back the first 2
  - the row path is updated

### 4. Docs and skill

- `docs/cli/commands.md`, `docs/cli/agents.md`, and `skills/qrate-cli/SKILL.md`: document both new
  commands, with an end-to-end example: mapping CSV → `program-run` computes the pairs →
  `stage-column-rewrite` → archivist applies.
- State once, where the agent contract's non-goals are listed: staged bulk edits are allowed because
  applying is a human menu action.

## Non-goals

- Any agent, CLI, or plugin path that writes without the menu click.
- Auto-apply settings, including "trust this agent".
- Renaming outside the project dir. Deleting files.
- Changing the read-only Luau sandbox.

## Definition of done

The sample project (`sample/`) passes this walkthrough:

1. Pi reads a mapping file and stages an identifier rewrite for every row, plus matching file
   renames.
2. The archivist applies both from the menu.
3. Two undos restore everything, on disk too.

All tests pass on Linux, macOS, and Windows CI, with no new clippy warnings.

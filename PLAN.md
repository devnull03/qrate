# qrate command-line interface

## Purpose

Make qrate projects safely inspectable and automatable by external agents and shell workflows with
stable, machine-readable commands. This is the long-term successor to the Agent Bridge for
persisted project workflows.

## Scope

1. Provide `--json` commands for project overview, bounded row queries, validation, findings, and
   exports.
2. Make command input composable through flags, files, and standard input; reserve standard error
   for progress and human diagnostics.
3. Attach revisions to reads and require an expected revision/current value for writes.
4. Require plan/preview followed by an explicit apply command for mutations.
5. Define stable exit codes and structured error output.
6. Route eligible plugin capabilities through the same command/tool contract.
7. Mark the Agent Bridge deprecated in documentation and user-facing guidance while retaining it
   during the compatibility window.

## Non-goals

- Shelling out to arbitrary plugins or granting a plugin unrestricted process access.
- Reproducing unsaved edits from a currently running qrate window.
- Removing the Agent Bridge before the CLI covers its persisted-project workflows.

## Dependencies and merge notes

The CLI may start independently of `gpui-kit-migration`. It must reuse project/domain operations
rather than coupling command behavior to GPUI panels. The Agent Bridge remains necessary for live,
unsaved window state that an offline CLI cannot read.

## Definition of done

- External tools can query and validate a project with predictable JSON and bounded output.
- Mutating operations reject stale state and cannot run without explicit apply.
- Shell pipelines and an agent integration test exercise the documented workflows.

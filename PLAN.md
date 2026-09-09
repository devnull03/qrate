# Agent tools and Islandora Workbench

This pull request defines the implementation plan. It does not add the plugin tools or Workbench
runtime.

## Purpose

Extend qrate plugins with approved, structured agent-callable tools and prove the contract through
an Islandora Workbench plugin for preflight and explicitly approved ingest workflows.

## Scope

1. Define tool declarations with stable names, descriptions, input/output schemas, and availability
   metadata.
2. Define permission categories for read-only operations, local mutations, and external side effects.
3. Implement registration and bounded invocation in the plugin host.
4. Publish matching plugin API types everywhere plugin authors receive them.
5. Integrate tool discovery and structured calls/results with the qrate Pi extension.
6. Build the Islandora Workbench plugin as the first consumer:
   - validate configuration and selected source rows;
   - produce a Workbench plan and preflight/dry-run report;
   - expose direct ingest only as an explicitly confirmed external side effect;
   - return structured reports suitable for an agent to explain.

## Non-goals

- A speculative generic tool catalogue with no real plugin consumer.
- Arbitrary agent execution of plugin code.
- Silent or AI-initiated Islandora ingest.

## Dependencies and merge notes

This branch is blocked by the plugin registry/marketplace work. Workbench is deliberately developed
in this branch so every capability and permission decision is exercised by preflight and ingest,
rather than a synthetic example.

## Definition of done

- An enabled plugin exposes only its declared, approved tools.
- Tool calls validate inputs and return structured results/errors.
- Preflight is usable without ingest credentials or side effects.
- Ingest requires visible human confirmation and leaves an auditable result.

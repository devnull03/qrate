# Learn-First Collaboration

This project uses **teach-first pairing**. The goal is for the user to learn the rationale and mechanics of the codebase, not just to receive automated patches.

## Core Directives

- **Explanations Over Edits:** Do not perform unsolicited code changes. Unless the user explicitly says "implement", "apply", "just fix it", or similar, prefer explanations, rationales, and small illustrative snippets or pseudocode over direct edits.
- **Explicit Rationales:** When asked to edit the codebase, always state the rationale (trade-offs, principles, or patterns) before or alongside the change.
- **Diagnosis Before Treatment:** When something breaks or is confusing:
  1. **Skills Check** — Ask 1–3 targeted questions or propose a tiny exercise to verify the user's understanding of the underlying concept (e.g., lifetimes, trait bounds, borrow checker rules). Skip only for trivial fixes.
  2. **Deep Diagnosis** — Identify the underlying cause, not just the symptom.
  3. **Theoretic Fix** — Explain *why* the proposed approach resolves the root cause. Distinguish between a "minimal fix" and the "ideal design."

## Handling Ambiguity

If a request is ambiguous, ask whether the user wants:
- **Pedagogy only** — explanation or theory
- **Guided steps** — steps for the user to type themselves
- **Full implementation** — the agent writes the code

## Proficiency Tracking

**Primary source of truth: Notion** — [Learning Progress](https://www.notion.so/37221d32b13b81dca40ee9176e8ddc0f)

At the start of a teaching-heavy thread, check the Notion page for proficiency levels and calibrate depth:
- **4–5 (comfortable/expert):** go deeper or faster, skip basics
- **1–2 (new/basic):** use standard skills-check depth, go slower

After a substantive learning exchange, update the Notion page:
- Bump `times_practiced`
- Adjust `proficiency` (1–5 scale)
- Set `last_touched` to today's date (YYYY-MM-DD)
- Add a concise note summarising what was covered

**`learnt.md` (backup only):** If a Notion connection is unavailable, fall back to `learnt.md`. On the next successful Notion sync, delete `learnt.md`.

**Proficiency scale:** 1 = first exposure · 2 = basic understanding · 3 = usable with prompts · 4 = comfortable applying · 5 = comfortable teaching

## Project Status Tracking

**Primary source of truth: Notion** — [qrate project hub](https://www.notion.so/devnull03/qrate-35921d32b13b8034accec1cef966fd61), specifically the **Tasks Tracker** database inline on that page.

At the start of a work session (or when asked "what's next"), check the Tasks Tracker for current status, priority, and dependencies (`Blocked by`/`Blocking`) instead of inferring "next steps" from git log alone — git history shows what shipped, not what's planned or why.

Each task page has two collapsible sections worth reading before starting work:
- **🤖 Claude Code agent instructions** — scope, pinned deps, task breakdown, definition of done.
- **🧠 Agent memory log** — append-only, newest entry on top. Add a dated bullet each session (what was tried, what worked/didn't, decisions made, commit links) before ending work on a task. Never delete or rewrite prior entries.

The hub also has a **Document Hub** database (design docs, specs) linked from tasks via `Related docs` — check it for additional context when a task references a doc that isn't fully explained inline.

`notion-query-data-sources` in SQL mode works on this workspace and is the fastest way to read the tracker — a single-data-source query returns all rows in one call, no pagination. Multi-data-source queries are the Enterprise-gated ones. Use `notion-fetch` on a task URL when you need its page content (the agent instructions and memory log live in the body, not in properties).

### Creating tasks

Notion is where a task is born. Never create a Notion task from a GitHub issue — make it in Notion first, then sync it out to GitHub and record the issue URL in the task's `GitHub Link` property. The Notion UUID goes in the **PR body** (`Closes #N (Notion ID: <uuid>)`), never in the issue description.

## Code Style — no bloat

A 2026-07-17 whole-repo audit removed exactly this kind of code; don't reintroduce it:

- **No single-line forwarding wrappers.** A function whose body is one call to another function with no added logic gets deleted — make the target `pub` and call it directly (e.g. the removed `persist_layout_on_quit`). Exception: thin accessors that are the only route to a private field (`QrateTableDelegate::cell`/`row_image`) are encapsulation, keep those.
- **No `new()`/`Default` for fieldless unit structs.** Construct `StatusBar`-style structs directly: `cx.new(|_| StatusBar)`.
- **One builder function per UI component, conditions inline.** Don't split a gpui component into a helper fn per visual part — use closures, `FluentBuilder::map`/`when`, and `match` inside a single function (see `render_image_frame` in `crates/workspace/src/panels/details.rs`). If a builder is genuinely shared across multiple `Render` impls, return `AnyElement` — propagating gpui's nested builder generics into several callers overflows rustc's stack at type-check time.
- **No speculative scaffolding.** Code "kept for later" outside the module tree, provider stubs full of `todo!()`, and builder methods only tests call are deletions, not investments — git history is the archive. **One deliberate exception, do not flag or delete it:** the `ai` crate (`crates/ai/` — traits + Cohere/mock providers) is the planned home for AI review/embedding and stays despite its `todo!()` bodies.
- **Explanation comments.** If you have to write a comment of more than 1 line to justify a decision, that is the wrong decision. Go back and rethink the implimentation from the start in a different manner.

## gpui test modules

- `#[gpui::test]` needs `gpui = { workspace = true, features = ["test-support"] }` under `[dev-dependencies]`.
- **Never `use super::*` in a tests module whose parent file has `use gpui::*`** — the chained glob makes gpui's `test` proc-macro shadow the built-in `#[test]` its own expansion emits, so it expands into itself forever ("recursion limit reached" that no `recursion_limit` bump fixes, ending in a rustc stack overflow). Import explicitly; never add `#![recursion_limit]` for this.

## Git Workflow

- **Commit message format:** Conventional Commits — `type(scope): summary`, imperative mood, one blank line then body if needed (e.g. `feat(table): extract table into its own crate`). Match the style already in `git log`.
- **Never add Claude as a co-author.** Do not include a `Co-Authored-By: Claude` (or any Anthropic/Claude) trailer in any commit — this overrides the default Claude Code behavior. No exceptions unless explicitly told otherwise for that one commit.
- Only commit when explicitly asked; stage specific files by name rather than `git add -A`/`git add .`.
- **Work directly on the `main` branch — do not create git worktrees.** Zed's file watcher holds worktree dirs open on Windows, making them a pain to delete. Branch only when explicitly asked.
- A shared pre-commit hook (`.githooks/pre-commit`) runs `cargo fmt --all --check` so unformatted code can't be committed. Enable it once per clone: `git config core.hooksPath .githooks`.

### Pull requests

- **Before opening any PR, reproduce CI locally and make it green.** CI (`.github/workflows/ci.yml`; Windows + macOS only — gpui won't build on Linux CI) runs exactly these three, in order — run the same before every PR and don't open it until all pass:
  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings -A dead_code
  cargo test --workspace
  ```
  Clippy allows `-A dead_code` (this repo scaffolds UI ahead of its consumers) but hard-fails every other warning. `clippy --all-targets` + `test` already compile everything, so there's no separate build step. Scope the checks to affected crates while iterating, but run the full `--workspace` form once before pushing.
- Open the PR with `gh pr create`; PRs target `dev` or `main` (the branches CI gates). Body gets a short summary + a **Verification** line stating which of the four checks you ran and that they passed.
- **After opening the PR, check for a GitHub Copilot code review and audit it.** Fetch its comments (`gh pr view <n> --comments`, or `gh api repos/{owner}/{repo}/pulls/{n}/comments`), then for each suggestion decide implement vs. dismiss: apply the correct/worthwhile ones and push to the same branch, skip false positives and noise. Report back which you applied vs. dismissed and why — don't blindly accept or ignore the whole review.

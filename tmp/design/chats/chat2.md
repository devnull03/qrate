# New feature page design

_Started 2026-09-28 17:31 UTC_

---

## User

Continuing from "Onboarding flow UI mockups".

## Assistant

We were building UI mockups for an onboarding flow across the Qrate app—both the launcher (first run and returning) and workspace views. The goal was to familiarize new users with core features: file matching, column setup, media linking, and exporting.

We created two canvas files covering the launcher wizard (frames 1–5) and workspace states (frames 6–8). Each included both light and dark theme variants with tweaks to show one at a time. The launcher walked through welcome, file selection, column configuration, and project creation. The workspace showed a blank project with the Getting started card in three possible placements, an imported project with tips, linked media views, and export guidance.

Key decisions made: we kept some details as educated guesses pending verification (Details panel layout, Problems messages, Export menu names, and the breakpoint for the guide collapsing to the status bar). We tagged any changes requiring code beyond onboarding as PROPOSED (making File optional, adding the example project). We used Qrate's own Light/Dark colours from the repo since no default theme was set. Icons were hand-inlined from lucide rather than copied from the library.

A layout check flagged several issues. We fixed the critical one: frame 9c (keyboard focus) had the third close-up running past the card's right edge—we widened the card to 860px and allowed the row to wrap. Other warnings about cards sitting atop the grid and table rows being cut off by window edges are intentional, reflecting real app behaviour.

Current state: both canvas files are updated and layout-validated. The Getting started card placement options are ready for you to choose from.

Next steps: verify the guessed details against the actual codebase, decide which Get started card placement works best, and confirm whether the PROPOSED features need implementation.

Files: qrate Onboarding - Launcher & Wizard.dc.html, qrate Onboarding - Workspace.dc.html, github.md

## User

new feature, make a new page to design this one.

## Assistant

_[tool: read_file]_

_[tool: list_files]_

_[tool: list_files]_

_[tool: read_file]_

_[tool: grep]_

_[tool: read_file]_

_[tool: read_file]_

_[tool: dc_write]_

_[tool: ready_for_verification]_

I've added a new page, `qrate Pop-out Viewer.dc.html`. Frames 1a–1c show the Pop out button in three states: default, on hover, and when the window is already open. Frames 2a–2m show the window in every state from the brief, in Light and Dark. Two tweaks switch the theme and the viewing-area background.

The main decisions:
- **Background behind the file:** it stays black at 85% opacity, the same as the full-screen viewer. The title bar and sidebar follow the theme. You can switch the tweak to "Theme stage" to compare.
- **Sidebar:** on the right, 320px by default, resizable from 240 to 720px, and collapsed with the same panel button the main window uses.
- **Find:** for documents it's a tab in the sidebar ("Details | Find"), not a third column, so the window still works at 800px wide.
- **Pausing:** a "Following / Pinned" toggle in the title bar. While pinned, a chip shows which row the table is on, with a Jump button.

These are my guesses, tagged TO CHECK on the page:
- In the several-rows state, stepping through the stack uses Alt+←/→, because ←/→ already turn pages.
- The wording of the "component not installed" banner and its Install… button are placeholders.

There's no frame at 4K size. The note on 2m says the sidebar keeps its width there and the file area takes the rest.


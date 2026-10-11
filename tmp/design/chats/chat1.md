# Onboarding flow UI mockups

_Started 2026-09-25 05:24 UTC_

---

## User

Make UI mockups for an onboarding flow for the whole app to familarize the user with how to use the app, use this info about what the flow would be and how the app does things right now:

## Assistant

_[tool: update_todos]_

_[tool: github_read_files]_

_[tool: github_get_tree]_

_[tool: github_read_files]_

_[tool: github_get_tree]_

_[tool: github_read_files]_

_[tool: github_read_files]_

_[tool: github_read_files]_

_[tool: github_copy_files]_

_[tool: github_search_code]_

_[tool: view_image]_

_[tool: view_image]_

_[tool: github_search_code]_

_[tool: run_script]_

_[tool: github_search_code]_

_[tool: write_file]_

_[tool: update_todos]_

_[tool: dc_write]_

_[tool: show_html]_

_[tool: dc_write]_

_[tool: dc_js_str_replace]_

_[tool: dc_html_str_replace]_

_[tool: update_todos]_

_[tool: ready_for_verification]_

I've built the mockups as two canvas files. Each frame appears in Qrate Light and Qrate Dark, with a Tweak to show one theme only. They're out for a background check now.

- **Launcher & Wizard:** frames 1–5.
  - First-run welcome in the empty Recent Projects area, plus a variant with the example-project path marked as proposed.
  - Returning launcher with the new Getting started and User guide entries in Help.
  - Files step in three states: full match, partial match, and add files later.
  - Columns step: one version that explains the current Title + File rule, and one where File can wait until media is linked.
  - Create review with the project path, record count, file-match count and a note that media stays in its folder.
  - For comparison, the current empty launcher and Files step recreated from the source.
- **Workspace:** frames 6–8 and the other states you asked for.
  - Blank project, with the Getting started card in three places for you to choose from: floating in the grid's corner, at the top of Details, or in the status bar.
  - Imported project with a tip on the Problems panel.
  - Linked media: the Table-to-Gallery tip, then Gallery with Details on the right.
  - Dismissed, reopened from Help, and keyboard-focus states.
  - The export tip.
  - The narrow 520 × 300 window.

**Caveats:**
- The code has no default theme set, so I used the repo's own Qrate Light/Dark colours. The README screenshots use a different, yellow-accent dark theme.
- Icons are hand-inlined copies of the lucide set gpui-component uses; I didn't copy them from the library.
- Some details are my own guesses and would need checking against the real UI:
  - the Details panel layout, since I only skimmed `details.rs`
  - the wording of the Problems messages
  - the names in the Export submenu
  - the threshold where the guide collapses to the status bar
  - the Ctrl+2 shortcut shown on the Gallery tip
- Anything that needs code changes beyond onboarding is tagged PROPOSED: making File optional (4b) and the example project (1b).

`github.md` records which repo files each screen was built from.

## User

Found issues — fixing…

## Assistant

_[tool: dc_html_str_replace]_

_[tool: ready_for_verification]_

I fixed the one layout problem the check found: in 9c (keyboard focus), the third close-up ran past the card's right edge. The card is now 860px wide and its row can wrap.

The other layout warnings are intentional. The Getting started cards and tips sit on top of the grid, and the last table rows are cut off by the edge of the window, as they would be in the app.

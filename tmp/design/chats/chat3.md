# Missing file with annotations

_Started 2026-10-02 18:55 UTC_

---

## User

Continuing from "New feature page design".

## Assistant

We started a new feature: a Pop-out Viewer window for the Qrate app. The goal is to let users open files in a detached window while keeping the main app accessible, with support for browsing stacks, finding content, and pinning specific rows.

I created a new page, `qrate Pop-out Viewer.dc.html`, with frames covering the Pop out button states (1a–1c: default, hover, window-open) and the viewer window across all states described in your brief (2a–2m in Light and Dark themes).

Key design decisions:
- The background behind files stays black at 85% opacity (matching full-screen viewer). Title bar and sidebar follow the app's theme.
- Sidebar is 320px by default, resizable between 240–720px, collapsing with the same panel button as the main app.
- Find is a sidebar tab ("Details | Find") for documents, not a separate column—keeps the window viable at 800px wide.
- Pinning uses a "Following / Pinned" toggle in the title bar; when pinned, a chip shows the current table row with a Jump button.

I've marked several details as TO CHECK pending code verification: Alt+←/→ for stepping through stacks (since ←/→ navigate pages), the exact wording of the "component not installed" banner and Install button, and sidebar width behaviour at 4K (currently noted as keeping its width while the file area expands).

Current state: all frames are laid out and themed. The page is ready for review.

Next steps: verify the navigation shortcuts and banner copy against the codebase, confirm sidebar behaviour at extreme sizes, and decide if any refinements are needed before handoff.

Files: qrate Pop-out Viewer.dc.html

## User

also make sure to make a design for when someone opens a file which has annotation but the original file and thumbanil is missing.

## User

Found issues — fixing…

## User

Found issues — fixing…

## Assistant

_[tool: dc_js_str_replace]_

_[tool: dc_html_str_replace]_

_[tool: ready_for_verification]_

I fixed the error that left the annotations page blank; a fresh check of all sections, 1 to 5, is running now.

## User

Found issues — fixing…

## Assistant

_[tool: dc_js_str_replace]_

_[tool: dc_js_str_replace]_

_[tool: dc_js_str_replace]_

_[tool: ready_for_verification]_

I fixed the Drawing frame (1c). The dashed outline should now sit around the area being dragged, and the crosshair should sit at the drag's end instead of the top-left corner. A fresh check of the page is running.

## User

the annotations hover effect needs some updating, i want it to show the whole comment, when it was made and the author name. (we don't have pfp so remove those). this same new hover card for annotations will be used now for table notes

## Assistant

_[tool: github_search_code]_

_[tool: github_read_files]_

_[tool: dc_html_str_replace]_

_[tool: dc_html_str_replace]_

_[tool: dc_html_str_replace]_

_[tool: dc_html_str_replace]_

_[tool: dc_js_str_replace]_

_[tool: dc_js_str_replace]_

_[tool: dc_js_str_replace]_

_[tool: dc_js_str_replace]_

_[tool: dc_js_str_replace]_

_[tool: ready_for_verification]_

I've redesigned the annotation hover card; the page didn't finish loading for my check, so it isn't confirmed yet. A review is running in the background.

- **New hover card:** it now shows the whole note, then a muted line with the author's full name and when the note was filed, for example "Anna Moreau · 2026-10-02 14:20". An edited note adds "edited", and an unsigned one reads "Unsigned". The initials are gone, and the card flips above the region when it would run off the bottom of the viewer. This is in 1a, 1h and 1k.
- **Table notes (new section 6):**
  - **6a:** the same card when hovering a cell that has a note. It opens after the grid's existing 600ms delay, and the corner marker on the cell is unchanged.
  - **6b:** a cell that also has a column description and a validator error. Today's tooltip shows those two; in the new card they move under a divider below the note. This is marked TO CHECK.


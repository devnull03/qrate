# Google Sheets parity: what qrate deliberately does not have

Companion to `menu-handoff.md`. That file is the menu roadmap; this one is the list of Sheets
features the menus **do not** show, and why.

The menu bar and the grid's context menus were regrouped to follow Google Sheets' information
architecture. Anything planned and tracked appears as a greyed-out item naming its task
(ASNT-44 text wrapping, ASNT-72 gallery view, ASNT-79 merge/explode rows,
ASNT-73 AI review). Everything below has no task, so it has no menu entry either — a greyed-out
item that points at nothing is a promise nobody made.

## Should have, needs a task

| Sheets feature | What it needs first |
|---|---|
| Sort A→Z / Z→A, sort range | A sort descriptor persisted apart from the source data, and visible rows built by filtering then sorting row identities — `menu-handoff.md` M5. The most obviously missing item; a cataloguer expects to sort by title. |
| Find and replace | Find exists (Ctrl+F). Replace needs the match set to route through `apply_edit` as one undo step. |
| Paste special (values only, transposed) | Ordinary paste landed first; qrate has no cell formatting, so "values only" is currently a distinction without a difference. Revisit if cells ever carry formatting. |
| Hide / resize row | Columns already resize. Hiding rows competes with filters, which are visible and reversible — `menu-handoff.md` argues hidden-row state is easy to lose. Resizing rows is coupled to ASNT-44 (wrapped text needs variable row height). |
| Remove duplicates, trim whitespace | Real archival cleanup work, but each needs a preview and one-step undo before it is safe to offer. |
| Split text to columns | ASNT-70 supplied both the column insert and the stable (name-based) column identity, so all that is left is its own delimiter UI. |
| Data validation UI | The rules exist as plugin/validator output in the Problems panel. A Sheets-style per-column rule editor is a settings surface, not a menu item. |

## Should not have

| Sheets feature | Why not |
|---|---|
| Charts, pivot tables, slicers | qrate is a cataloguing editor, not an analysis tool. Export to CSV and analyse elsewhere. |
| Formulas, formula bar, named functions, show formulas | Cells hold catalogue values, not expressions. A formula engine would change what the file means. |
| Text/fill colour, font, bold/italic, borders, merge cells, text rotation, alternating colours | Presentation formatting has nowhere to live: a `.qrate` project stores data, and exports are CSV/JSON-LD/CSL. Alternating row stripes already exist as a display setting, not a cell property. |
| Conditional formatting | The equivalent already exists and is better suited: validators paint squiggles and file findings in the Problems panel. |
| Multiple sheets, sheet tabs, protected ranges, named ranges | One project is one dataset. Nothing in the model has a second grid to name or protect. |
| Sharing, comments/discussions, version history, offline, publish to web | Single-user desktop app over local files. Version history is git's job for the project folder. |
| Macros, script editor, AppSheet | The Lua plugin system is qrate's extension surface, and it is the one that ships type definitions to plugin authors. A second scripting layer would split it. |
| Print | Export covers getting data out. A print layout for a 10k-row grid is its own project. |
| Zoom in/out, gridlines toggle, full screen | Window-level chrome the OS and the theme already handle. |
| Group and outline, column stats, data connectors, refresh connected data | Spreadsheet-workbook concepts with no counterpart in a flat catalogue. |

## If any of these get built

Create the task in Notion first, fan it out to GitHub and Linear (see `CLAUDE.md`), then add the
menu item greyed out with the task named in a comment beside it — the same pattern
`crates/app/src/app_menus.rs` uses for the row/column entries. Move the row out of this file when
it stops being a decision and becomes work.

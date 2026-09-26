# The grid

The grid is the main view of a project. Each row is a record. Each column is a field.

## Editing

Click a cell to select it. Type to replace its content, or press Enter to edit in place.
Copy, cut, and paste work across one cell or a selected range, including between qrate and
other spreadsheet apps. **Delete** or **Backspace** clears the selected cells, and **Esc**
drops the selection. On macOS, use Cmd wherever this guide says Ctrl. Every shortcut is listed
in [Keyboard shortcuts](shortcuts.md).

qrate saves your edits to the project file on its own. See [Saving](projects.md#saving).

Every edit goes on the undo stack, including row and column changes. Press **Ctrl+Z** to
undo and **Ctrl+Shift+Z** (or **Ctrl+Y**) to redo. Undo and redo also cover adding,
removing, and reordering rows and columns, so a structural change is as safe to try as a
cell edit. Undo goes back 200 steps by default. **Settings ▸ Table ▸ Editing ▸ Undo steps**
raises that to 500, 1,000, or 2,000; each step keeps what it replaced, so a deeper history uses
more memory.

**Settings ▸ Table ▸ Appearance ▸ Row density** chooses **Comfortable** (the default) or
**Compact**, which uses smaller text and fits more rows on screen. Undo steps and row density
are defaults a project can override; see
[Projects](projects.md#your-defaults-and-project-overrides).

## Rows and columns

Right-click a row header for a menu to insert, duplicate, clear, or delete rows. Right-click a
column header to insert, delete, or rename a column, or choose **Freeze up to here** to keep
the columns up to that one in view while you scroll. **View ▸ Unfreeze All Columns** releases
them. The **Insert** and **Data** menus offer the same commands for the current selection, and
they work even while another panel has focus. **Alt+Shift+Up** and **Alt+Shift+Down** insert a
row above or below the selection, and **Ctrl+-** deletes the selected row.

Drag a column header to move the column. Drag a row header to move the row; see
[Groups](#groups).

## Groups

Rows can nest inside other rows, the way a series holds files and a file holds items. A
project created from a folder tree starts nested; any project can be arranged by hand. A row
with children shows a chevron in its header. Click it to collapse or expand the group, or press
**Ctrl+Alt+Right** and **Ctrl+Alt+Left** to expand or collapse every group. **View ▸ Expand All
Rows** and **View ▸ Collapse All Rows** do the same.

- **Group rows.** Select rows, right-click a row header, and choose **Group** (the item
  reads, for example, **Group 3 rows**). qrate inserts a new parent row above them and moves
  them inside it.
- **Indent and outdent.** **Ctrl+]** makes a row the last child of the row just above it at
  the same level. **Ctrl+[** moves it out to sit just after its parent. **Data ▸ Indent Row**
  and **Data ▸ Outdent Row** do the same.
- **Drag.** Drop a row header on the top edge of another row to place it before that row, on
  the middle to make it a child, or on the bottom edge to place it after. A row always moves
  with its children.
- **Delete.** Deleting a parent row offers **Delete and promote children**, which keeps the
  children, or **Delete row and descendants**.

Every arrangement change is one undoable step. qrate remembers which groups you left expanded.

## Search and replace

Open search with **Ctrl+F**. Search moves through matching cells in the grid. Open replace
with **Ctrl+H** to replace one match or all matches at once. The search bar belongs to
whichever view is showing, so a search carries on when you switch between the grid and the
gallery.

Two toggles in the search bar look beyond the cells, and both narrow the view to the rows
they match. The book searches the text inside linked PDFs. The picture searches what the
linked images show, without using their metadata at all. See
[Visual search](visual-search.md).

## Filtering

Click the filter icon in a column header to open its filter. If a column has no filter icon,
right-click its header and choose **Filter ▸ Show filter dropdown**, or pick the columns under
**Settings ▸ Columns ▸ Filters**. To keep only the rows that hold one value, right-click a cell
holding it and choose **Filter by this value**. It lists every distinct value
in that column as a checklist. Uncheck a value to hide the rows that hold it. The grid
shows only rows that pass every active column filter at once.

Filtering hides rows. It does not change or delete data, and it does not affect export or
diagnostics, which both still see every row.

## Gallery view

Switch the grid to a gallery of thumbnails with **View ▸ Switch View ▸ Gallery**, or press
**Ctrl+2**. **Ctrl+1** returns to the table. The gallery shows one tile
per row, using the file or photo linked to that row. It is a way to review a collection by
image instead of by cell, useful for photo collections in particular.

## Notes

Right-click a cell and choose **Notes ▸ Add note…**, use **Insert ▸ Note…**, or press
**Shift+F2** to attach a free-text note to it. A cell with a note shows a small corner marker. Notes appear on the
Notes tab of the Problems panel. If you set an author name in **Settings ▸ Application**, each
new note records it.

Excel exports keep notes as cell comments, and Google Sheets exports add them as cell notes.
CSV, JSON-LD, CSL-JSON, and ZIP exports leave them out.

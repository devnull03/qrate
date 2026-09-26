# Step 5: Save and export

[Getting started](index.md) ▸ Step 5 of 5

## How qrate saves

qrate saves your edits to the `.qrate` file automatically. By default, it saves after a short
pause in your typing. Column settings and other project settings save when you change them.

A dot in the title bar means that some changes are not saved yet. To save at once, press
**Ctrl+S** or choose **File ▸ Save**.

To change when qrate saves, open **Settings ▸ Table ▸ Saving**. **Autosave** turns automatic
saving on or off. **Method** chooses **After a short pause** or **On every edit**.

If a save fails, qrate tells you why and keeps your changes. Before qrate closes, it asks
about unsaved changes: **Save**, **Don't Save**, or **Cancel**.

To move the project, copy the `.qrate` file. If you also move the files folder, set its new
location in **Settings ▸ Project**.

## Export the project

1. Choose **File ▸ Export**.
2. Choose a format, for example **CSV…** or **Excel (.xlsx)…**.
3. Select a folder and a file name. qrate suggests a name from the project name.
4. Click **Save**.

qrate writes the file in the background. A notice tells you when the export is done, or why it
failed.

Choose the format that the next system needs:

| Format | Use it to |
|---|---|
| CSV | Load the catalog into almost any other system. |
| Excel (.xlsx) | Share the catalog as a workbook. Cell notes become comments. |
| JSON-LD | Give the records and their hierarchy to a linked-data system. |
| CSL-JSON | Load the records into a reference manager. |
| ZIP Archive | Keep the data as CSV and JSON-LD, together with the linked files. |

An export always includes every row, including the rows that a filter hides. To export to a
Google Sheet, first turn on **Settings ▸ Google ▸ Enable Google Sheets**. See
[Export and Google Sheets](../export-and-sync.md) for the CSV options and for Google Sheets.

## Next steps

You now know the full path, from a new project to an export. These reference pages explain each
part in detail:

- [Projects](../projects.md): open, import, and save projects, and the project settings
- [The grid](../grid.md): rows, columns, groups, search, filters, the gallery, and notes
- [Keyboard shortcuts](../shortcuts.md): every shortcut in one table
- [Files and photos](../files-and-photos.md): linking, missing files, and the viewer
- [Visual search](../visual-search.md): find records by what their pictures show
- [Diagnostics](../diagnostics.md): the checks, the Problems panel, and fixes
- [Columns](../columns.md): column types, authority lists, and per-column settings
- [Export and Google Sheets](../export-and-sync.md): every export format and Google Sheets sync
- [The Agent panel](../agent-panel.md): let a local AI agent review the project
- [Plugins](../plugins/index.md): add checks, commands, and export formats

Back to [Getting started](index.md)

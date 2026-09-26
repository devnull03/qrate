# Export and Google Sheets

## Exporting

Export the project from **File ▸ Export** as one of:

- **CSV**
- **Excel (.xlsx)**, with cell values kept as text so identifiers and dates stay unchanged.
  Project notes become cell comments.
- **JSON-LD**. Each row records the row it is part of, so the archival arrangement
  survives. See [Groups](grid.md#groups).
- **Zotero (CSL-JSON)**
- **ZIP Archive**, which bundles the data as CSV and JSON-LD with the linked files it points
  to. Files imported from folders keep the folder paths they were imported from.

Plugins can add their own formats to the same menu.

The save dialog suggests a file named after the project, such as `My Collection.csv`. qrate writes
the export in the background and shows a notice when it is done, or the reason it failed. A ZIP
export copies every linked file, so it shows its progress as it goes and offers **Cancel**. The
built-in formats are written to a temporary file first, so a cancelled or failed export leaves
any existing file at that path unchanged.

Export always reads every row, regardless of any active filter. See
[The grid](grid.md#filtering).

No qrate installed? [Convert a project in your browser](https://qrate.dvnl.work/convert).

## Google Sheets

Google Sheets export and sync is off by default. Turn it on in **Settings ▸ Google**. Until
you do, qrate shows no Google Sheets item in its menus.

Once it is on, you can:

- **Export to a new Google Sheet** with **File ▸ Export ▸ New Google Sheet…**.
- **Sync to an existing sheet** with **File ▸ Export ▸ Sync to Google Sheet…**. qrate writes
  the project into the sheet this project is linked to, or into one you pick through Google's
  file picker. Sync replaces the entire contents of the spreadsheet's first tab: rows and
  columns beyond the project's are cleared, and so are the notes an earlier sync left there.
  Before it writes, qrate names the spreadsheet and asks you to confirm with **Replace**. Sync
  goes one way, from qrate to the sheet; edits made in the sheet do not come back into qrate.

qrate writes the new values before it clears anything, so a sync that fails partway does not
leave the sheet empty. A notice reports when the export or sync is done, or why it failed.

Project notes are added to the corresponding cells in Google Sheets. Notes on columns
are added to the header cells.

Sign-in happens on your own machine. qrate never sees your Google password, and it can
reach only the sheets it created or that you picked yourself through Google's file picker.
No qrate account or hosted service is involved at any point.

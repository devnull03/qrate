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
  file picker, replacing what the sheet's first tab held. Sync goes one way, from qrate to the
  sheet; edits made in the sheet do not come back into qrate.

Project notes are added to the corresponding cells in Google Sheets. Notes on columns
are added to the header cells.

Sign-in happens on your own machine. qrate never sees your Google password, and it can
reach only the sheets it created or that you picked yourself through Google's file picker.
No qrate account or hosted service is involved at any point.

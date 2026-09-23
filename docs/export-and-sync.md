# Export and Google Sheets

## Exporting

Export the project from the **File** menu as one of:

- **CSV**
- **Excel (.xlsx)**, with cell values kept as text so identifiers and dates stay unchanged. Project notes become cell comments.
- **JSON-LD**
- **CSL-JSON**
- **A ZIP archive**, which bundles the exported data with the linked files it points to.

Export always reads every row, regardless of any active filter. See
[The grid](grid.md#filtering).

No qrate installed? [Convert a project in your browser](https://qrate.dvnl.work/convert).

## Google Sheets

Google Sheets export and sync is off by default. Turn it on in **Settings ▸ Google**. Until
you do, qrate shows no Google Sheets item in its menus.

Once it is on, you can:

- **Export to a new Google Sheet.**
- **Sync an existing sheet**, so qrate and the sheet stay in step with each other.

Project notes are added to the corresponding cells in Google Sheets. Notes on columns
are added to the header cells.

Sign-in happens on your own machine. qrate never sees your Google password, and it can
reach only the sheets it created or that you picked yourself through Google's file picker.
No qrate account or hosted service is involved at any point.

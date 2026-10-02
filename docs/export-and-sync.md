# Export and Google Sheets

## Exporting

Export the project from **File ▸ Export** as one of:

- **CSV**
- **Excel (.xlsx)**, with cell values kept as text so identifiers and dates stay unchanged.
  Project notes become cell comments.
- **JSON-LD**. Each row records the row it is part of, so the archival arrangement
  survives. See [Groups](grid.md#groups).
- **Zotero (CSL-JSON)**
- **IIIF Manifest**, which describes the project and its linked files for IIIF viewers such as
  Mirador and Universal Viewer. See [IIIF manifests](#iiif-manifests).
- **ZIP Archive**, which bundles the data as CSV and JSON-LD with the linked files it points
  to. Files imported from folders keep the folder paths they were imported from.

Plugins can add their own formats to the same menu.

To convert a `.qrate` file on a machine without qrate installed, use the
[online converter](https://qrate.dvnl.work/convert). It runs in your browser, so the project
never leaves your machine unless you send it to a Google Sheet.

**Settings ▸ Table ▸ CSV export** shapes the CSV, including the `data.csv` inside a ZIP
archive:

- **Byte order mark**, on by default, starts the file with a UTF-8 marker. Excel on Windows
  needs it to show accented letters correctly; turn it off for an older import script that does
  not expect it.
- **Separator** is **Comma** (the default), **Semicolon**, or **Tab**. Excel in regions that
  write decimals with a comma expects semicolons.

Both are defaults a project can override; see
[Projects](projects.md#your-defaults-and-project-overrides).

The save dialog opens in the folder this project last exported to, or beside the project file
the first time. It suggests a file named after the project, such as `My Collection.csv`. qrate writes
the export in the background and shows a notice when it is done, or the reason it failed. A ZIP
export copies every linked file, so it shows its progress as it goes and offers **Cancel**. The
built-in formats are written to a temporary file first, so a cancelled or failed export leaves
any existing file at that path unchanged.

Export always reads every row, regardless of any active filter. See
[The grid](grid.md#filtering).

No qrate installed? [Convert a project in your browser](https://qrate.dvnl.work/convert).

## IIIF manifests

**File ▸ Export ▸ IIIF Manifest…** writes the project as one
[IIIF Presentation API 3.0](https://iiif.io/api/presentation/3.0/) manifest. Every row that
links a file becomes one item in it, in table order, and the archival arrangement becomes the
viewer's table of contents. See [Groups](grid.md#groups).

A manifest names everything by web address, and qrate does not host your files. So the export
first asks for the web address of the folder you will publish the manifest in, such as
`https://example.org/iiif/harbour`. qrate remembers the address in the project. To publish:

1. Export the manifest. Keep the name `manifest.json`, because the manifest's own address ends
   in it.
2. Export a **ZIP Archive** and unpack it. Its `files` folder holds every linked file at the
   path the manifest expects.
3. Upload `manifest.json` and the `files` folder into the folder at that web address.

A viewer on another site can load the files only if your web server allows it, which servers
call CORS.

What goes into the manifest:

| In the project | In the manifest |
|---|---|
| The project's name | The manifest's label |
| The **Title** column, or else the **Identifier** column | Each item's label |
| Every cell that is not empty | Each item's metadata, under the column's name |
| A **Date** column holding one year, month, or day, such as `1943` or `1943-06-12` | The item's navigation date |
| A column named **Rights** or **License** holding a Creative Commons or RightsStatements.org web address | The item's rights |
| A row that groups other rows | A section in the table of contents |

A date range, an approximate date, or a rights note in words stays in the metadata, where a
viewer still shows it.

Which files a manifest can present:

- **Pictures**: JPEG, PNG, GIF, WebP, BMP, TIFF, AVIF, and JPEG 2000. Web browsers cannot show
  TIFF or JPEG 2000 directly, so convert those for viewing or publish them through an image
  server.
- **Recordings and video**, with their running time. Video needs ffmpeg.
- **PDF documents**, at the size of their first page. This needs PDFium. Not every
  viewer shows documents: Universal Viewer does, and Mirador shows only their description.

ffmpeg and PDFium are the same parts that draw previews. See
[Viewing a file](files-and-photos.md#viewing-a-file).

When the export finishes, its notice says how many rows went in and why any were left out: a
row with no linked file, a file type IIIF cannot present, or a file whose size or length could
not be read. qrate leaves those rows out instead of writing a manifest that viewers reject.

qrate writes manifests for the Presentation API only. It does not serve images, so it is not a
IIIF Image API server, and the manifest does not refer to one.

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

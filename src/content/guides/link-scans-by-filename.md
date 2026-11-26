---
title: Link an existing spreadsheet to its scans by filename
description: Already have a catalogue spreadsheet and a folder of images? Bring both into qrate so every row shows its scan, and missing files get flagged.
published: 2026-11-26
tags: [files, import]
related:
  - { label: 'Files and photos', href: '/docs/files-and-photos' }
  - { label: 'Projects: create a project', href: '/docs/projects#create-a-project' }
  - { label: 'Diagnostics', href: '/docs/diagnostics' }
---

A common starting point: a CSV or Excel sheet somebody has been filling in for years, and a
folder of scans named after the records in it. The sheet says `MS-0042` and the folder has
`MS-0042_001.tif`. Nothing connects the two except a person flipping between windows.

qrate joins them. Once a row is linked to its file, the scan shows beside the row while you
describe it, and qrate tells you which rows point at a file that is not there.

## What you need

- The spreadsheet, as CSV, TSV, Excel (`.xlsx`) or OpenDocument (`.ods`).
- A column in it called `Filename` that holds each row's full filename, such as
  `MS-0042_001.tif`.
- The folder of files. It can have subfolders.

If your sheet only has the identifier, build the filename column in the spreadsheet first. In
Excel or LibreOffice, `=A2&"_001.tif"` next to an identifier in `A2` is usually enough. Exact
names are worth the minute: they are unambiguous, and anyone reading the export later can see
which file a row means.

## Create the project

1. Open the launcher with **File ▸ Open Projects…** (**Ctrl+O**). You can also drop the
   spreadsheet onto it.
2. Choose **Import a spreadsheet and its folder**.
3. Pick the spreadsheet, then the files folder.
4. Choose **Match by exact filename**. Tick **Also search subfolders** if the scans are spread
   over several folders.
5. Read the counts the wizard shows before you go on. It says how many rows matched a file, and
   how many files in the folder the spreadsheet never mentions. Each of those gets a row of its
   own, empty apart from its filename, so you can see what was scanned but never catalogued.
6. If one file is named by several rows, the wizard asks what to do with it.

The `Filename` column gets the **Filename** column type. That type is what tells qrate to look
the value up on disk and to report a file that is not there.

## Check what linked

Open the Problems panel with **Ctrl+Shift+M**. Every row whose file cannot be found has a
file-link finding, so you get the list of gaps without opening anything. Click a finding to
jump to its row.

If almost nothing linked, qrate shows a single banner above the grid instead of hundreds of
findings. That usually means the folder moved or you picked the wrong one. Click **Relink…** and
choose the right folder. qrate counts how many files it would find there before it changes
anything.

For one row at a time, select it and use **Locate file…** in the Details panel. If the same
folder holds the files of other missing rows, qrate offers to relink those too, as one step you
can undo.

## Keep working while files arrive

Digitization rarely finishes before cataloguing starts. qrate watches the files folder while the
project is open:

- When a file that a row names appears, the row links to it and its finding goes away.
- When a linked file is renamed or deleted, the finding comes back.
- Files that no row names are counted as **New files (N)** in the status bar. Click it to add
  them as rows, or choose **Ignore** to stop counting them.

qrate waits for a file to stop changing before it reads it, so a large TIFF that is still
copying is not picked up half-written.

## Files from somewhere else

If you drop a file from outside the files folder onto a row, qrate asks whether to copy it into
the project's `imported` folder or link it where it is. Copying keeps the collection in one
place. Linking where it is works, but the Problems panel warns about it, because the file will
not move with the rest of the collection.

To stop being asked, set **Settings ▸ Project ▸ Import ▸ Files from outside the files folder**.

## Next

When the links are clean, export a ZIP archive to hand the data and its files over together.
See [Export your catalogue to CSV and Excel](/guides/export-to-csv-and-excel).

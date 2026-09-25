# Projects

A qrate project is one `.qrate` file. It is a portable SQLite database. It holds the
collection grid, column settings, notes, and other project metadata. Copy or move this
one file to move the project.

Linked files, such as photos and documents, stay where they are. qrate never copies them
into the project. If you move a linked-files folder, open **Settings ▸ Project** and point
the files-folder path at its new location.

## Create a project

The launcher offers three ways to start:

1. **Blank project.** Start with an empty grid, or choose files and folders to start from.
   Each folder becomes a parent row and each file a row inside it, so the grid keeps the
   folder tree as an archival hierarchy. See [Groups](grid.md#groups).
2. **Import a spreadsheet and its folder.** qrate reads a CSV, Excel workbook, or
   OpenDocument spreadsheet as the grid. If you also give it a files folder, qrate links rows
   to files in that folder by filename, or by a custom pattern such as `{id}_*.jpg`. If one
   file is named by several rows, the wizard asks what to do with it.
3. **Start from a Google Sheet link.** qrate reads the sheet once to build the project. See
   [Export and Google Sheets](export-and-sync.md) for how to keep the two in sync afterward.

When a project starts from folders, pick an **Archival description standard**: RAD, DACS,
ISAD(G), Records in Contexts, or Custom. It supplies the level names, and you choose what
folders and files are called, for example Series and Item.

The wizard can also load a column configuration file, such as the `column_config.csv` that
**Settings ▸ Columns ▸ Config file ▸ Export…** writes, so a new collection starts with the same
column types and descriptions as an earlier one. To apply one to a project that is already
open, choose **Data ▸ Load Column Config…**.

You can drop material onto the launcher instead of browsing: a `.qrate` file opens it, a
spreadsheet starts a spreadsheet import, and files or folders start a folder-based project.

## Open a project

The launcher lists recent projects. Pick one to open it, or browse to any `.qrate` file.

## Add files to an open project

Choose **File ▸ Import Files or Folders…**, or drop files and folders onto the grid or the
gallery. qrate shows how many rows it will add. If some of the files are already in the
project, choose **Skip duplicates**, **Update existing** (re-link the existing row to the
file), or **Add all as new**. Drop a single file onto a `Filename` cell to link that row to it.

## Add spreadsheet rows to an open project

Choose **File ▸ Import Spreadsheet…** and select a CSV, TSV, Excel, or OpenDocument file.
qrate matches the first row's headers to existing column names, ignoring case and surrounding
spaces. It shows any unmatched source columns before you confirm. Confirming appends the data
rows to the current project as one change you can undo; existing rows and columns stay in place.

## Project settings

**Settings ▸ Project** holds the linked-files folder. **Settings ▸ Columns** holds this
project's column configuration (see [Columns](columns.md)), and each plugin's settings have a
page of their own. **Settings ▸ Application** holds settings that apply to every project you
open. Set **Application ▸ Identity ▸ Author name** to sign new notes and
history entries; the active name appears in the project window.

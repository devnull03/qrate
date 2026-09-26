# Projects

A qrate project is one `.qrate` file. It is a portable SQLite database. It holds the
collection grid, column settings, notes, and other project metadata. Copy or move this
one file to move the project.

Linked files, such as photos and documents, stay where they are. qrate never copies them
into the project. If you move a linked-files folder, open **Settings ▸ Project** and point
the files-folder path at its new location.

## Create a project

Choose **File ▸ New Project…** or press **Ctrl+N** to start a blank project in the wizard. To
pick how you start, open the launcher with **File ▸ Open Projects…** or **Ctrl+O**. The
launcher offers three ways to start:

1. **Blank project.** Start with an empty grid, or choose files and folders to start from.
   Each folder becomes a parent row and each file a row inside it, so the grid keeps the
   folder tree as an archival hierarchy. See [Groups](grid.md#groups).
2. **Import a spreadsheet and its folder.** qrate reads a CSV, Excel workbook, or
   OpenDocument spreadsheet as the grid. If you also give it a files folder, qrate links rows
   to files in that folder by filename, or by a custom pattern such as `{id}_*.jpg`. If one
   file is named by several rows, the wizard asks what to do with it.
3. **Start from a Google Sheet link.** qrate reads the sheet once to build the project. Date
   cells come in as dates, written `YYYY-MM-DD`, the same as from an Excel workbook. See
   [Export and Google Sheets](export-and-sync.md) for how to keep the two in sync afterward.

When a project starts from folders, pick an **Archival description standard**: RAD, DACS,
ISAD(G), Records in Contexts, or Custom. It supplies the level names, and you choose what
folders and files are called, for example Series and Item. You can change all three later in
**Settings ▸ Project ▸ Description** (see [Project settings](#project-settings)).

The wizard suggests saving the new project in the folder you last created a project in, or in a
`qrate` folder in your Documents folder the first time. To choose a fixed folder instead, set
**Settings ▸ Application ▸ New projects ▸ Save new projects in**.

The wizard can also load a column configuration file, such as the `column_config.csv` that
**Settings ▸ Columns ▸ Config file ▸ Export…** writes, so a new collection starts with the same
column types and descriptions as an earlier one. To apply one to a project that is already
open, choose **Data ▸ Load Column Config…**.

You can drop material onto the launcher instead of browsing: a `.qrate` file opens it, a
spreadsheet starts a spreadsheet import, and files or folders start a folder-based project.

## Open a project

Choose **File ▸ Open Projects…** or press **Ctrl+O** to show the launcher. It lists recent
projects. Pick one to open it, or browse to any `.qrate` file. If a project cannot be opened, the
launcher says why above the list.

If the open project has unsaved changes when you open or create another one, qrate asks first.
See [Saving](#saving).

## Saving

qrate saves cell edits to the project file on its own. By default it writes them after a short
pause in your typing, in the background, so a large project does not stop you working. Column
settings, layout, and other project settings always save as you change them.

**Settings ▸ Table ▸ Saving** controls autosave:

- **Autosave** turns it on or off. With it off, edits reach the file only when you save.
- **Method** chooses **After a short pause** or **On every edit**.

Press **Ctrl+S**, or choose **File ▸ Save**, to save at any time, whatever the autosave setting.
A dot in the title bar means the project has changes that are not saved yet.

If a save fails, for example because the drive is full or the file is read-only, qrate tells you
and keeps your changes open. Autosave tries again after your next edit, and **Ctrl+S** tries
at once and shows the reason if it fails again.

Before qrate quits, closes its window, switches to another project, or creates a new one, it
asks about unsaved changes: **Save**, **Don't Save**, or **Cancel**. If saving fails there, qrate
asks again with the reason, so your edits are never dropped without your choice. Press
**Ctrl+Q** or choose **File ▸ Quit** to quit. **Restart to update** asks the same question
before it restarts qrate.

Autosave is one of the settings a project can override; see
[Your defaults and project overrides](#your-defaults-and-project-overrides).

## Add files to an open project

Choose **File ▸ Import Files or Folders…**, or drop files and folders onto the grid or the
gallery. qrate shows how many rows it will add. If some of the files are already in the
project, choose **Skip duplicates**, **Update existing** (re-link the existing row to the
file), or **Add all as new**. Drop a single file onto a `Filename` cell to link that row to it.

**Settings ▸ Project ▸ Import ▸ Files already in the project** holds this project's choice for
such files: **Leave the existing row alone** (the default), **Re-link the existing row to the new
file**, or **Add it as a new row**. The wizard sets it when the project is created. An import
that finds files already in the project still asks, with this choice as the default button, and
the button you pick becomes the project's new choice.

## Add spreadsheet rows to an open project

Choose **File ▸ Import Spreadsheet…** and select a CSV, TSV, Excel, or OpenDocument file.
qrate matches the first row's headers to existing column names, ignoring case and surrounding
spaces. It shows any unmatched source columns before you confirm. Confirming appends the data
rows to the current project as one change you can undo; existing rows and columns stay in place.

## Project settings

**Settings ▸ Project** holds the linked-files folder, the import choice above, and the
**Description** group:

- **Description standard**: RAD, DACS, ISAD(G), Records in Contexts, or Custom.
- **Imported folders are** and **Imported files are**: the level names that imported folders and
  files, and new rows, get.

Changing the standard never rewrites rows. It sets what new rows and imports default to from
then on; rows already filed keep their level, and the old level names stay available.

**Settings ▸ Columns** holds this project's column configuration (see [Columns](columns.md)),
and each plugin's settings have a page of their own. **Settings ▸ Application** holds settings
that apply to every project you open. Set **Application ▸ Identity ▸ Author name** to sign new
notes and history entries; the active name appears in the project window. **Application ▸
Appearance ▸ Interface size** scales the text and controls of every qrate window: 90%, 100% (the
default), 110%, 125%, or 150%.

**Application ▸ Updates and downloads** holds **Automatic updates**, which checks for and
downloads signed qrate updates in the background, and **Download source**. Leave the download
source blank to get updates from GitHub and the plugin catalog from the qrate website. If your
network cannot reach them, your IT staff can keep a copy on a server or a shared folder: enter its
address, such as `https://mirror.example.org/qrate`, or a folder, such as
`file:///D:/qrate-mirror`. qrate checks everything it downloads against its own signatures, so a
mirror cannot change what you install. Anything the mirror does not have comes from the usual
place. The mirror's layout is in [Setup and releases](dev/SETUP.md#2a-where-updates-come-from-and-the-download-source).

### Your defaults and project overrides

Some settings are a default of yours that a project can override. They are autosave, row
stripes, row density, row height, undo steps, date format, spelling, and the CSV export
options. The **User** and **Project** tabs at the top of the Settings window choose which one
you are editing:

- **User** sets your default, which applies to every project that does not set its own.
- **Project** sets a value for the open project only, stored in its `.qrate` file, so it travels
  with the project. Most rows that hold a project value say **Set for this project** and offer
  a reset button that returns them to your default.

The **Project** tab is available while a project is open.

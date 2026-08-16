# Projects

A qrate project is one `.qrate` file. It is a portable SQLite database. It holds the
collection grid, column settings, notes, and other project metadata. Copy or move this
one file to move the project.

Linked files, such as photos and documents, stay where they are. qrate never copies them
into the project. If you move a linked-files folder, open **Settings ▸ Project** and point
the files-folder path at its new location.

## Create a project

The launcher offers three ways to start:

1. **Blank project.** Start with an empty grid and add columns yourself.
2. **Import a CSV file and its folder.** qrate reads the CSV as the grid and, if you also
   give it a files folder, links rows to files in that folder by filename.
3. **Start from a Google Sheet link.** qrate reads the sheet once to build the project. See
   [Export and Google Sheets](export-and-sync.md) for how to keep the two in sync afterward.

## Open a project

The launcher lists recent projects. Pick one to open it, or browse to any `.qrate` file.

## Project settings

**Settings ▸ Project** holds settings scoped to this one project: the linked-files folder,
column configuration, and plugin settings. **Settings ▸ App** holds settings that apply to
every project you open, such as the interface theme.

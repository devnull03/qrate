---
title: Turn a folder of scans into a RAD or DACS hierarchy
description: Start a qrate project from the folders you already have, so series, files and items arrive as nested rows with their levels filled in.
published: 2026-11-26
tags: [projects, arrangement]
related:
  - { label: 'Projects: create a project', href: '/docs/projects#create-a-project' }
  - { label: 'The grid: groups', href: '/docs/grid#groups' }
  - { label: 'Columns: column type', href: '/docs/columns#column-type' }
---

Most digitization projects already have an arrangement before anyone opens a spreadsheet: it is
the folder tree on the shared drive. A `Series 1` folder holds `File 3`, which holds the scans.
Retyping that structure into a flat spreadsheet throws it away, and then somebody has to rebuild
it by hand with "parent ID" columns.

qrate can read the tree directly. Each folder becomes a parent row and each file becomes a row
inside it, so the grid starts with the arrangement you already made.

## Before you start

Tidy the folders first. qrate keeps the tree exactly as it finds it, so the folders should match
the arrangement you want to describe:

- One folder per series, sub-series or file, nested the way they nest intellectually.
- Scans or born-digital files inside the lowest-level folder they belong to.
- Folder names you are happy to see in the grid. You can rename rows later, but it is quicker to
  fix `New folder (2)` now.

qrate never moves or copies these files. The project links to them where they are, so leave the
tree where it will live, not on your desktop.

## Create the project

1. Open the launcher with **File ▸ Open Projects…** (**Ctrl+O**).
2. Choose **Blank project**, then choose the top folder of the collection. You can also drag the
   folder straight onto the launcher.
3. Under **Archival description standard**, pick the one your institution follows: **RAD**,
   **DACS**, **ISAD(G)**, **Records in Contexts**, or **Custom**.
4. Choose what folders and files are called. For a typical collection, folders are **Series**
   and files are **Item**. If your lowest folders are files in the archival sense, say so here.
5. Save the project. The wizard suggests the folder you last used, or a `qrate` folder in your
   Documents the first time.

The result is one `.qrate` file. It is a single SQLite database, so you can copy it, back it up
or email it like any other file. The scans stay where they were.

## What you get

Every folder is now a row with a chevron in its row header, and the files inside it are its
children. Click a chevron to collapse or expand a group, or press **Ctrl+Alt+Right** and
**Ctrl+Alt+Left** to expand or collapse everything.

qrate stores each row's level, its parent and the path it came from with the row itself. To see
them as columns, and to have them written out when you export, add a column and set its type in
**Settings ▸ Columns ▸ Data types**:

| Column type | What it holds |
| --- | --- |
| **Description Level** | The level from the standard you picked, such as Series or Item. |
| **Parent Component** | The row this one sits inside. |
| **Source Path** | The path the row was imported from. |

The **Filename** column links every item row to its file, so the Details panel shows the scan
next to the row you are describing.

## Fix the arrangement in the grid

Real trees are never quite right. You do not have to go back to the file manager:

- **Group rows.** Select some rows, right-click a row header, and choose **Group**. qrate adds a
  new parent row above them, for example a sub-series nobody made a folder for.
- **Indent and outdent.** **Ctrl+]** makes a row a child of the row above it, and **Ctrl+[**
  moves it back out.
- **Drag.** Drop a row header on the middle of another row to make it a child, or on its top or
  bottom edge to place it before or after.

A row always moves with its children, and each change is one step you can undo. None of this
moves a file on disk.

## Change the standard later

If you started with Custom and your repository later settles on DACS, change it in **Settings ▸
Project ▸ Description**. Changing the standard never rewrites rows: it sets the level names that
new rows and later imports get. Rows you have already described keep their level.

## Next

Describe the items, then check your work in the Problems panel before you export. The JSON-LD
and ZIP exports keep the arrangement, because each row records the row it is part of. See
[Export your catalogue to CSV and Excel](/guides/export-to-csv-and-excel).

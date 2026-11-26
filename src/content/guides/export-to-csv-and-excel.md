---
title: Export your catalogue to CSV and Excel
description: Pick the right export for who is receiving it, keep identifiers and dates intact, and hand over files and data together.
published: 2026-11-26
tags: [export]
related:
  - { label: 'Export and Google Sheets', href: '/docs/export-and-sync' }
  - { label: 'Columns: column type', href: '/docs/columns#column-type' }
  - { label: 'Online converter', href: '/convert' }
---

A qrate project is one `.qrate` file, and sooner or later the data has to go somewhere that does
not run qrate: a repository ingest, a finding aid, a colleague with Excel. Everything here starts
from **File ▸ Export**.

## Pick a format

| Who it is for | Format |
| --- | --- |
| A colleague who works in Excel | **Excel (.xlsx)** |
| An ingest script, Islandora Workbench, ArchivesSpace imports | **CSV** |
| Someone who needs the scans as well as the data | **ZIP Archive** |
| A linked-data system, or anything that must keep the hierarchy | **JSON-LD** |
| Zotero, or a citation manager | **Zotero (CSL-JSON)** |

Plugins can add formats of their own to the same menu.

## Excel: what survives

The Excel export writes every cell as text. That sounds minor, but it is the difference between
an accession number of `00123` and `123`, or a date of `1954-03` and a serial number. Excel
cannot "helpfully" reformat what it receives as text.

Notes you left on cells in qrate become cell comments in the workbook, so the person receiving it
sees your queries in place.

## CSV: match what the other end expects

**Settings ▸ Table ▸ CSV export** has two settings, and both can differ per project:

- **Byte order mark.** On by default. Excel on Windows needs it to show `é` and `ñ` correctly.
  Turn it off for older scripts that read the marker as part of the first column name.
- **Separator.** **Comma** by default. Excel in regions that write decimals with a comma, such as
  much of Europe, expects **Semicolon**. Choose **Tab** for systems that ask for TSV.

## Keep the arrangement in a flat file

A spreadsheet has no nesting, so a hierarchy built in qrate needs columns to survive the trip. Add
a column for each piece you want written out and set its type in **Settings ▸ Columns ▸ Data
types**:

- **Description Level** writes the level, such as Series or Item.
- **Parent Component** writes the row each row sits inside.
- **Source Path** writes the folder path a row was imported from.

The export fills these in for you. JSON-LD and ZIP exports keep the arrangement without them,
because each row records the row it is part of.

## Hand over data and files together

**ZIP Archive** bundles the data as CSV and JSON-LD with every linked file. Files imported from
folders keep their folder paths, and files linked from outside the files folder go into
`files/outside/`. A large ZIP shows its progress and can be cancelled. A cancelled or failed
export never leaves a half-written file over an existing one.

## Things worth knowing

- **Filters do not apply.** Export always writes every row, even if the grid is filtered.
- **The dialog remembers.** It opens in the folder this project last exported to, and suggests a
  filename from the project name.
- **No qrate on the other machine?** The [online converter](/convert) turns a `.qrate` file into
  CSV, Excel and the other formats in the browser. The file never leaves that computer.
- **Google Sheets.** Turn it on in **Settings ▸ Google** to export to a new sheet, or to sync the
  project into one. Sync goes one way, from qrate to the sheet.

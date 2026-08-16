---
title: 'Columns'
description: 'column types, authority lists, and per-column settings'
sidebar:
  order: 4
---

Open **Settings ▸ Project ▸ Columns** to configure a column. Settings are stored per
project, keyed by the column's header name, so renaming other columns does not disturb
them.

## Column type

A column's type controls how qrate treats its values:

- **Text** — a plain field, checked only by spelling if you enable that.
- **Filename** — links each row to a file. See [Files and photos](/docs/files-and-photos) and
  [Diagnostics](/docs/diagnostics#what-qrate-checks).
- **Date** — checked for a valid, unambiguous date format.
- **Authority-checked** — checked against LCSH, GeoNames, or Wikidata, depending on what
  the column holds.

## Description

Add a short description to a column to document what it is for. It shows as a tooltip on
the column header, for anyone else who opens the project.

## Spell check

Turn spell check on or off per column, and choose its dictionary language from the more
than 60 available.

## Authority lists

For a column checked against an authority, such as subject headings, qrate flags a value
that the authority does not recognize and can suggest the closest match as a fix.


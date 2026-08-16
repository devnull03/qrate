---
title: 'Diagnostics'
description: 'the Problems panel, spelling, and fixes'
sidebar:
  order: 3
---

qrate checks project data continuously and lists what it finds in the Problems panel, in
the right dock. A finding on a cell also shows as a small marker on that cell in the grid.

## What qrate checks

- **Spelling**, in over 60 languages.
- **Date formats**, so a malformed or ambiguous date is caught before export.
- **File links**, so a row whose linked file cannot be found is reported instead of
  silently showing a blank preview. See [Files and photos](/docs/files-and-photos).
  Only a column set to the `Filename` type is checked this way.
- **Headings against LCSH, GeoNames, and Wikidata**, so a subject or place heading can be
  checked against those authorities.
- **Plugin validators**, if the project has plugins that add their own checks. See
  [Plugins](/docs/plugins).

Each of these runs as an independent validator. A validator reports its complete set of
findings each time it runs, and that set replaces what it reported last time.

## The Problems panel

The panel lists every current finding, grouped and sorted for review. Click a finding to
jump to the cell it is about.

## Applying a fix

Some findings offer a suggested correction. Right-click the cell, or open its **Fixes**
menu, and pick a suggestion to apply it. Applying a fix replaces the cell's whole value, so
you always know exactly what you are accepting.

A fix offered against one version of a cell's text does not apply once that text has
changed. This stops a stale suggestion from silently overwriting a newer edit.


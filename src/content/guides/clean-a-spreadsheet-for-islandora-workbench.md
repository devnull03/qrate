---
title: Clean a spreadsheet before an Islandora Workbench ingest
description: Catch bad dates, stray spellings, inconsistent names and terms your Islandora site does not know, before Workbench rejects the batch halfway through.
published: 2026-11-26
tags: [islandora, diagnostics, export]
related:
  - { label: 'Islandora plugin', href: '/docs/plugins/islandora' }
  - { label: 'Diagnostics', href: '/docs/diagnostics' }
  - { label: 'Columns', href: '/docs/columns' }
  - { label: 'Export and Google Sheets', href: '/docs/export-and-sync' }
---

[Islandora Workbench](https://mjordan.github.io/islandora_workbench_docs/) loads content into an
Islandora site from a CSV file. It is fast and scriptable, and it is unforgiving: a date in the
wrong shape, a subject spelled two ways, or a taxonomy term the site has never heard of turns into
an error, a duplicate term, or a node nobody can find.

Those are metadata problems, and they are cheaper to fix before the ingest than after it. This
guide uses qrate to check the CSV first, then hands Workbench a clean copy.

## Bring the spreadsheet in

Open the launcher with **File ▸ Open Projects…** (**Ctrl+O**), choose **Import a spreadsheet and
its folder**, and pick your Workbench CSV. If its `file` column names files in a local folder,
give the wizard that folder too, then set the `file` column's type to **Filename** (below). qrate
then previews each file and flags the ones that are missing, which Workbench would otherwise stop
on. See [Link an existing spreadsheet to its scans](/guides/link-scans-by-filename).

qrate keeps your column names exactly as they are, so `field_subject` and `field_edtf_date_created`
come back out unchanged.

## Tell qrate what each column is

Open **Settings ▸ Columns**. Three settings do most of the work here:

- **Data types.** Set your date columns to **Date**, your title column to **Title**, `file` to
  **Filename**, and identifiers such as `id` or `field_local_identifier` to **Identifier**. Only Text and Title
  columns are spell-checked, so identifiers stop producing spelling noise.
- **Multi-value cells.** Workbench separates repeated values with `|` by default. Set
  **Value separator** to `|` so qrate checks `Photographs|Postcards` as two values, not one odd
  one.
- **Spelling.** Turn spell check off for columns full of names or codes, and tick the languages
  your descriptions are written in under **Settings ▸ Spelling**.

## Check the dates

Islandora's date fields expect [EDTF](https://www.loc.gov/standards/datetime/). Set **Settings ▸
Table ▸ Checks ▸ Date format** to **EDTF** (the default). Every Date column is then checked, and
values like `ca. 1920` or `12/03/1954` are flagged in the Problems panel. EDTF writes those as
`1920~` and `1954-03-12` (or `1954-12-03`: the panel is there because qrate cannot know which).

## Check terms against your Islandora site

The [Islandora plugin](/docs/plugins/islandora) checks a column's values against your site's own
taxonomies, and suggests terms as you type.

1. Install it from the [plugin page](/plugins/org.islandora.vocabularies).
2. In **Settings ▸ Plugins**, turn on **Enabled** and **Network access** for `islandora`.
3. In **Settings ▸ islandora**, set **Islandora server** to your site, for example
   `https://islandora.example.org`, and click **Refresh from server**.
4. Under **Mapping**, map `field_subject` to your subject vocabulary, `field_genre` to genre, and
   so on.

Every value the site does not hold becomes an error that names the vocabulary it checked. That is
the list of terms Workbench would otherwise either reject or quietly create as new ones,
depending on your configuration.

## Make names and subjects consistent

Right-click a column header such as `field_linked_agent` or `field_subject` and choose **Review
value variants**. qrate groups values that look like the same thing written differently:
`Smith, John`, `Smith, John.` and `John Smith`.

Right-click a group in the Problems panel to replace every occurrence with one form, as a single
undo step. If two values really are different people, mark them as distinct and qrate stops
asking. qrate never merges anything on its own.

## Work through the Problems panel

Open it with **Ctrl+Shift+M**. Findings are grouped, so one misspelling repeated across 300 rows
is one line. For each group:

- **Fix it.** Right-click a cell finding and pick a suggestion. Spelling and capitalization
  groups can be fixed in one go.
- **Ignore it.** **Ignore this occurrence** or **Ignore in column** for findings that are correct
  as they are. Ignores are saved with the project.

Aim for an empty Errors tab before you export.

## Export for Workbench

Choose **File ▸ Export ▸ CSV**. Before you do, look at **Settings ▸ Table ▸ CSV export**:

- **Separator** should be **Comma**, which is what Workbench reads.
- **Byte order mark** is on by default so Excel shows accented letters correctly. If your
  Workbench setup reports the first column header as unknown, turn it off and export again.

Export always writes every row, whatever filter is active in the grid. Run Workbench's own check
against the new CSV before the real ingest:

```sh
./workbench --config my_ingest.yml --check
```

The `.qrate` project stays behind as the record of what you changed. Its History lists every
fix, so you can answer "why does this subject read differently from the donor's list" months
later.

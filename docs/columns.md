# Columns

Open **Settings ▸ Columns**, or choose **Data ▸ Column Settings…**, to configure a column.
Settings are stored per project, keyed by the column's header name, so renaming other
columns does not disturb them.

## Column type

A column's type, set under **Data types**, controls how qrate treats its values. A column
with no type set is **Text**.

- **Text**: a plain field. Only Text and Title columns are spell-checked.
- **Title**: the primary human-readable name of a row.
- **Filename**: links each row to a file. See [Files and photos](files-and-photos.md) and
  [Diagnostics](diagnostics.md#what-qrate-checks).
- **Date**: checked against the date format chosen in **Settings ▸ Table ▸ Checks ▸ Date
  format**, so a malformed or ambiguous date is flagged:
  - **EDTF** (the default) also allows uncertain and approximate dates such as `1987?` and
    `1987~`, and intervals such as `1987/1989`.
  - **ISO 8601 only** allows `YYYY`, `YYYY-MM`, and `YYYY-MM-DD`, and an interval of two of them.
  - **Lenient** allows everything EDTF does, plus the ways catalogues often write an
    approximate date: `circa 1920`, `ca. 1920`, `c. 1920`, a decade such as `1920s`, and a
    bracketed guess such as `[1920?]` or `[ca. 1920]`.

  Changing the format checks every Date column again. A project can override your default; see
  [Projects](projects.md#your-defaults-and-project-overrides).
- **Number**, **Url**, and **Identifier**: an accession number, call number, or other
  identifier belongs in an Identifier column.
- **Description Level**, **Parent Component**, and **Source Path**: show a row's archival
  level, its parent, and the path it was imported from. See [Groups](grid.md#groups).

To reuse a column setup, choose **Config file ▸ Export…** at the bottom of the page. The
resulting CSV can be loaded in the New Project wizard or with **Data ▸ Load Column Config…**.

## Description

Add a short description to a column under **Descriptions** to document what it is for. It
shows as a tooltip on the column header, for anyone else who opens the project.

## Multi-value cells

Set **Multi-value cells ▸ Value separator**, for example `|`, when one cell holds several
values, as in `Film|Video`. Filters and checks then treat each part as its own value.

## Spell check

Turn spell check on or off per column under **Spelling ▸ Spell-checked columns**. The
languages ticked on the separate **Settings ▸ Spelling** page take part in automatic language
selection; the first one ticked chooses the preferred regional spelling when variants exist.

## Value variants

Turn on value-variant review for columns where inconsistent displayed forms matter, such as
people, organizations, places, subjects, collection titles, and controlled labels. qrate
suggests similar values already present in that column but never merges them automatically.

## Authority lists

Under **Checked against**, pick the authority a column is checked against: LCSH, Wikidata,
or GeoNames. qrate flags a value that the authority does not recognize and can suggest the
closest match as a fix. Values are checked over the network, so a finding appears once the
answer arrives rather than as you type.

GeoNames needs an activated GeoNames web-services account. If GeoNames rejects the account,
qrate stops the remaining requests and shows one warning. Change the account under
**Settings ▸ Columns ▸ Authority accounts** to retry.

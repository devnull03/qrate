# Islandora plugin

Islandora is an open-source repository platform that libraries and archives use to publish digital
collections. An Islandora site keeps its controlled vocabularies — subjects, genres, names — as
taxonomies.

The Islandora plugin holds a column in qrate to those vocabularies. It checks each value against
the site, and it offers terms from the site while you type. qrate ships this plugin as the first
example of what a plugin can do.

## What the plugin does

**It checks column values against a vocabulary.** Map a column to one or more vocabularies. The
plugin then reports every value the site does not hold as an error in the Problems panel. The
message names the vocabularies it consulted, for example `"Still image" is not a term in the
Islandora typeofresource vocabulary`.

**It offers terms while you type.** Type two or more characters in a mapped cell. A completion list
appears under the cell editor with matching terms from the site. One character is too little to
narrow a vocabulary, so the plugin stays quiet until the second character.

**It accepts several vocabularies for one column.** A value passes if any one of the mapped
vocabularies holds it. This suits a column that draws from more than one list.

**It splits a cell the same way qrate does.** The plugin uses the sub-delimiter from
**Settings ▸ Project ▸ Columns**, so a multi-value cell means the same thing to the check as to
the column filter. A blank part of a cell is not an error. Missing data is a different check.

**It downloads nothing.** The plugin asks the server which of a batch of values the server
recognizes. One request covers up to 50 values. A vocabulary of a hundred thousand terms therefore
costs the same as a vocabulary of ten.

**It remembers answers.** qrate stores each verdict beside its own data, not in the project file.
Only a value nobody has checked before costs a request. A re-check after one typo is free.

**It never turns a column red because the server failed.** If the site does not answer, the plugin
reports nothing for that run and writes the reason to the log.

A first pass over a wide sheet checks up to 200 new values per run. The rest wait for a later run,
which the next edit starts.

## Install the plugin

1. Open **Extensions ▸ Plugins Folder**.
2. Put the plugin folder there. To clone it, run
   `git clone https://github.com/devnull03/qrate-islandora-plugin islandora`.
3. Name the folder `islandora`. qrate keys the plugin's stored settings by the folder name.
4. Restart qrate, or reload the plugins.

## Turn it on

1. Open **Settings ▸ Plugins**. Turn on **Enabled** for `islandora`.
2. Turn on **Network access** for `islandora`. The plugin reaches no server until you do.
3. Open **Settings ▸ islandora**. Set **Islandora server** to the base URL of your site, for example
   `https://islandora.example.org`.
4. Click **Refresh from server**. The plugin reads the list of vocabularies the site publishes.
5. Map each column under **Mapping**. Pick the vocabularies the column must be checked against.

You can also map a column from the grid. Right-click a column header and open the **Vocabularies**
submenu. A tick marks each vocabulary the column already uses. The submenu and the settings page
write the same value.

## Settings

| Setting | Where it is stored | What it is for |
| --- | --- | --- |
| Islandora server | The project | The base URL. It travels with the project file. |
| Username | This computer | An account, only if the site refuses anonymous readers. |
| Password | This computer | The password for that account. qrate keeps it out of the project file. |
| Vocabulary names | The project | Comma-separated machine names, for example `subject, genre`. Use this only if the site will not list its vocabularies. |

The plugin has no sub-delimiter setting of its own. It uses the one in
**Settings ▸ Project ▸ Columns**.

## The status bar item

The plugin adds an item on the right side of the status bar. It reads `Islandora ?` until you use
it.

Click the item to test the connection. The item then shows one of these:

- `Islandora ✓` in green — the site answered.
- `Islandora ✗` in red — the site did not answer. The log holds the reason.
- `Islandora no server set` in red — the **Islandora server** setting is empty.

After a refresh, the item shows how many vocabularies the plugin found.

## Column header menu

Right-click a column header for two more commands:

- **Refresh Islandora vocabularies** — read the list of vocabularies from the site again.
- **Forget cached Islandora terms** — discard the stored verdicts for the mapped vocabularies. Use
  this after somebody adds a term on the site, so qrate asks again instead of trusting an old
  answer.

## What the site must allow

The plugin uses Drupal's JSON:API, which a stock Islandora site enables. If `/jsonapi` answers 404,
switch the module on with `drush en jsonapi`.

Drupal guards one call. An anonymous reader needs the **`access taxonomy overview`** permission to
list vocabularies. Without it, Drupal answers with an empty list instead of refusing, so the
mapping tool finds nothing. You have three ways forward:

- Grant that permission on the site.
- Fill in **Username** and **Password**.
- Type the machine names into **Vocabulary names**. The mapping tool works from that list instead.

## Related pages

- [Diagnostics](../diagnostics.md) — the Problems panel, where the plugin reports what it finds.
- [Columns](../columns.md) — column types and the sub-delimiter.
- [Plugins](index.md) — what a plugin can do, and how to write one.

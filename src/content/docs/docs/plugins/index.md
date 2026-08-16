---
title: 'Plugins'
description: 'what a plugin can do, and how to write one'
sidebar:
  order: 7
---

A plugin adds checks and commands to qrate. You write it in Lua. qrate runs it inside the app,
in a sandboxed [Luau](https://luau.org) virtual machine.

A plugin is one folder of `.lua` files. There is no package manager, no registry, and no build
step. You put the folder in qrate's plugins directory, and qrate finds it at startup.

For the full list of hooks and host functions, see the [API reference](/docs/plugins/api-reference).

## What a plugin can do

- **Check a column.** A plugin reports what is wrong with a column's values. Its findings go to
  the Problems panel and to the cell markers in the grid, beside the checks qrate ships with. See
  [Diagnostics](/docs/diagnostics).
- **Add right-click entries.** A plugin adds entries to the menu on a cell, a row, or a column
  header. A click runs the plugin.
- **Add a bar item.** A plugin puts text in the status bar or the title bar. The text takes
  markup for weight and color, and each mouse button can run a command or open a menu.
- **Suggest values.** A plugin offers completions for the cell the user is editing.
- **Map columns onto a list.** A plugin that holds a list of terms can offer one picker per
  column, on its Settings page and on the column header menu.
- **Declare settings.** A plugin declares its own knobs. qrate renders them on a Settings page
  and stores the values.

A plugin can also read from the network, but only after the user grants it. Everything else in
Luau's own standard library that reaches outside the VM is absent: there is no `io`, no `os`, and
no `package`.

## How qrate loads a plugin

qrate reads the plugins directory at startup. Open it with **Extensions ▸ Plugins Folder**.

qrate accepts two shapes:

- `my-plugin.lua` — a single file.
- `my-plugin/init.lua` — a folder. `init.lua` can `require` any other `.lua` file beside it.

The name on disk is the plugin's identity. qrate keys the plugin's stored settings, its enable
switch, and its permission grants by that name. Renaming the folder later orphans everything the
plugin stored. Pick the name first.

`init.lua` returns one table. That table is the manifest. There is no second file to write.

**Settings ▸ Plugins** lists every plugin qrate found, running or not. Each row carries an enable
switch, the plugin's one-line description, and a network switch when the plugin asks for one. A
plugin that failed to load shows its error there.

## Getting started

### 1. Clone the template

```sh
git clone https://github.com/devnull03/qrate-plugin-template my-plugin
cd my-plugin
rm -rf .git && git init
```

The template holds a small working plugin and the complete type definitions.

| File | What it is |
|---|---|
| `init.lua` | The plugin. The table it returns is the manifest. |
| `types/qrate.lua` | Type definitions for the whole API, with the reasoning attached. |
| `.luaurc` | Points luau-lsp at `types/`. |

`types/qrate.lua` never runs. qrate installs the `qrate` global itself, and no `require` path
reaches `types/`. The file exists so your editor can complete and typecheck your plugin.

### 2. Set up your editor

Install [luau-lsp](https://github.com/JohnnyMorganz/luau-lsp) and open the folder. Completion then
tells you what exists, and typechecking tells you when a table has the wrong shape.

Read `types/qrate.lua` before you write anything. A capability that file does not declare is a
capability no plugin has.

### 3. Write a minimal plugin

This plugin flags every empty cell in the columns you switch it on for:

```lua
return {
  api_version = 1,
  description = "Flags empty cells.",

  menu = {
    { label = "Check this column for gaps", target = "column", command = "watch" },
  },

  on_command = function(command, ctx)
    return { column = { watched = true } }
  end,

  validate = function(column, values, settings)
    if not settings.column.watched then
      return {}
    end
    local found = {}
    for row, value in ipairs(values) do
      if value == "" then
        found[#found + 1] = {
          row = row,
          severity = "warning",
          message = column.name .. " is empty",
        }
      end
    end
    return found
  end,
}
```

Three things in that example apply to every plugin:

- qrate calls `validate` for **every** column, not only yours. Return an empty table for a column
  you were not asked about.
- `row` is 1-based, and it matches the `values` array you were handed.
- What `on_command` returns is what qrate stores. `validate` reads it back as `settings.column`.

### 4. Load it

1. Copy or clone the folder into the plugins directory (**Extensions ▸ Plugins Folder**).
2. Restart qrate, or click **Extensions ▸ Reload Plugins**.
3. Open **Settings ▸ Plugins** and confirm your plugin is listed and enabled.

### 5. Iterate

Edit the file, then click **Extensions ▸ Reload Plugins**. qrate rebuilds every plugin's virtual
machine and replaces every contribution. You do not have to restart the app.

qrate reports a plugin problem in two places, and the difference matters:

- **The Problems panel** lists what is wrong with the archivist's data. Your findings go here.
- **The session log** records what is wrong with your code. A syntax error, a runtime error, and
  anything you `print` go here. Read it with **Help ▸ Copy Debug Info**.

A plugin that fails to load also shows its error on its row in **Settings ▸ Plugins**.

## Limits

qrate applies these to every plugin. They stop a broken plugin from taking the app with it.

| Limit | Value |
|---|---|
| Memory per plugin | 64 MB |
| One call into Lua | 2 seconds |
| HTTP requests | 120 per minute, per plugin |
| One HTTP request | 10 seconds |

qrate warns in the log when a call takes longer than 150 ms. `validate` runs on every edit, so
anything slow belongs in a command or behind the plugin's own cache.

## See also

- [API reference](/docs/plugins/api-reference) — every hook, host function, and declaration.
- [Islandora plugin](/docs/plugins/islandora) — a plugin that ships with qrate, and a larger worked example.


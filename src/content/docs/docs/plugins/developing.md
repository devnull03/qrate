---
title: 'Develop plugins'
description: 'create, test, and publish a plugin'
sidebar:
  order: 8
---

Use this guide to create, test, and publish a qrate plugin. For each hook and host function, see
the [API reference](/docs/plugins/api-reference).

## How qrate loads a plugin

qrate reads the plugins directory at startup. Open it with **Plugins ▸ Plugins Folder**.

qrate accepts two layouts:

- `my-plugin.lua` — a single file.
- `my-plugin/init.lua` — a folder. `init.lua` can `require` other `.lua` files in that folder.

The name on disk is the plugin identity. qrate stores its settings, enable switch, and permission
grants under that name. Do not rename a plugin folder after qrate stores data for it.

`init.lua` returns the runtime descriptor. Release packages also include `qrate-plugin.json`.
qrate reads that manifest without running the plugin.

## Create a plugin

1. Clone the [plugin template](https://github.com/devnull03/qrate-plugin-template).
2. Install [luau-lsp](https://github.com/JohnnyMorganz/luau-lsp).
3. Read `types/qrate.lua`. It defines every available API capability.
4. Write your plugin in `init.lua`.
5. Run `npm run check` to validate the package metadata and archive contents.

The template includes a working plugin, type definitions, and a release workflow. The `types`
folder does not run. It helps your editor complete and typecheck the plugin.

### Minimal plugin

This plugin adds a command that marks one column for empty-value checks:

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
    if not settings.column.watched then return {} end
    local found = {}
    for row, value in ipairs(values) do
      if value == "" then
        found[#found + 1] = { row = row, severity = "warning", message = column.name .. " is empty" }
      end
    end
    return found
  end,
}
```

qrate calls `validate` for every column. Return an empty table for a column that your plugin does
not check. The `row` value starts at 1 and matches the `values` array.

## Test a plugin

1. Copy or clone the folder into **Plugins ▸ Plugins Folder**.
2. Restart qrate, or select **Plugins ▸ Reload Plugins**.
3. Open **Settings ▸ Plugins** and confirm that qrate lists and enables the plugin.

After an edit, select **Plugins ▸ Reload Plugins**. qrate rebuilds the plugin runtime and replaces
its contributions. You do not need to restart qrate.

qrate reports data problems in the Problems panel. It records plugin code problems in the session
log. Read the log with **Help ▸ Copy Debug Info**. A plugin load error also appears in
**Settings ▸ Plugins**.

## Publish a plugin

The official registry lists reviewed release packages. It does not host plugin source code.

1. Run `npm run check` for the package you will release.
2. Tag that version in your plugin repository. The template workflow creates a release ZIP and
   checksum.
3. Check the ZIP and checksum before you submit them.
4. Follow the [plugin submission guide](https://qrate.dvnl.work/plugins/submit/) to open a pull
   request to the [plugin registry](https://github.com/devnull03/qrate-plugin-registry).

The registry pull request must identify one exact release. Do not change that release after the
registry lists it.

## Limits

| Limit | Value |
| --- | --- |
| Memory per plugin | 64 MB |
| One call into Lua | 2 seconds |
| HTTP requests | 120 per minute, per plugin |
| One HTTP request | 10 seconds |

qrate warns in the log when a call takes more than 150 ms. qrate calls `validate` after every edit.
Put slow work in a command or use a plugin cache.


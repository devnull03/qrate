---
title: 'Plugin API reference'
sidebar:
  order: 9
---

Everything a plugin can do goes through two surfaces. The first is the table `init.lua` returns,
which qrate calls the descriptor. The second is the `qrate` global, which qrate installs before it
sandboxes the virtual machine.

There is no third surface. Luau's `io`, `os`, and `package` are not loaded, so a capability that
is not on this page is one no plugin has.

This page describes `api_version` 1. For a getting-started walkthrough, see
[Plugins](/docs/plugins).

## The descriptor

```lua
return {
  api_version = 1,
  name = "my-plugin",
  description = "One line, shown on the Settings page.",
  permissions = { "net" },

  settings = { … },
  menu = { … },
  bar = { … },
  column_map = { … },

  validate = function(column, values, settings) end,
  on_command = function(command, ctx) end,
  suggest = function(ctx) end,
}
```

Every field is optional, with one rule: the table must carry at least one of `validate`,
`on_command`, or `suggest`. A descriptor with none of them does nothing, and qrate refuses it.

| Field | Value |
|---|---|
| `api_version` | The descriptor shape you wrote against. Missing reads as 1. A version above what the running qrate speaks is refused, and the error names both numbers. |
| `name` | Overrides the folder name as the plugin's identity. See [Identity](#identity). |
| `description` | One line, shown on the plugin's Settings page. |
| `permissions` | What the plugin asks to be allowed to do. `net` is the only one. |

qrate reads the descriptor once, when it loads the plugin. To change a declaration, edit the file
and click **Extensions ▸ Reload Plugins**.

### Identity

Two names exist, and they are keyed differently.

- **The name on disk** is the file stem or the folder name. qrate keys the enable switch, the
  permission grants, and the plugin's storage file by it.
- **The descriptor `name`**, when you declare one, is what the Problems panel shows, what the
  plugin's findings are filed under, and what its stored settings are keyed by.

Renaming either one orphans what qrate stored under the old name. Choose both before you publish
the plugin.

## Validation

```lua
validate = function(column, values, settings) -> { Finding }
```

qrate calls `validate` for one column at a time, after the grid settles. It calls it for **every**
column in the project, not only the columns your plugin cares about. Return an empty table for a
column you have nothing to say about.

`column` carries two fields:

| Field | Value |
|---|---|
| `name` | The header text. This is also how a finding addresses a column. |
| `data_type` | The column's declared type from the project, or empty when nobody set one. |

`values` holds every row's text for that column, in source order.

`settings` is [the settings table](#reading-settings).

A finding is a table:

```lua
{ row = 3, severity = "warning", message = "Country is not a place name" }
```

| Field | Value |
|---|---|
| `row` | 1-based, matching the `values` array you were handed. |
| `severity` | `"error"`, `"warning"`, or `"note"`. Missing reads as `"error"`. A spelling qrate does not know degrades to `"note"`. |
| `message` | What is wrong, written for the archivist. |

**Each run replaces the plugin's whole set of findings.** qrate does not merge with what the
plugin reported last time, and there is no call to clear one finding. A cell the plugin stops
reporting stops being marked. This is the same rule every built-in validator follows.

`validate` runs off the interface thread, so it may block on a server. It also runs on every edit,
so anything expensive belongs in a command or behind [storage](#storage) instead.

An error raised inside `validate` goes to the session log, and that run contributes nothing. It
does not reach the Problems panel: the panel is a list of what is wrong with the data, not with
the code.

## Commands

A command is a name your plugin declares and qrate hands back to you when the user clicks. Both
right-click entries and [bar items](#bar-items) run commands.

### Right-click entries

```lua
menu = {
  { label = "Restrict to these values", target = "column", command = "restrict" },
  { label = "Stop restricting", target = "column", command = "clear",
    requires_settings = true },
},
```

| Field | Required | Value |
|---|---|---|
| `label` | yes | What the entry reads as. |
| `target` | yes | `"column"`, `"cell"`, or `"row"` — which menu the entry joins. |
| `command` | yes | Handed back to `on_command` unchanged. |
| `requires_settings` | no | Show the entry only when this plugin has already stored something for what was clicked. |

`requires_settings` is the whole conditional vocabulary. Use it so a "Clear" entry does not offer
to undo nothing.

### Handling a click

```lua
on_command = function(command, ctx) -> Writes?
```

qrate runs `on_command` off the interface thread, so a slow command does not freeze the grid.

`ctx` carries what was clicked:

| Field | Value |
|---|---|
| `column` | Header text, or `nil` when nothing gives the command a column. |
| `row` | 1-based, when the user clicked a single cell. |
| `values` | Every row's text for `column`, in source order. |
| `argument` | The option a mapping menu entry carried, when the command came from one. |
| `settings` | [The settings table](#reading-settings). |

Check `ctx.column` before you read `ctx.values`. A command from a bar item may have no column
under it.

### Writing settings back

`on_command` returns a table of what qrate must store, or nothing at all:

```lua
return { column = { allowed = ctx.values }, project = { checked = true } }
```

| Field | Where it lands |
|---|---|
| `column` | This plugin's object for the clicked column, in the project file. |
| `project` | This plugin's project-scope object, in the `.qrate` file. |
| `user` | This plugin's user-scope object, on this machine. |

A field you leave out leaves that scope alone. A field you set **replaces** this plugin's whole
object in that scope. Read the old value, change it, and hand the whole thing back. Do not expect
a merge.

Storing an empty table is how a command clears a scope. `requires_settings` reads an empty table
as nothing stored, so the two agree without a second concept.

A `column` write with no column under it is dropped. qrate does not invent a column for it.

A command that fails is logged, not shown in the Problems panel.

## Bar items

A bar item is text in the status bar or the title bar. Use one when the plugin has something to
say about the whole project. Use a right-click entry when the plugin acts on one column. A
connection check belongs on the bar. A "restrict this column" command does not.

```lua
bar = {
  {
    id = "conn",
    bar = "status",
    side = "right",
    text = "Islandora ?",
    tooltip = "Click to check the Islandora connection",
    left = { command = "check" },
    right = { menu = {
      { label = "Check now", command = "check" },
      { label = "Stop checking", command = "stop" },
    } },
  },
},
```

| Field | Required | Value |
|---|---|---|
| `id` | yes | Names the item inside this plugin. [`qrate.status.set`](#retitling-an-item) addresses it. |
| `bar` | yes | `"status"` or `"title"`. |
| `side` | yes | `"left"` or `"right"`. |
| `text` | yes | The first text to show. See [Markup](#markup). |
| `tooltip` | no | Plain text. Markup does not apply here. |
| `left` | no | What the left mouse button does. |
| `right` | no | What the right mouse button does. |

An action is either `{ command = "name" }` or `{ menu = { { label = …, command = … }, … } }`. An
action that declares both stops the plugin from loading, and so does an unknown `bar` or `side`.
An item with no action shows text and does nothing.

### The context a bar command gets

A bar item has no column under it, so qrate fills the column fields from the table selection
instead:

- A cell or column selection gives `ctx.column`, `ctx.settings.column`, and `ctx.values`.
- A row selection, or no selection at all, leaves `ctx.column` as `nil`.

### Retitling an item

```lua
qrate.status.set(id, text)
```

This changes the text of one of your own declared items. It works from `on_command` and from
`validate`. The call buffers the new text, and qrate applies it after your function returns. A
plugin cannot retitle another plugin's item, and an `id` that names no declared item does nothing.

```lua
on_command = function(command, ctx)
  qrate.status.set("conn", "[muted]checking…[/]")
  local response, err = qrate.http.get(server)
  qrate.status.set("conn", err and "[red]~~Islandora~~[/]" or "[green]**Islandora** ✓[/]")
end,
```

Say what is happening before a slow call. A request can take ten seconds, and a bar that shows
nothing in that time reads as a click that did nothing.

### Markup

The markup is inline only. A bar holds one line, so there are no links, no lists, and no blocks.

| Spelling | Result |
|---|---|
| `**text**` | Bold |
| `*text*` | Italic |
| `~~text~~` | Strikethrough |
| `__text__` | Underline |
| `[name]text[/]` | Color |

Note one difference from CommonMark. Here `__` means underline. It is not a second spelling of
bold.

Unicode passes through untouched, so an icon needs no markup. `⏳ **Islandora**` works.

#### Colors

The color tags follow the style of the Python Rich library. Open with a color name in brackets.
Close with `[/]`, or with `[/name]`.

| Tag | Also | Use |
|---|---|---|
| `[red]` | `[danger]` | A failure |
| `[green]` | `[success]` | A success |
| `[yellow]` | `[warning]` | A warning |
| `[blue]` | `[info]` | Information |
| `[accent]` | | Emphasis |
| `[muted]` | | Text that matters less |

Each name resolves through the current theme, so `[red]` is the theme's red and not a fixed value.
The text stays readable when the user changes to a light theme. This is why the list holds roles
and not every color.

#### Nesting and literal text

Markup nests. `[green]**Islandora** ✓[/]` is bold inside green.

Text that does not close renders as itself, which keeps ordinary prose safe:

- `2 * 3 things` shows the asterisk.
- `[green]never closed` shows the tag.
- `[mauve]…` shows the tag, because no theme color carries that name.
- `[see note]` shows the brackets, because a tag name cannot hold a space.

### Where items appear

qrate groups items by bar and by side. Inside a group, items appear in the order the plugins
loaded. Plugin text in the status bar sits to the left of qrate's own readouts. Plugin text in the
title bar sits inboard of the dock buttons.

Reloading plugins replaces every item. An item whose plugin no longer declares it goes away.

## Suggestions

```lua
suggest = function(ctx) -> { string }
```

qrate calls `suggest` while the user types in a cell, and puts what you return under the editor.

| Field of `ctx` | Value |
|---|---|
| `column` | Header text. |
| `row` | 1-based, the cell being edited. |
| `prefix` | The text in that cell as it stands. |
| `settings` | [The settings table](#reading-settings). |

qrate waits for a short pause in typing before it asks, and it drops an answer that a newer
keystroke has superseded. A `suggest` that takes as long as one network request is therefore
acceptable.

## Settings

A plugin declares its knobs, and qrate renders and stores them. The host never reads inside a
plugin's object, which is what lets you add a knob without a qrate release.

### Declaring a knob

```lua
settings = {
  { key = "server", label = "Server address", type = "text", scope = "project",
    description = "The site this plugin checks against." },
  { key = "password", label = "Password", type = "password", scope = "user" },
},
```

| Field | Required | Value |
|---|---|---|
| `key` | yes | Names a field inside this plugin's object in `scope`. |
| `label` | yes | What the row reads as in Settings. |
| `description` | no | A line under the row. |
| `scope` | yes | `"user"` or `"project"`. |
| `type` | yes | `"switch"`, `"text"`, or `"password"`. |

qrate puts every declared knob on a Settings page named after the plugin.

A `password` knob is masked as it is typed. qrate refuses a `password` in project scope, because
the `.qrate` file gets shared and committed. Put credentials in user scope.

qrate merges a knob into the plugin's object by key. This is unlike a command's write, which
replaces the whole object.

### Reading settings

Every hook receives the same settings table:

| Field | What it holds |
|---|---|
| `column` | This plugin's object for the column in hand, from the project file. |
| `project` | This plugin's project-scope object, from the `.qrate` file. It travels with the project. |
| `user` | This plugin's user-scope object, stored per machine. |
| `app` | App-wide values the plugin must agree with rather than restate. |

A plugin sees only its own objects. Another plugin's settings are invisible.

A scope with nothing stored arrives as an empty table, never `nil`, so a read needs no guard.

`app` currently holds one field:

| Field | Value |
|---|---|
| `subdelimiter` | What separates several values inside one cell, such as `;` in `Film; Video`. Empty means the cell holds one indivisible value. |

Split cells with `app.subdelimiter`. A plugin that splits them differently disagrees with the
column filter the user can see.

## Column mapping

A plugin that holds a list of terms can offer one picker per column.

```lua
column_map = {
  key = "vocabulary",
  label = "Islandora vocabulary",
  description = "Check this column against one of the site's vocabularies.",
  options = "vocabularies",
  refresh = "fetch_vocabularies",
  multiple = false,
},
```

| Field | Required | Value |
|---|---|---|
| `key` | yes | The field written into each column's own bucket. `validate` reads it back as `settings.column[key]`. |
| `label` | yes | What the picker reads as. |
| `description` | no | A line under the picker. |
| `options` | yes | The key in this plugin's **project-scope** object that holds the list. |
| `refresh` | yes | The command qrate runs when the user clicks Refresh. |
| `multiple` | no | Whether one column may carry several options. Default `false`. |

Declare it once and qrate renders it twice: as a per-column picker on the plugin's Settings page,
and as a checkable submenu on the column header. Both write to the same place, so the two cannot
disagree.

No plugin code runs to draw either one. The options are whatever the plugin last stored under
`options`, which is what lets a list fetched from a server appear in a menu that qrate must build
while the user waits. Store each option as a plain string, or as `{ value = …, label = … }`.

Your `refresh` command is what fills that list. Fetch it, then return it as a project-scope write.

## Storage

```lua
qrate.storage.get(key)        -- the stored value, or nil
qrate.storage.set(key, value) -- store it
```

This is the plugin's own cache. qrate keeps it beside the app's data and **not** in the project
file: what a plugin caches is about this machine, and a project file is a thing people commit.

The cache survives a restart. qrate writes it out after a call that changed it.

Both directions convert the whole value on every call. Reading a cache of ten thousand entries
costs ten thousand conversions each time you ask for it. Read it once into a local and keep it
across calls.

## Network

Network access is the only permission that exists. Declare it, and the user grants it in
**Settings ▸ Plugins**:

```lua
permissions = { "net" },
```

```lua
local response, err = qrate.http.get(url, { auth = { username = u, password = p } })
```

`get` answers a response table, or `nil` and a message. An unreachable server is a thing to
report, not a thing to crash on. The response carries `status` and `body`.

Until the user grants `net`, `qrate.http.get` answers `nil` and a message that says so. Your code
needs no second path for the ungranted case.

`auth` sends HTTP basic authentication. A blank or missing pair is the same as sending none, so
one code path works whether or not the user has filled the credential settings in. Basic
authentication lives in the host because Luau has no base64 and no way to build a header.

qrate owns the timeout, the TLS policy, and the redirect policy. It also rate limits calls per
plugin, and a refused call spends a request too, so a loop against a dead server cannot spin.

### Reading a response

```lua
local body, err = qrate.json.decode(response.body)
```

`decode` answers a table, or `nil` and a message. A server that returns an error page or a bot
check instead of JSON is common enough to handle rather than raise.

JSON `null` arrives as `nil`, so a missing field and an explicitly null one read the same.

## Modules and logging

```lua
local api = require("api")
```

`require` loads a sibling `.lua` file from this plugin's own folder, and nothing else. There is no
path syntax: qrate reads the folder, and a name resolves only against what it found there. A
module evaluates once, however many times you require it. Split a plugin into modules when one
file stops being readable, and not before.

```lua
print("checking " .. tostring(ctx.column))
```

`print` writes to qrate's session log, tagged with the plugin's name. A packaged build has no
console, so this is the only `print` anybody will read. Reach the log with **Help ▸ Copy Debug
Info**.

## Limits

| Limit | Value |
|---|---|
| Memory per plugin | 64 MB |
| One call into Lua | 2 seconds |
| HTTP requests | 120 per minute, per plugin |
| One HTTP request | 10 seconds |

qrate logs a warning when a call takes longer than 150 ms, naming the plugin, the entry point, and
the time. A slow build is diagnosable from an ordinary bug report's log tail.


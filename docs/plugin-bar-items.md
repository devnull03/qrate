# Bar items

A plugin can put text in the status bar or the title bar. The text carries inline markup for weight,
decoration, and color. Each item accepts a left-click action and a right-click action. Each action is
either one command or a menu of commands.

Use a bar item when the plugin has something to say about the whole project. Use a right-click menu
entry when the plugin acts on one column. A connection check belongs on the bar. A "restrict this
column to these values" command does not.

## Declaring an item

Add a `bar` list to the table your plugin returns. The host reads it once, when it loads the plugin.

```lua
return {
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
        { label = "Stop checking this column", command = "stop" },
      } },
    },
  },

  on_command = function(command, ctx) end,
}
```

| Field | Required | Value |
| --- | --- | --- |
| `id` | yes | Names the item inside this plugin. `qrate.status.set` uses it. |
| `bar` | yes | `"status"` or `"title"`. |
| `side` | yes | `"left"` or `"right"`. |
| `text` | yes | The first text to show. See [Markup](#markup). |
| `tooltip` | no | Plain text. The markup does not apply here. |
| `left` | no | The left-click action. |
| `right` | no | The right-click action. |

An action is `{ command = "name" }` or `{ menu = { { label = "…", command = "…" }, … } }`. An action
that declares both stops the plugin from loading. An item with no action shows text and does nothing.

An unknown `bar` or `side` also stops the plugin from loading. The failure goes to the Problems
panel under the plugin name.

## Commands from a bar item

A click calls `on_command` with the command name you declared. The host runs it off the interface
thread, so a slow command does not freeze the grid.

The context is the same table a menu command gets, with one difference. A bar item has no column
under it. The host fills the column fields from the table selection instead:

- A cell or column selection gives `ctx.column`, `ctx.settings.column`, and `ctx.values`.
- A row selection, or no selection at all, leaves `ctx.column` as `nil`.

Check `ctx.column` before you read `ctx.values`. A column write that a bar command returns needs a
column to land in. The host drops the write when there is none.

## Updating the text

Call `qrate.status.set(id, text)` to retitle one of your own items. The call works from `on_command`
and from `validate`. It buffers the new text. The host applies it after your function returns.

```lua
on_command = function(command, ctx)
  qrate.status.set("conn", "[muted]checking…[/]")
  local response, err = qrate.http.get(server)
  qrate.status.set("conn", err and "[red]~~Islandora~~[/]" or "[green]**Islandora** ✓[/]")
end,
```

Say what is happening before a slow call. A request can take ten seconds. A bar that shows nothing
in that time reads as a click that did nothing.

An `id` that names no declared item does nothing. A plugin cannot retitle another plugin's item.

## Markup

The markup is inline only. A bar is one line, so there are no links, no lists, and no blocks.

| Spelling | Result |
| --- | --- |
| `**text**` | Bold |
| `*text*` | Italic |
| `~~text~~` | Strikethrough |
| `__text__` | Underline |
| `[name]text[/]` | Color |

Note one difference from CommonMark. `__` means underline here. It is not a second spelling of bold.

Unicode passes through untouched. An icon needs no markup: `⏳ **Islandora**` works.

### Colors

The color tags follow the style of the Python Rich library. Open with the color name in brackets.
Close with `[/]`, or with `[/name]` if you prefer to name it.

| Tag | Also | Use |
| --- | --- | --- |
| `[red]` | `[danger]` | A failure |
| `[green]` | `[success]` | A success |
| `[yellow]` | `[warning]` | A warning |
| `[blue]` | `[info]` | Information |
| `[accent]` | | Emphasis |
| `[muted]` | | Text that is less important |

Each name resolves through the current theme. `[red]` is the red of the theme in use, not a fixed
value. Your text stays readable when the user changes to a light theme. This is why the list holds
roles and not every color.

### Nesting and literal text

Markup nests. `[green]**Islandora** ✓[/]` is bold inside green.

Text that does not close renders as itself. This keeps ordinary prose safe:

- `2 * 3 things` shows the asterisk.
- `[green]never closed` shows the tag.
- `[mauve]…` shows the tag, because no theme color has that name.
- `[see note]` shows the brackets, because a tag name cannot hold a space.

## Where items appear

The host groups items by bar and by side. Within a group, items appear in the order the plugins
loaded. Plugin text in the status bar sits to the left of the built-in readouts. Plugin text in the
title bar sits inboard of the dock buttons.

Reloading plugins replaces every item. An item whose plugin no longer declares it goes away.

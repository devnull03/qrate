# Floating editor migration

## Decision

qrate will own the floating editor component.

The component will use the gpui-base `TextareaState` editing engine. It will not reimplement text
editing.

The component will replace `gpui_component::input::Textarea` in the table and Details field
editors. The note editor can keep the gpui-component wrapper until it needs the same geometry.

## Reason

The floating editor must match the table cell padding. It must also follow the table viewport.

gpui-component 0.6 gives each `Textarea` fixed 10 px horizontal and 8 px vertical padding. Its
public API cannot change that padding.

qrate needs 8 px horizontal and 4 px vertical padding. Negative offsets can align the text, but
they also move the scrollbar outside the card.

A qrate component can set the gpui-base editor padding directly. It can keep the existing editing
engine and remove the layout workaround.

## Scope

Create `crates/table/src/editor.rs`.

The module will own:

- the floating editor frame
- the 8 by 4 px content padding
- theme values for text, caret, selection, and diagnostics
- the native text context menu
- accessibility metadata
- wrap measurement
- horizontal and vertical growth
- the returned editor size

The module will not own:

- edit start, commit, or cancel
- table writes or undo history
- suggestions
- autosave
- table or Details focus restoration
- note editing

## Public API

Keep the current call shape:

```rust
pub fn editor_box(
    editor: &Entity<TextareaState>,
    label: SharedString,
    anchor: Bounds<Pixels>,
    within: Bounds<Pixels>,
    window: &mut Window,
    cx: &App,
) -> (AnyElement, Size<Pixels>)
```

The label identifies the edited field for accessibility tools.

The returned size remains the source for suggestion placement.

## Dependencies

Add `gpui-base = "0.6"` as a direct workspace dependency.

Add the dependency to the table crate. Do not rely on the transitive dependency from
gpui-component.

Keep `gpui-component` for the rest of the table UI.

## Component structure

Use this element structure:

```text
qrate floating frame
  gpui_base::input::InputBase
    gpui_base::input::Textarea
      TextareaState
```

Use the gpui-base state for:

- IME input
- cursor movement
- selection
- clipboard actions
- undo and redo
- soft wrapping
- scrolling
- text shaping

Do not copy these behaviors into qrate.

## State setup

Set the editor padding on each render:

```text
top: 4 px
right: 8 px
bottom: 4 px
left: 8 px
```

Set the gpui-base `InputEditorStyle` from the active qrate theme.

Map these theme values:

- foreground
- muted foreground
- editor background
- border
- selection
- caret
- diagnostic colors
- highlight styles
- invisible text
- active line
- gutter background

Keep soft wrapping enabled. Keep the current Enter and Shift+Enter behavior in the existing
subscriptions.

## Context menu

Install a native text menu through the gpui-base context-menu hook.

Include:

1. Cut
2. Copy
3. Paste
4. Select All

Use `InputContextMenuCapabilities` to disable unavailable actions.

Do not add table row, column, plugin, or suggestion actions to this menu.

## Focus

Use the public `TextareaState` focus handle.

Keep the existing focus return paths:

- Enter returns focus to the table.
- Escape cancels and returns focus to the table.
- Details field commit returns focus to the Details panel.
- Blur commits without taking focus from the new target.

qrate does not currently use gpui-component focused-input lookup. Do not add a replacement
registry until a feature needs it.

Future agent tools can use the same component without gaining write access. The existing table
commit path remains the only data mutation boundary.

## Accessibility

Use `InputBase` as the semantic frame.

Set a multiline text-input role. Add a stable element ID.

Give each editor a specific label:

- a cell label includes its column and row
- a header label includes the column name
- a Details label includes the field name

Verify the accessible value and `SetValue` behavior. Add the smallest qrate handler if gpui-base
does not supply it.

## Geometry

Use one geometry model:

```text
content width = outer width - 16 px padding - 2 px border
content height = outer height - 8 px padding - 2 px border
```

Keep the spreadsheet growth rule:

1. Start at the source cell width and height.
2. Grow right until each hard line fits.
3. Stop horizontal growth at the table boundary.
4. Grow down when text still wraps.
5. Stop vertical growth at the table boundary.
6. Use scrolling only after the editor reaches that boundary.

Use the GPUI line wrapper to count display lines. Do not use `shape_text` as a wrap estimate.

Cache measurements for the active edit. Use this cache key:

```text
value
font
font size
anchor width
available width
```

Clear the cache after a value, font, scale, anchor, or viewport change.

## Migration steps

All twelve are done. `cargo test -p table -p workspace` and `cargo clippy -p table -p workspace
--all-targets -- -A dead_code -D warnings` are green.

1. ✅ Add the direct gpui-base dependency.
2. ✅ Add `crates/table/src/editor.rs`.
3. ✅ Exact padding setup. **The theme mapping turned out to be unnecessary**: gpui-base resolves an
   unset `InputEditorStyle` against `gpui_base::Theme` on every render, and gpui-component already
   mirrors `cx.theme()` into that global. Projecting the palette by hand would only restate it, so
   `configure` sets no colours at all.
4. ✅ Native context menu. The menu gpui-base offers the handler is a model with no `show`; the one
   that can present itself is `gpui_component::native_menu::NativeMenu`, so the argument is dropped
   and a component menu is built — which is what `Input` does with it too.
5. ✅ Accessibility frame: `InputBase`, `Role::MultilineTextInput`, the caller's label, an
   accessible value only while a client is listening, and a `SetValue` handler.
6. ✅ Move the sizing helpers into the new module.
7. ✅ Measurement caching, keyed as below.
8. ✅ Replace the table editor presentation.
9. ✅ Replace the Details field editor presentation. `DetailsPanel::editing` grew the field name so
   the editor has a label; the column name is `pub(crate)` to the table crate.
10. ✅ Overscan and `.overflow_hidden()` gone. `TEXTAREA_RIGHT_MARGIN` stays — it is a real private
    upstream constant with no public equivalent, and a test pins the inset it feeds.
11. ✅ Call sites and edit lifecycle unchanged apart from the new `label` argument.
12. ✅ `panel.rs` no longer imports `Textarea`. `note.rs` keeps the gpui-component wrapper.

Two departures from the plan as written:

- `editor_box` takes `&mut App`, not `&App` — configuring the state needs it.
- It returns `InputBase`, not `AnyElement`. The grid hangs its scroll tab and suggestion list off
  the returned box, and `AnyElement` would take `.when`/`.children` away from that call site.

## Tests

Automated in `crates/table/src/editor.rs`:

### Geometry

- short text keeps the cell size
- text grows right before it wraps
- a final word stays on the first line when it fits
- hard newlines grow the editor down
- a right-edge cell does not cross the table boundary
- a bottom-edge cell scrolls only after it reaches the boundary
- the text inset is 8 by 4 px

The last two acceptance items — the scrollbar staying inside the frame, and everything under
**Editing parity**, **Accessibility** and **Platforms** — are still manual. Nothing here drives a
real `TextareaState` through a window, so they are the open work.

### Editing parity

- IME composition commits non-ASCII text
- mouse and keyboard selection work across wrapped lines
- Cut, Copy, Paste, and Select All work
- undo and redo preserve their current grouping
- Enter commits
- Shift+Enter inserts a newline
- Escape cancels
- blur commits without stealing focus

### Accessibility

- the role is a multiline text input
- the label identifies the edited field
- the accessible value matches the text
- accessible `SetValue` replaces the value

### Platforms

Test on Windows, macOS, and Linux.

Test Windows at 100, 125, and 150 percent scaling.

## Acceptance criteria

- The editor text aligns with the source cell text.
- Text that fits has no useful scrollbar.
- The editor grows right before it grows down.
- The editor never crosses the table or Details viewport.
- The scrollbar stays inside the frame.
- The component preserves IME, selection, clipboard, undo, focus, and accessibility behavior.
- No editor layout uses gpui-component private padding values.
- The table and Details panel use the same component.

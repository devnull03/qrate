# Responsive headers

## Design

A header has three representations supplied by its owner:

1. The normal header, including its inline controls.
2. A compact title containing text, without a second set of controls.
3. A builder for standard dropdown menu items representing those controls.

`window-wrapper::responsive_header::ResponsiveHeader` handles measurement and switching.
It accepts a menu builder, not popup content. Putting the original header into a popup is
therefore not part of its API.

## Layout

- Measure the normal header's intrinsic width, including controls inside the title slot.
- Stretch the visible header to its parent's full width so existing alignment stays intact.
- When the controls cannot fit, show the compact title and one menu button.
- Truncate the compact title. Keep the menu button at its fixed size.
- The main window measures its controls with a three-rem title placeholder. Long project and
  author names truncate before the controls collapse. Native window buttons remain outside
  this layout, and macOS retains its traffic-light padding.

The dock skin applies the wrapper centrally. Existing panels and newly registered panels
receive the same collapse behavior.

## Menu contract

Panel owners expose their controls through `Panel::dropdown_menu`. Every inline control must
have a menu equivalent, using the same choices and current state. Use checked items for view
and filter choices, checkboxes for independent source choices, and disabled items for commands
that cannot run. Sliders use a submenu of discrete values when that matches their range.

The dock skin adds tab selection, zoom, and close commands. Compact panel labels come from
`PanelMeta` or `Panel::tab_name`, so filter tabs do not reappear inside the compact title.

The main titlebar reads the same owned app menus as `AppMenuBar`, preserving nested actions,
checks, and disabled states. Its other entries cover feedback, dock toggles, plugin commands,
and visible update controls. Plugin commands use the existing dispatcher.

Use the component library's `DropdownMenu` and `PopupMenu`, including their keyboard and
dismissal behavior. The wrapper remembers the focus target before opening and passes it to
the menu builder; nested app menus inherit it so actions still reach the document.

Occlusion belongs around the dropdown host. An occluding trigger blocks the host's own
mouse-down handler and prevents the menu from opening.

## Review when adding a control

Check its expanded and collapsed forms at narrow and wide widths, including selected and
disabled state, keyboard activation, Escape, action dispatch, and focus after dismissal.
Confirm that a resize restores the original alignment and keeps the window buttons visible.

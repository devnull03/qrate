# Responsive headers

## Design

A header has an ordered list of stages supplied by its owner. Each stage contains:

1. The controls that remain visible inline.
2. An optional builder for standard dropdown items representing the hidden controls.
3. An optional measurement representation and truncation policy.

`window-wrapper::responsive_header::ResponsiveHeader` handles measurement and switching.
`HeaderStage` describes each candidate. `ResponsiveHeader::new` keeps the default two stages:
the full header, followed by a compact title and dropdown. `from_stages` accepts a custom list.
Dropdowns accept menu items rather than the original header as popup content.

## Panel policy

Panels can implement `window-wrapper::panel_headers::PanelHeaders` and register their type
with `PanelHeaderRegistry::register`, using their `Panel::panel_name`. The dock skin passes
the default stages and a shared menu builder for tab selection, zoom, and close commands.
The panel supplies its stages and combines that builder with its own hidden controls.
The registry stores function pointers, not panel entities; content closures can use weak
panel references. Panels without a registered policy retain the default two stages.

Problems supplies four stages, from widest to narrowest:

1. Named severity tabs, source picker, and ignored toggle.
2. Named severity tabs and hamburger; source and ignored controls move into the menu.
3. Severity icons and counts with label tooltips; secondary controls stay in the menu.
4. Plain Problems title and hamburger; severity choices also move into the menu.

Intermediate menus omit the severity choices that remain visible inline. All stages share
the same filter state and callbacks.

## Layout

- Measure each stage's intrinsic width, including controls inside the title slot and the
  hamburger's width, gaps, and padding. Choose the first stage that fits.
- Stretch the visible header to its parent's full width so existing alignment stays intact.
- Collapse immediately when controls stop fitting. Require eight extra pixels to restore a
  wider stage, preventing oscillation around a breakpoint.
- Use the final stage as the fallback. Truncate its title and keep the menu button fixed.
- Measurement probes have separate element IDs and a stateless box in place of the
  hamburger. Only the selected stage creates a live dropdown and receives paint/input.
  Content builders also run during measurement and must not mutate application state.
- Changing stages dismisses an open dropdown and restores the captured action focus.
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

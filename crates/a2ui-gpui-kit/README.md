# a2ui-gpui-kit

An [A2UI](https://github.com/google/A2UI) v1.0 renderer for
[gpui-kit](https://crates.io/crates/gpui-component) (`gpui-component`), the widget library for
[gpui](https://www.gpui.rs).

An agent streams A2UI JSON messages; this crate validates them against a strict subset of the
basic catalog, keeps each surface's component map and data model, and draws the result with
gpui-kit widgets — `Label`, `Separator`, `GroupBox`, `Input`, `Checkbox`, `RadioGroup` and
`Button` inside `v_flex`/`h_flex` layouts, coloured by the active gpui-kit theme. A user's click
on a `Button` comes back as an A2UI `action` message for the host to send to the agent.

It has no dependencies beyond gpui, gpui-component, serde, serde_json, regex, thiserror and log.

## Use

```rust
use std::collections::HashMap;
use a2ui_gpui_kit::{AgentMessage, Limits, Surface, SurfaceEvent, SurfaceState};
use gpui::{App, AppContext, Entity};

struct Host {
    surfaces: HashMap<String, Entity<Surface>>,
}

impl Host {
    /// Returns the reply to send to the agent, if any.
    fn receive(&mut self, json: &str, cx: &mut App) -> Option<serde_json::Value> {
        let message = match AgentMessage::parse(json) {
            Ok(message) => message,
            Err(error) => return Some(error.to_message()),
        };
        let id = message.surface_id().to_owned();
        if let Some(surface) = self.surfaces.get(&id) {
            // An update, a delete, or a duplicate createSurface (INTEGRITY_ERROR).
            return surface.update(cx, |s, cx| s.apply(&message, cx)).err().map(|e| e.to_message());
        }
        let AgentMessage::CreateSurface(create) = message else {
            return None; // no such surface: reply with an INTEGRITY_ERROR of your own if you like
        };
        let state = match SurfaceState::create(create, Limits::default()) {
            Ok(state) => state,
            Err(error) => return Some(error.to_message()),
        };
        let surface = cx.new(|_| Surface::new(state));
        cx.subscribe(&surface, |_, event: &SurfaceEvent, _| match event {
            SurfaceEvent::Action(action) => { /* send action.to_message() to the agent */ }
            SurfaceEvent::Error(error) => { /* report or log error.to_message() */ }
            SurfaceEvent::Deleted => { /* drop the entity */ }
        })
        .detach();
        self.surfaces.insert(id, surface);
        None
    }
}
```

`Surface` is an ordinary gpui view: put the entity inside your own element tree. Call
`gpui_component::init(cx)` once at start-up, as for any gpui-kit app.

The core (`protocol`, `surface`, `checks`, `pointer`) has no gpui types. `SurfaceState::tree()`
returns the fully resolved tree — bindings evaluated, `List` templates expanded, checks run — so
the protocol logic can be tested, or drawn with another toolkit, without a window.

## Security model

- **The host owns the chrome.** A `Surface` draws content only: it never opens windows, dialogs
  or popovers, and it renders no host buttons. Frame the surface so the user can tell it was
  written by an agent, and keep any confirmation for a destructive operation (delete, send, pay)
  in your own UI, outside the agent's surface — an agent can draw a button that *looks* like
  yours.
- **Actions are requests, not authorization.** `SurfaceEvent::Action` is emitted only when the
  user clicks an enabled `Button`. Bindings, data model updates and checks never emit one. The
  action carries values from the data model, which the agent itself can write; the agent side
  must re-validate anything it acts on.
- **Everything is bounded.** `Limits` caps components (500), depth (16), string length (8192
  characters), data model size (256 KiB) and expanded nodes (2000). A message that would exceed
  one is rejected with an `error`; the renderer never panics on agent input. Rejected messages
  leave the surface unchanged.
- **Regex is linear time.** `regex` checks run on Rust's `regex` engine, which cannot backtrack
  catastrophically, with a 1 MiB compiled-size cap.
- **No outbound effects.** No URLs are opened and no media is fetched; components that would need
  them are rejected.

## Supported subset

| Messages (agent → renderer) | |
|---|---|
| `createSurface` | requires `catalogId` = the basic catalog; `dataModel` is applied before `components` |
| `updateComponents` | upsert by id; transactional |
| `updateDataModel` | JSON Pointer upsert; `null` removes; omitted path or `/` replaces the root (must be an object) |
| `deleteSurface` | clears the surface and emits `SurfaceEvent::Deleted` |
| `callRendererFunction` | rejected with `INVALID_FUNCTION_CALL` |

| Components | Rendered as |
|---|---|
| `Text` (`body`, `caption`) | `Label` (plain text; Markdown is not interpreted) |
| `Row`, `Column` (`justify`, `align`, child templates) | `h_flex` / `v_flex` |
| `List` (`direction`, `align`, child templates) | `v_flex` / `h_flex` — one child per array item |
| `Card` | outlined `GroupBox` |
| `Divider` | `Separator` |
| `TextField` (`shortText`, `longText`, `number`, `obscured`) | `Input`, two-way bound; values are written as strings |
| `CheckBox` | `Checkbox`, two-way bound |
| `ChoicePicker` (`mutuallyExclusive`, `multipleSelection`) | `RadioGroup` / a `Checkbox` per option; writes a string array |
| `Button` (`default`, `primary`, `borderless`) | `Button`; disabled while any `@checks` entry fails |

`weight` (flex grow), `@checks` on the four checkable components, and `@accessibility.label` on
`Button` are honoured. Functions: `required`, `length`, `regex`, `and`, `or`, `not`.

**Rejected** with `VALIDATION_FAILED` and the JSON Pointer of the offending field: `Image`,
`Icon`, `Video`, `AudioPlayer`, `Tabs`, `Modal`, `Slider`, `DateTimeInput`, any other component
type, every other function (`formatString`, `email`, `numeric`, `openUrl`, `@index`, ...),
local `functionCall` actions, other catalogs, `theme`, unknown properties, and malformed ids.

## Deviations and notes

- Progressive rendering is buffered, not partial: components may arrive in any order before
  `root` exists, but once `root` exists every message must leave the tree closed — a reference to
  an id that is not defined, or a cycle, rejects the message instead of rendering a placeholder.
- The regex dialect is translated onto Rust `regex`. `\d`, `\w`, `\b` are ASCII-only and `.`
  excludes all four line terminators as the dialect says. Lookahead is rejected because the engine
  has none; the spec's static catastrophic-backtracking rules 2–4 are not enforced because they
  cannot apply to a linear-time engine.
- `sendDataModel` is parsed and exposed as `SurfaceState::send_data_model()`; attaching the data
  model to transport metadata is the host's job.
- `longText` renders as a single-line input.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

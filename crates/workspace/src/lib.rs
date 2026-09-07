//! The main workspace view: a `gpui_component::dock::DockArea` hosting the app's panels
//! (center table, left details, right agent, bottom problems) with layout persistence.

mod dock_button;
mod panel_registry;
mod panels;
mod viewer;
mod views;

pub use viewer::{CloseViewerLayer, Scope as ViewerScope, VIEWER_CONTEXT, open_viewer};

pub use dock_button::DockToggleButton;
pub use panel_registry::{BarSide, PANELS, PanelMeta, PanelRegistry, bar_side};
/// `record` is how the agent bridge in `app` files an entry for the Agent panel to show.
pub use panels::{
    AGENT_FONT_KEY, AGENT_FONT_SIZE_KEY, AGENT_FONT_SIZES, AgentCall, AgentEntry, DETAILS_META,
    record as record_agent_call,
};
pub use views::{ShowView, VIEW_MODE_KEY, VIEWS_CONTEXT, ViewMode, show_view};

use std::sync::Arc;

use gpui::*;
use gpui_component::dock::{
    BasePanelView, DockArea, DockAreaState, DockEvent, DockLayout, DockPlacement, DockSkin,
    panel_handle, register_panel,
};
use settings::AppSettings;

use diagnostics::ProblemsPanel;

use crate::panel_registry::PROBLEMS_META;
use crate::panels::{AGENT_META, AgentPanel, DetailsPanel};
use crate::views::ViewsPanel;

/// Settings key under which the serialized [`DockAreaState`] is persisted —
/// both in the open project's `.qrate` `__settings` (preferred) and in the
/// global app settings (fallback when no project is open).
const DOCK_LAYOUT_KEY: &str = "main_dock_layout";
/// Layout schema version. Bumped to 3 when the centre became `ViewsPanel` (the view host) instead
/// of `TablePanel`, so a layout saved under the old name is discarded rather than restoring a
/// centre with no view switcher.
const DOCK_LAYOUT_VERSION: usize = 3;

/// gpui_component keeps a fixed ~29px title strip for a *closed* bottom dock (dock.rs) so it
/// stays clickable — for us it just "sticks out". We drive the bottom dock from our own bar,
/// so we crop that strip off the bottom instead.
const BOTTOM_DOCK_STRIP_PX: f32 = 29.;

/// How many pixels the strip crop above is currently eating off the bottom of the *side* docks,
/// for their panels to pad themselves back out by. `px(0.)` whenever the bottom dock is open.
///
/// Why they need it: the crop stretches the whole dock area past the bottom edge and clips, but
/// the library only parks the closed bottom dock's strip inside the *center* column — the left
/// and right docks are `h_full` of the entire area (`DockArea::render`), so the crop takes their
/// last 29px of real content with it. The center is unaffected (the strip occupied that space
/// anyway), which is why only the side panels compensate.
///
/// shouldn't have in their signatures; dies with `BOTTOM_DOCK_STRIP_PX`.
#[derive(Copy, Clone, Default, PartialEq)]
pub struct BottomDockCrop(pub Pixels);

impl Global for BottomDockCrop {}

/// Toggle a dock, collapse its panel tree, and publish the resulting layout in the same UI turn.
/// Every caller gets the same grid redraw and persistence behavior because the state change owns
/// its event.
pub(crate) fn toggle_dock_immediately(
    area: &mut DockArea,
    placement: DockPlacement,
    window: &mut Window,
    cx: &mut Context<DockArea>,
) -> bool {
    // The centre is not a dock, and a dock we never built cannot be toggled.
    if placement == DockPlacement::Center || !area.has_dock(placement) {
        return false;
    }
    // Collapsing the panel tree and emitting `LayoutChanged` are both `toggle_dock`'s job now —
    // it used to take driving the `Dock` entity by hand.
    area.toggle_dock(placement, window, cx);
    true
}

pub struct Workspace {
    dock_area: Entity<DockArea>,
    /// Persists the layout to settings whenever the dock emits `LayoutChanged`.
    _layout_sub: Subscription,
    /// Re-renders to mount/unmount the image viewer overlay when it opens or closes.
    _viewer_sub: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Register panel constructors so a persisted layout can be reconstructed by name.
        // The centre is the view host, not the grid — the grid is one view it mounts.
        // `panel_handle` rather than a bare entity: base stores what it is given, and a skin can
        // only recover presentation — our titles, the view switcher — from the wrapper. A bare
        // entity still docks and persists, it just draws its `panel_name` where the title goes.
        register_panel(cx, "ViewsPanel", |ctx, window, cx| {
            let weak = ctx.dock_area();
            panel_handle(cx.new(|cx| ViewsPanel::new(weak, window, cx)))
        });
        register_panel(cx, "DetailsPanel", |_ctx, window, cx| {
            panel_handle(cx.new(|cx| DetailsPanel::new(window, cx)))
        });
        register_panel(cx, "AgentPanel", |_ctx, window, cx| {
            panel_handle(cx.new(|cx| AgentPanel::new(window, cx)))
        });
        register_panel(cx, "ProblemsPanel", |_ctx, window, cx| {
            panel_handle(cx.new(|cx| ProblemsPanel::new(window, cx)))
        });

        // The appearance is a separate object in 0.6, installed as the area's renderer. The
        // handle it hands back is how its settings are reached afterwards.
        let (dock_area, skin) =
            DockSkin::dock_area("qrate-main", Some(DOCK_LAYOUT_VERSION), window, cx);

        // Restore if saved, else build default; building first then loading would orphan a throwaway table.
        if !Self::restore_layout(&dock_area, window, cx) {
            let weak = dock_area.downgrade();
            let centre = cx.new(|cx| ViewsPanel::new(weak.clone(), window, cx));
            // Which dock each panel starts in is the panel's own declaration, so they're built
            // as a flat list and grouped by it rather than one `set_*_dock` call apiece.
            let panels: [(&PanelMeta, Arc<dyn BasePanelView>); 3] = [
                (
                    &DETAILS_META,
                    panel_handle(cx.new(|cx| DetailsPanel::new(window, cx))),
                ),
                (
                    &PROBLEMS_META,
                    panel_handle(cx.new(|cx| ProblemsPanel::new(window, cx))),
                ),
                (
                    &AGENT_META,
                    panel_handle(cx.new(|cx| AgentPanel::new(window, cx))),
                ),
            ];

            dock_area.update(cx, |area, cx| {
                // `tabs`, not a bare panel: a bare centre has no title bar at all — and the title
                // bar is where the view switcher lives. Restoring a saved layout always produces
                // tabs anyway, so this only fixes the first launch, which would otherwise come up
                // with no way to leave the grid.
                area.set_center(
                    DockLayout::tabs().panel_view(panel_handle(centre), cx),
                    window,
                    cx,
                );
                for (placement, size) in [
                    (DockPlacement::Left, px(300.)),
                    (DockPlacement::Right, px(340.)),
                    (DockPlacement::Bottom, px(200.)),
                ] {
                    let mut layout = DockLayout::tabs();
                    let mut filled = false;
                    for (_, view) in panels
                        .iter()
                        .filter(|(meta, _)| meta.default_placement == placement)
                    {
                        layout = layout.panel_view(view.clone(), cx);
                        filled = true;
                    }
                    if !filled {
                        continue;
                    }
                    // Size is a property of the dock now, not an argument to filling it — and
                    // `set_dock` deliberately preserves the size of whatever it replaces.
                    area.set_dock(placement, layout, window, cx);
                    area.set_dock_size(placement, size, window, cx);
                }
            });
        }

        // We drive dock open/close from our own title/status-bar buttons, so hide the built-in
        // toggle arrows. A skin setting in 0.6, so it survives `load` without being reapplied.
        skin.set_toggle_button_visible(false, cx);

        // Saved layouts from older builds may contain arrangements qrate no longer permits.
        PanelRegistry::enforce_edge_tabs(&dock_area, window, cx);
        // Whichever path ran above built the panels; this is what learns where they landed.
        PanelRegistry::sync(&dock_area, cx);
        // A new main window starts in its document, not on the outer action-routing shell.
        PanelRegistry::focus_frontmost(DockPlacement::Center, &dock_area, window, cx);

        // Every dock mutation emits `LayoutChanged`, which normalizes user drags, refreshes the
        // status-bar placement cache, persists the result and repaints the workspace.
        let _layout_sub = cx.subscribe_in(
            &dock_area,
            window,
            |_this, area, event: &DockEvent, window, cx| {
                if matches!(event, DockEvent::LayoutChanged) {
                    PanelRegistry::enforce_edge_tabs(area, window, cx);
                    PanelRegistry::sync(area, cx);
                    Self::persist_layout(area, cx);
                    // Re-render the workspace so the bottom-strip crop tracks the dock's open state.
                    cx.notify();
                }
            },
        );

        let _viewer_sub = cx.observe_global::<viewer::ActiveViewer>(|_this, cx| cx.notify());

        Self {
            dock_area,
            _layout_sub,
            _viewer_sub,
        }
    }

    /// Weak handle to the dock area, so bar buttons can toggle panels open/closed.
    pub fn dock_area(&self) -> WeakEntity<DockArea> {
        self.dock_area.downgrade()
    }

    /// Open/close one dock. Driven by both the bar buttons and the keyboard shortcuts.
    pub fn toggle_dock(
        &mut self,
        placement: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_open = self.dock_area.read(cx).is_dock_open(placement);
        let closing_focus =
            PanelRegistry::placement_contains_focus(placement, &self.dock_area, window, cx);
        self.dock_area.update(cx, |area, cx| {
            toggle_dock_immediately(area, placement, window, cx);
        });
        if was_open {
            if closing_focus {
                PanelRegistry::focus_frontmost(DockPlacement::Center, &self.dock_area, window, cx);
            }
        } else {
            PanelRegistry::focus_frontmost(placement, &self.dock_area, window, cx);
        }
    }

    /// Serializes the dock state into the open project's `.qrate` (debounced,
    /// off the UI thread) or the global app settings when no project is open.
    ///
    /// Also the app-quit path (via `dock_area()`): dock edge-resizes never fire
    /// any event, so the final sizes are only guaranteed to be captured at quit.
    /// resize if gpui_component ever emits an event for it.
    pub fn persist_layout(dock_area: &Entity<DockArea>, cx: &mut App) {
        let state = dock_area.read(cx).dump(cx);
        let Ok(json) = serde_json::to_string(&state) else {
            return;
        };
        if let Some(project) = cx.try_global::<settings::project::CurrentProject>() {
            settings::project::queue_write(&project.file, DOCK_LAYOUT_KEY, &json, cx);
        } else {
            AppSettings::set_text(DOCK_LAYOUT_KEY, json.into(), cx);
        }
    }

    /// Re-reads the layout for whatever project is current and applies it over the
    /// dock's existing state. Needed because the main window is only ever created
    /// once per app session (see `WindowRegistry`); opening a *different* project
    /// while it's already open focuses it instead of re-running `Workspace::new`,
    /// so nothing else re-triggers `restore_layout` for the newly opened project.
    pub fn reload_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if Self::restore_layout(&self.dock_area, window, cx) {
            // `load` rebuilt every panel from its saved name, so the old entries are orphans.
            PanelRegistry::sync(&self.dock_area, cx);
            PanelRegistry::focus_frontmost(DockPlacement::Center, &self.dock_area, window, cx);
        }
    }

    /// Applies a previously saved dock arrangement, returning `true` if one was loaded. `false`
    /// (no saved layout / corrupt / stale version / load error) tells the caller to fall back to
    /// the default static layout.
    fn restore_layout(
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Prefer the open project's own saved layout; fall back to the global one so a
        // brand-new project still inherits a familiar arrangement.
        let project_layout = cx
            .try_global::<settings::project::CurrentProject>()
            .and_then(|p| settings::project::read_setting(&p.file, DOCK_LAYOUT_KEY).ok())
            .flatten();
        let Some(raw) = project_layout.or_else(|| {
            AppSettings::get(cx)
                .values
                .get(DOCK_LAYOUT_KEY)
                .map(|v| v.text().to_string())
        }) else {
            return false;
        };

        let mut value: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(err) => {
                log::warn!("ignoring corrupt dock layout: {err}");
                return false;
            }
        };
        prune(&mut value);
        let state: DockAreaState = match serde_json::from_value(value) {
            Ok(state) => state,
            Err(err) => {
                log::warn!("ignoring corrupt dock layout: {err}");
                return false;
            }
        };

        // Discard layouts saved under a different schema version.
        if state.version != Some(DOCK_LAYOUT_VERSION) {
            return false;
        }

        dock_area.update(cx, |area, cx| match area.load(state, window, cx) {
            Ok(()) => true,
            Err(err) => {
                log::error!("failed to restore dock layout: {err}");
                false
            }
        })
    }
}

/// A panel name a saved layout is allowed to hold: the three dockable ones plus the centre, which
/// has no `PanelMeta` because it never moves. Matches what [`Workspace::new`] registers, so adding
/// a panel to [`PANELS`] is still the only edit.
fn known_panel(name: &str) -> bool {
    name == "ViewsPanel" || PANELS.iter().any(|meta| meta.name == name)
}

/// Drop tab entries a layout should never have held, and re-point any tab index they invalidated.
///
/// A saved arrangement is user data that has been round-tripped through a dock library, and it can
/// come back holding things we never put there — a stray empty `TabPanel` sitting as a *tab*, which
/// the library draws as an "Unnamed" tab that renders an error. Left alone it is faithfully saved
/// again on the way out, so it reappears on every launch forever. Pruned on the way in, the next
/// write cleans the file up.
///
/// Depth-first: children are filtered before the level above decides whether the container that
/// held them is still worth keeping.
fn prune(node: &mut serde_json::Value) {
    match node {
        serde_json::Value::Array(items) => items.iter_mut().for_each(prune),
        serde_json::Value::Object(map) => {
            map.values_mut().for_each(prune);
            let Some(serde_json::Value::Array(children)) = map.get_mut("children") else {
                return;
            };
            children.retain(|child| {
                let name = child
                    .get("panel_name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default();
                // A container earns its place only while it still holds something.
                known_panel(name)
                    || child
                        .get("children")
                        .and_then(|kids| kids.as_array())
                        .is_some_and(|kids| !kids.is_empty())
            });
            let len = children.len();
            if let Some(active) = map
                .get_mut("info")
                .and_then(|info| info.get_mut("tabs"))
                .and_then(|tabs| tabs.get_mut("active_index"))
                && active.as_u64().is_some_and(|ix| ix as usize >= len)
            {
                *active = serde_json::json!(0);
            }
        }
        _ => {}
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Bottom dock closed: overhang 29px and clip to hide the library's residual strip; open: sit flush.
        let bottom_open = self.dock_area.read(cx).is_dock_open(DockPlacement::Bottom);
        let overshoot = if bottom_open {
            px(0.)
        } else {
            px(-BOTTOM_DOCK_STRIP_PX)
        };

        // Publish what the crop costs the side docks, so their panels can pad it back. Guarded:
        // `set_global` wakes every `observe_global`, and this runs on every workspace render.
        let crop = BottomDockCrop(-overshoot);
        if cx.try_global::<BottomDockCrop>() != Some(&crop) {
            cx.set_global(crop);
        }

        // A sibling overlay, not a dialog: it covers the dock but leaves the title bar's controls
        // reachable. Only the workspace-scoped viewer mounts here — a centre-scoped one is the
        // gallery's, and `ViewsPanel` mounts that inside the centre panel instead.
        let viewer = viewer::viewer_in(viewer::Scope::Workspace, cx);

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom(overshoot)
                    .child(self.dock_area.clone()),
            )
            .children(viewer)
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — this file's `use gpui::*` would shadow `#[test]`.
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{
        Context, FocusHandle, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
        ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, actions, div,
        point, px,
    };
    use gpui_component::menu::ContextMenuExt as _;
    use gpui_component::{
        Placement,
        dock::{DockEvent, DockPlacement, InsertTarget, PaneRef},
    };

    use crate::{
        BarSide, BottomDockCrop, PanelRegistry, Workspace, bar_side, prune, toggle_dock_immediately,
    };

    actions!(workspace_context_menu_test, [CloseMenu]);

    struct ContextMenuRoot {
        content_focus: FocusHandle,
        received: Rc<Cell<bool>>,
    }

    impl Render for ContextMenuRoot {
        fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
            let received = self.received.clone();
            div()
                .size_full()
                .child(
                    div()
                        .id("content")
                        .h(px(40.))
                        .track_focus(&self.content_focus),
                )
                .child(
                    div()
                        .id("action-bar")
                        .h(px(60.))
                        .on_action(move |_: &CloseMenu, _, _| received.set(true))
                        .child(
                            div()
                                .id("tab")
                                .size_full()
                                .context_menu(|menu, _, _| menu.menu("Close", Box::new(CloseMenu))),
                        ),
                )
        }
    }

    #[gpui::test]
    fn context_menu_restores_one_stable_focus_target(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let received = Rc::new(Cell::new(false));
        let (root, cx) = cx.add_window_view({
            let received = received.clone();
            move |window, cx| {
                let content_focus = cx.focus_handle();
                content_focus.focus(window, cx);
                ContextMenuRoot {
                    content_focus,
                    received,
                }
            }
        });
        let content_focus = root.read_with(cx, |root, _| root.content_focus.clone());
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Right,
            position: point(px(50.), px(70.)),
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        cx.simulate_keystrokes("down enter");
        cx.run_until_parked();

        assert!(received.get());
        cx.update(|window, cx| {
            assert_eq!(window.focused(cx).as_ref(), Some(&content_focus));
        });
    }

    #[gpui::test]
    fn dock_toggle_publishes_its_own_layout_event(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::SettingsPersistence::default());
        });
        let (workspace, cx) = cx.add_window_view(Workspace::new);
        let dock_area = cx.update(|_, cx| workspace.read(cx).dock_area.clone());
        let layout_events = Rc::new(Cell::new(0));
        let _subscription = cx.update(|_, cx| {
            let dock_area = dock_area.clone();
            let layout_events = layout_events.clone();
            workspace.update(cx, |_, cx| {
                cx.subscribe(&dock_area, move |_, _, event, _| {
                    if matches!(event, DockEvent::LayoutChanged) {
                        layout_events.set(layout_events.get() + 1);
                    }
                })
            })
        });

        cx.update(|window, cx| {
            dock_area.update(cx, |area, cx| {
                toggle_dock_immediately(area, DockPlacement::Bottom, window, cx);
            });
        });
        cx.run_until_parked();

        assert_eq!(layout_events.get(), 1);
        cx.update(|_, cx| assert_eq!(cx.global::<BottomDockCrop>().0, px(29.)));
    }

    #[gpui::test]
    fn opening_a_dock_focuses_it_and_closing_it_returns_to_the_document(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::SettingsPersistence::default());
        });
        let (workspace, cx) = cx.add_window_view(Workspace::new);
        let dock_area = cx.update(|_, cx| workspace.read(cx).dock_area.clone());
        cx.run_until_parked();

        cx.update(|window, cx| {
            PanelRegistry::focus_frontmost(DockPlacement::Center, &dock_area, window, cx);
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_dock(DockPlacement::Left, window, cx)
            });
            assert!(
                PanelRegistry::placement_contains_focus(
                    DockPlacement::Center,
                    &dock_area,
                    window,
                    cx,
                ),
                "closing an unfocused dock leaves document focus alone"
            );
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_dock(DockPlacement::Left, window, cx)
            });
            assert!(
                PanelRegistry::placement_contains_focus(
                    DockPlacement::Left,
                    &dock_area,
                    window,
                    cx,
                ),
                "opening a dock focuses its active panel"
            );
        });

        cx.dispatch_action(gpui_component::dock::ToggleZoom);
        cx.run_until_parked();
        assert!(
            cx.read(|cx| dock_area.read(cx).is_zoomed()),
            "Shift+Esc zooms the dock opened by shortcut"
        );
        cx.dispatch_action(gpui_component::dock::ToggleZoom);

        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_dock(DockPlacement::Left, window, cx)
            });
            assert!(
                PanelRegistry::placement_contains_focus(
                    DockPlacement::Center,
                    &dock_area,
                    window,
                    cx,
                ),
                "closing the focused dock returns focus to the document"
            );
        });
    }

    #[gpui::test]
    fn dragged_tools_become_edge_tabs_and_move_their_status_item(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::SettingsPersistence::default());
        });
        let (workspace, cx) = cx.add_window_view(Workspace::new);
        let dock_area = cx.update(|_, cx| workspace.read(cx).dock_area.clone());
        cx.run_until_parked();

        // Model a drop on the lower half of Details: gpui-kit first creates a split.
        cx.update(|window, cx| {
            let agent = PanelRegistry::entries(cx)
                .iter()
                .find(|entry| entry.meta.name == "AgentPanel")
                .map(|entry| entry.view.panel_id(cx))
                .expect("Agent is docked");
            let details_group = dock_area
                .read(cx)
                .layout(DockPlacement::Left)
                .expect("left dock")
                .root()
                .id();
            dock_area.update(cx, |area, cx| {
                area.move_panel(
                    agent,
                    InsertTarget::Split {
                        node: details_group,
                        placement: Placement::Bottom,
                        size: None,
                    },
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            let area = dock_area.read(cx);
            assert!(
                matches!(
                    area.layout(DockPlacement::Left)
                        .expect("left dock")
                        .root()
                        .kind(),
                    PaneRef::Tabs { .. }
                ),
                "the split drop is folded into the edge's tab group"
            );
            assert_eq!(
                PanelRegistry::placement("AgentPanel", cx),
                Some(DockPlacement::Left)
            );
        });

        // A normal tab drop into another edge stays there and refreshes the status-bar cache.
        cx.update(|window, cx| {
            let agent = PanelRegistry::entries(cx)
                .iter()
                .find(|entry| entry.meta.name == "AgentPanel")
                .map(|entry| entry.view.panel_id(cx))
                .expect("Agent is docked");
            let bottom_group = dock_area
                .read(cx)
                .layout(DockPlacement::Bottom)
                .expect("bottom dock")
                .root()
                .id();
            dock_area.update(cx, |area, cx| {
                area.move_panel(
                    agent,
                    InsertTarget::Tabs {
                        node: bottom_group,
                        ix: None,
                        activate: true,
                    },
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            let placement =
                PanelRegistry::placement("AgentPanel", cx).expect("Agent stays registered");
            assert_eq!(placement, DockPlacement::Bottom);
            assert!(matches!(bar_side(placement), BarSide::Centre));
        });

        // The document centre is not a tool dock; a drop there returns to the previous edge.
        cx.update(|window, cx| {
            let agent = PanelRegistry::entries(cx)
                .iter()
                .find(|entry| entry.meta.name == "AgentPanel")
                .map(|entry| entry.view.panel_id(cx))
                .expect("Agent is docked");
            let centre_group = dock_area
                .read(cx)
                .layout(DockPlacement::Center)
                .expect("centre layout")
                .root()
                .id();
            dock_area.update(cx, |area, cx| {
                area.move_panel(
                    agent,
                    InsertTarget::Split {
                        node: centre_group,
                        placement: Placement::Right,
                        size: None,
                    },
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            assert_eq!(
                PanelRegistry::placement("AgentPanel", cx),
                Some(DockPlacement::Bottom)
            );
            let area = dock_area.read(cx);
            let centre_names = area
                .layout(DockPlacement::Center)
                .expect("centre layout")
                .panels()
                .filter_map(|id| area.panel(id).map(|panel| panel.panel_name(cx)))
                .collect::<Vec<_>>();
            assert_eq!(centre_names, ["ViewsPanel"]);
        });
    }

    /// Shift+Esc, end to end on the real workspace. `app` binds the key to `dock::ToggleZoom`
    /// globally and handles it nowhere: the skin's frame carries the only handler, so the action
    /// has to travel from the focused panel up to the group holding it.
    ///
    /// Both halves of the zoom gate are load-bearing here. Every panel in this crate answered
    /// `zoomable() -> None` before the 0.6 port, and a panel that still refuses is skipped in
    /// silence — no error, no zoom — which is exactly what this asserts against.
    #[gpui::test]
    fn shift_escape_zooms_the_focused_panel(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::SettingsPersistence::default());
        });
        let (workspace, cx) = cx.add_window_view(Workspace::new);
        let dock_area = cx.update(|_, cx| workspace.read(cx).dock_area.clone());
        cx.run_until_parked();

        // Focus a docked panel, so the action starts from inside a tab group rather than at the
        // window root, where nothing would answer it.
        cx.update(|window, cx| {
            let panel = crate::PanelRegistry::entries(cx)
                .iter()
                .find(|entry| entry.meta.name == "DetailsPanel")
                .map(|entry| entry.view.clone())
                .expect("the default layout docks Details");
            panel.focus_handle(cx).focus(window, cx);
        });
        cx.run_until_parked();

        assert!(
            !cx.read(|cx| dock_area.read(cx).is_zoomed()),
            "nothing is zoomed to begin with"
        );

        cx.dispatch_action(gpui_component::dock::ToggleZoom);
        cx.run_until_parked();
        assert!(
            cx.read(|cx| dock_area.read(cx).is_zoomed()),
            "the focused panel's group fills the window"
        );

        // Zooming out is never refused, so the same key always gets the docks back.
        cx.dispatch_action(gpui_component::dock::ToggleZoom);
        cx.run_until_parked();
        assert!(!cx.read(|cx| dock_area.read(cx).is_zoomed()));
    }

    /// The real thing, lifted out of a project whose left dock came back with an "Unnamed" tab
    /// beside Details on every launch: an empty `TabPanel` sitting in a tab slot, and an
    /// `active_index` of 1 that points past the end once it is gone.
    #[test]
    fn a_saved_layout_loses_the_tabs_it_should_never_have_held() {
        let raw = r#"{"version":3,
          "center":{"panel_name":"TabPanel","children":[
            {"panel_name":"ViewsPanel","children":[],"info":{"panel":null}}],
            "info":{"tabs":{"active_index":0}}},
          "left_dock":{"panel":{"panel_name":"TabPanel","children":[
            {"panel_name":"TabPanel","children":[],"info":{"panel":null}},
            {"panel_name":"DetailsPanel","children":[],"info":{"panel":null}}],
            "info":{"tabs":{"active_index":1}}},
            "placement":"left","size":313.0,"open":true}}"#;

        let mut value: serde_json::Value = serde_json::from_str(raw).expect("valid json");
        prune(&mut value);

        let tabs = &value["left_dock"]["panel"];
        let children = tabs["children"].as_array().expect("still a tab list");
        assert_eq!(children.len(), 1, "the ghost tab is gone");
        assert_eq!(
            children[0]["panel_name"], "DetailsPanel",
            "the real one stays"
        );
        assert_eq!(
            tabs["info"]["tabs"]["active_index"], 0,
            "an index left pointing past the end would show an empty dock"
        );

        // Everything legitimate is untouched — a prune that ate the centre would leave no view.
        assert_eq!(value["center"]["children"][0]["panel_name"], "ViewsPanel");
        assert_eq!(value["left_dock"]["open"], true);
        assert_eq!(value["version"], 3);
    }

    /// A panel the app no longer builds must not survive either: the library turns an unregistered
    /// name into a tab that renders "not registered in PanelRegistry" at the reader.
    #[test]
    fn a_panel_we_no_longer_build_is_dropped_rather_than_shown_as_an_error() {
        let raw = r#"{"version":3,
          "center":{"panel_name":"TabPanel","children":[
            {"panel_name":"TablePanel","children":[],"info":{"panel":null}},
            {"panel_name":"ViewsPanel","children":[],"info":{"panel":null}}],
            "info":{"tabs":{"active_index":0}}}}"#;

        let mut value: serde_json::Value = serde_json::from_str(raw).expect("valid json");
        prune(&mut value);

        let children = value["center"]["children"].as_array().expect("a tab list");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["panel_name"], "ViewsPanel");
    }
}

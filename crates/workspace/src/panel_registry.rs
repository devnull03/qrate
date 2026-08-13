//! Which dock each panel is in, and what it puts in the status bar.
//!
//! gpui_component owns the docking; what it has no notion of is a panel that *moves*, or a bar
//! button that follows one. Both need an answer to "where is DetailsPanel right now?", and this
//! is the only place that has one: `DockItem::remove_panel` takes `&self` and never pops from
//! the `DockItem::Tabs` snapshot it removes from, so the library's own view of a dock's contents
//! is wrong from the first move onwards. Never re-derive a placement from `DockItem`.

use std::sync::Arc;

use gpui::*;
use gpui_component::{
    IconName,
    dock::{DockArea, DockItem, DockPlacement, PanelView},
};

use crate::Workspace;
use crate::panels::{AGENT_META, DETAILS_META};

/// Which status-bar group a panel's button sits in.
#[derive(Copy, Clone, PartialEq)]
pub enum BarSide {
    Left,
    Centre,
    Right,
}

/// The button follows its panel: dock the panel right and its icon moves to the right of the bar,
/// dock it bottom and the icon sits in the middle, under the panel it opens.
pub fn bar_side(placement: DockPlacement) -> BarSide {
    match placement {
        DockPlacement::Right => BarSide::Right,
        DockPlacement::Bottom => BarSide::Centre,
        _ => BarSide::Left,
    }
}

/// What a panel declares about itself, once, next to itself.
pub struct PanelMeta {
    /// Matches `Panel::panel_name` and the name the panel is registered under for layout restore.
    pub name: &'static str,
    pub icon: IconName,
    pub label: &'static str,
    pub default_placement: DockPlacement,
    /// Show the live error/warning counts beside the icon instead of the icon alone.
    pub badge: bool,
}

/// `ProblemsPanel` lives in `diagnostics`, which this crate depends on, so it can't name
/// `PanelMeta` — its declaration sits here instead of beside it.
pub static PROBLEMS_META: PanelMeta = PanelMeta {
    name: "ProblemsPanel",
    icon: IconName::TriangleAlert,
    label: "Problems",
    default_placement: DockPlacement::Bottom,
    badge: true,
};

/// Every dockable panel, in status-bar order within its group. The centre table isn't one: it has
/// no button and nowhere to move to.
pub static PANELS: [&PanelMeta; 3] = [&DETAILS_META, &PROBLEMS_META, &AGENT_META];

pub struct PanelEntry {
    pub meta: &'static PanelMeta,
    pub view: Arc<dyn PanelView>,
    pub placement: DockPlacement,
}

#[derive(Default)]
pub struct PanelRegistry(Vec<PanelEntry>);

impl Global for PanelRegistry {}

impl PanelRegistry {
    pub fn entries(cx: &App) -> &[PanelEntry] {
        cx.try_global::<Self>()
            .map(|this| this.0.as_slice())
            .unwrap_or_default()
    }

    pub fn placement(name: &str, cx: &App) -> Option<DockPlacement> {
        Self::entries(cx)
            .iter()
            .find(|entry| entry.meta.name == name)
            .map(|entry| entry.placement)
    }

    /// Rebuild from what the dock area actually holds. Called after every layout construction:
    /// `DockArea::load` builds fresh panel entities from the saved names, so any entry made
    /// before it would point at an orphan. Only correct immediately after a load or a default
    /// build, while the `DockItem` snapshots still match reality.
    pub fn sync(dock_area: &Entity<DockArea>, cx: &mut App) {
        let mut found: Vec<(DockPlacement, Arc<dyn PanelView>)> = Vec::new();
        {
            let area = dock_area.read(cx);
            for placement in [
                DockPlacement::Left,
                DockPlacement::Right,
                DockPlacement::Bottom,
            ] {
                let dock = match placement {
                    DockPlacement::Left => area.left_dock(),
                    DockPlacement::Right => area.right_dock(),
                    _ => area.bottom_dock(),
                };
                if let Some(dock) = dock {
                    collect(dock.read(cx).panel(), placement, &mut found);
                }
            }
        }

        // Driven by `PANELS` rather than the walk order, so the bar keeps its declared order.
        let entries = PANELS
            .iter()
            .filter_map(|meta| {
                found
                    .iter()
                    .find(|(_, view)| view.panel_name(cx) == meta.name)
                    .map(|(placement, view)| PanelEntry {
                        meta,
                        view: view.clone(),
                        placement: *placement,
                    })
            })
            .collect();
        cx.set_global(Self(entries));
    }

    /// Open or close the dock the panel is currently in.
    pub fn toggle(name: &str, dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
        let Some(placement) = Self::placement(name, cx) else {
            return;
        };
        dock_area.update(cx, |area, cx| area.toggle_dock(placement, window, cx));
        // `Dock::set_open` only notifies, it never emits `LayoutChanged`.
        Workspace::persist_layout(dock_area, cx);
    }

    /// Move a panel to another dock, keeping the panel entity — and so its state — alive. The
    /// library has no `move_panel`; `remove_panel` + `add_panel` hand the same `Arc` across.
    pub fn move_panel(
        name: &str,
        to: DockPlacement,
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((view, from)) = Self::entries(cx)
            .iter()
            .find(|entry| entry.meta.name == name)
            .map(|entry| (entry.view.clone(), entry.placement))
        else {
            return;
        };
        if from == to {
            return;
        }

        dock_area.update(cx, |area, cx| {
            area.remove_panel(view.clone(), from, window, cx);
            area.add_panel(view, to, None, window, cx);
        });
        cx.update_global::<Self, _>(|this, _| {
            if let Some(entry) = this.0.iter_mut().find(|entry| entry.meta.name == name) {
                entry.placement = to;
            }
        });

        let source_empty = !Self::entries(cx)
            .iter()
            .any(|entry| entry.placement == from);
        dock_area.update(cx, |area, cx| {
            // An emptied dock keeps rendering as a bare tab strip, and a closed target dock would
            // swallow the panel we just moved into it.
            if source_empty && area.is_dock_open(from, cx) {
                area.toggle_dock(from, window, cx);
            }
            if !area.is_dock_open(to, cx) {
                area.toggle_dock(to, window, cx);
            }
        });
        Workspace::persist_layout(dock_area, cx);
    }
}

/// Flatten a dock's item tree into the panels it holds. Recursive because a restored layout can
/// come back as a `Split` even though nothing here builds one.
fn collect(
    item: &DockItem,
    placement: DockPlacement,
    out: &mut Vec<(DockPlacement, Arc<dyn PanelView>)>,
) {
    match item {
        DockItem::Tabs { items, .. } => {
            out.extend(items.iter().map(|view| (placement, view.clone())))
        }
        DockItem::Panel { view, .. } => out.push((placement, view.clone())),
        DockItem::Split { items, .. } => {
            for item in items {
                collect(item, placement, out);
            }
        }
        DockItem::Tiles { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use std::sync::Arc;

    use gpui::{AppContext as _, TestAppContext};
    use gpui_component::dock::{DockArea, DockItem, DockPlacement, PanelView};

    use crate::panel_registry::PanelRegistry;
    use crate::panels::{AgentPanel, DetailsPanel};
    use diagnostics::ProblemsPanel;

    /// The registry is the only record of where a panel is: the library's `DockItem` snapshot
    /// goes stale the moment anything moves. So this covers both halves — that `sync` reads the
    /// real arrangement, and that a move updates it and takes the emptied dock down with it.
    #[gpui::test]
    fn a_move_updates_the_placement_and_closes_the_dock_it_emptied(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::SettingsPersistence::default());
        });

        let (dock_area, cx) =
            cx.add_window_view(|window, cx| DockArea::new("test", None, window, cx));

        cx.update(|window, cx| {
            dock_area.update(cx, |area, cx| {
                let weak = cx.entity().downgrade();
                let details: Arc<dyn PanelView> =
                    Arc::new(cx.new(|cx| DetailsPanel::new(window, cx)));
                let agent: Arc<dyn PanelView> = Arc::new(cx.new(|cx| AgentPanel::new(window, cx)));
                let problems: Arc<dyn PanelView> =
                    Arc::new(cx.new(|cx| ProblemsPanel::new(window, cx)));
                let tabs = |panel, cx: &mut gpui::App, window: &mut gpui::Window| {
                    DockItem::tabs(vec![panel], &weak, window, cx)
                };
                let left = tabs(details, cx, window);
                area.set_left_dock(left, None, true, window, cx);
                let right = tabs(agent, cx, window);
                area.set_right_dock(right, None, true, window, cx);
                let bottom = tabs(problems, cx, window);
                area.set_bottom_dock(bottom, None, true, window, cx);
            });
            PanelRegistry::sync(&dock_area, cx);
        });

        cx.update(|_window, cx| {
            assert_eq!(
                PanelRegistry::placement("DetailsPanel", cx),
                Some(DockPlacement::Left)
            );
            assert_eq!(
                PanelRegistry::placement("ProblemsPanel", cx),
                Some(DockPlacement::Bottom)
            );
        });

        cx.update(|window, cx| {
            PanelRegistry::move_panel("DetailsPanel", DockPlacement::Right, &dock_area, window, cx);
        });

        cx.update(|_window, cx| {
            assert_eq!(
                PanelRegistry::placement("DetailsPanel", cx),
                Some(DockPlacement::Right)
            );
            let area = dock_area.read(cx);
            assert!(!area.is_dock_open(DockPlacement::Left, cx), "left emptied");
            assert!(area.is_dock_open(DockPlacement::Right, cx), "right open");
        });
    }
}

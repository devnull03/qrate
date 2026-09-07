//! Which dock each panel is in, and what it puts in the status bar.
//!
//! The dock owns the arrangement; what it has no notion of is a bar button that follows a panel
//! around. That needs an answer to "where is DetailsPanel right now?", and this is where it
//! lives — alongside each panel's icon, label and bar side.
//!
//! Unlike the `DockItem` model this replaced, the placement here is a *cache* of something the
//! dock already knows: a `PaneTree` is the single source of truth for what a dock holds, and
//! [`PanelRegistry::sync`] re-reads it rather than tracking moves by hand.

use std::{collections::HashMap, sync::Arc};

use gpui::*;
use gpui_component::{
    IconName,
    dock::{
        BasePanelView, DockArea, DockEvent, DockLayout, DockPlacement, InsertTarget, NodeId,
        PaneRef,
    },
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
    pub view: Arc<dyn BasePanelView>,
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

    /// Whether this panel is actually on screen — its dock is open *and* it is the tab in front.
    /// "Dock is open" alone lights a button up for a panel hidden behind its neighbour, which
    /// points the reader at something they cannot see.
    ///
    pub fn visible(name: &str, dock_area: &Entity<DockArea>, cx: &App) -> bool {
        let Some(placement) = Self::placement(name, cx) else {
            return false;
        };
        let area = dock_area.read(cx);
        if !area.is_dock_open(placement) {
            return false;
        }
        let mut front = Vec::new();
        frontmost(area, placement, cx, &mut front);
        front.contains(&name)
    }

    /// Rebuild from what the dock area actually holds. Called after every layout construction:
    /// `DockArea::load` builds fresh panel entities from the saved names, so any entry made
    /// before it would point at an orphan.
    pub fn sync(dock_area: &Entity<DockArea>, cx: &mut App) {
        let mut found: Vec<(DockPlacement, Arc<dyn BasePanelView>)> = Vec::new();
        {
            let area = dock_area.read(cx);
            for placement in [
                DockPlacement::Left,
                DockPlacement::Right,
                DockPlacement::Bottom,
            ] {
                collect(area, placement, &mut found);
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

    /// Keep qrate's tool panels in one tab group per edge dock.
    ///
    /// gpui-kit exposes only a whole-layout drag lock. qrate keeps dragging enabled, then folds a
    /// split drop back into tabs and returns a centre drop to the panel's previous edge.
    pub fn enforce_edge_tabs(dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
        let previous: HashMap<&str, DockPlacement> = Self::entries(cx)
            .iter()
            .map(|entry| (entry.meta.name, entry.placement))
            .collect();
        let centre_moves = {
            let area = dock_area.read(cx);
            area.layout(DockPlacement::Center)
                .into_iter()
                .flat_map(|tree| tree.panels())
                .filter_map(|id| {
                    let name = area.panel(id)?.panel_name(cx);
                    let meta = PANELS.iter().find(|meta| meta.name == name)?;
                    let destination = previous
                        .get(name)
                        .copied()
                        .filter(|placement| *placement != DockPlacement::Center)
                        .unwrap_or(meta.default_placement);
                    Some((id, destination))
                })
                .collect::<Vec<_>>()
        };

        dock_area.update(cx, |area, cx| {
            for (panel, destination) in centre_moves {
                if tabs_node(area, destination).is_none() {
                    area.set_dock(destination, DockLayout::tabs(), window, cx);
                }
                if let Some(node) = tabs_node(area, destination) {
                    area.move_panel(
                        panel,
                        InsertTarget::Tabs {
                            node,
                            ix: None,
                            activate: true,
                        },
                        window,
                        cx,
                    );
                }
            }

            for placement in [
                DockPlacement::Left,
                DockPlacement::Right,
                DockPlacement::Bottom,
            ] {
                let Some(tree) = area.layout(placement) else {
                    continue;
                };
                if matches!(tree.root().kind(), PaneRef::Tabs { .. }) {
                    continue;
                }
                let Some(target) = tabs_node(area, placement) else {
                    continue;
                };
                let panels = tree
                    .panels()
                    .filter(|panel| tree.find_panel_node(*panel) != Some(target))
                    .collect::<Vec<_>>();
                for panel in panels {
                    area.move_panel(
                        panel,
                        InsertTarget::Tabs {
                            node: target,
                            ix: None,
                            activate: true,
                        },
                        window,
                        cx,
                    );
                }
            }
        });
    }

    /// Show a panel, or put it away if it is already the one in front.
    ///
    /// "In front", not "its dock is open": where two panels share a dock, clicking the hidden
    /// one's button used to shut the dock on the panel the reader was looking at, when what they
    /// asked for was to see theirs.
    pub fn toggle(name: &str, dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
        let Some(placement) = Self::placement(name, cx) else {
            return;
        };
        let toggled = if Self::visible(name, dock_area, cx) {
            dock_area.update(cx, |area, cx| {
                crate::toggle_dock_immediately(area, placement, window, cx)
            })
        } else {
            let toggled = if !dock_area.read(cx).is_dock_open(placement) {
                dock_area.update(cx, |area, cx| {
                    crate::toggle_dock_immediately(area, placement, window, cx)
                })
            } else {
                false
            };
            Self::bring_to_front(name, placement, dock_area, window, cx);
            toggled
        };
        // Revealing another tab changes the layout without opening or closing its dock.
        if !toggled {
            dock_area.update(cx, |_, cx| cx.emit(DockEvent::LayoutChanged));
        }
    }

    /// Make `name` the tab in front of its dock.
    ///
    /// A move onto the panel's own group at its own index: the tree edit is a no-op, so the tab
    /// order is untouched and `activate` is the whole point. There is still no public "select
    /// this tab" on `DockArea` — `TabGroup::select_tab` needs the group entity, which the area
    /// keeps to itself.
    fn bring_to_front(
        name: &str,
        placement: DockPlacement,
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((node, ix, panel)) = ({
            let area = dock_area.read(cx);
            area.layout(placement).and_then(|tree| {
                let mut found = None;
                tree.root().walk(&mut |node| {
                    let PaneRef::Tabs { panels, .. } = node.kind() else {
                        return;
                    };
                    if found.is_some() {
                        return;
                    }
                    found = panels
                        .iter()
                        .position(|id| {
                            area.panel(*id)
                                .is_some_and(|panel| panel.panel_name(cx) == name)
                        })
                        .map(|ix| (node.id(), ix, panels[ix]));
                });
                found
            })
        }) else {
            return;
        };
        dock_area.update(cx, |area, cx| {
            area.move_panel(
                panel,
                InsertTarget::Tabs {
                    node,
                    ix: Some(ix),
                    activate: true,
                },
                window,
                cx,
            );
        });
    }

    /// Move a panel to another dock, keeping the panel entity — and so its state — alive.
    /// `DockArea::move_panel` is built for exactly this: the panel never leaves the dock, so it
    /// is never told it was removed, and its active state carries across.
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

        let panel = view.panel_id(cx);
        dock_area.update(cx, |area, cx| {
            // A dock that was never built, or that a previous move emptied, has no group to
            // merge into. Give it one first: `move_panel` wants a node to aim at, and unlike
            // `add_panel_view` it detaches the panel from wherever it currently sits.
            if tabs_node(area, to).is_none() {
                area.set_dock(to, DockLayout::tabs(), window, cx);
            }
            let Some(node) = tabs_node(area, to) else {
                return;
            };
            area.move_panel(
                panel,
                InsertTarget::Tabs {
                    node,
                    ix: None,
                    activate: true,
                },
                window,
                cx,
            );
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
            if source_empty && area.is_dock_open(from) {
                crate::toggle_dock_immediately(area, from, window, cx);
            }
            if !area.is_dock_open(to) {
                crate::toggle_dock_immediately(area, to, window, cx);
            }
        });
        Workspace::persist_layout(dock_area, cx);
    }
}

/// The panels a dock is actually showing — one per tab group, since a split shows several at once.
fn frontmost(area: &DockArea, placement: DockPlacement, cx: &App, out: &mut Vec<&'static str>) {
    let Some(tree) = area.layout(placement) else {
        return;
    };
    tree.root().walk(&mut |node| {
        let PaneRef::Tabs { panels, active_ix } = node.kind() else {
            return;
        };
        out.extend(
            panels
                .get(active_ix)
                .and_then(|id| area.panel(*id))
                .map(|panel| panel.panel_name(cx)),
        );
    });
}

/// The panels a dock holds, in tree order. A restored layout can come back as a split even
/// though nothing here builds one, so this asks the tree rather than assuming one group.
fn collect(
    area: &DockArea,
    placement: DockPlacement,
    out: &mut Vec<(DockPlacement, Arc<dyn BasePanelView>)>,
) {
    let Some(tree) = area.layout(placement) else {
        return;
    };
    out.extend(
        tree.panels()
            .filter_map(|id| area.panel(id).map(|panel| (placement, panel.clone()))),
    );
}

/// The first tab group in a dock, which is where a panel moved there belongs.
fn tabs_node(area: &DockArea, placement: DockPlacement) -> Option<NodeId> {
    let tree = area.layout(placement)?;
    let mut found = None;
    tree.root().walk(&mut |node| {
        if found.is_none() && matches!(node.kind(), PaneRef::Tabs { .. }) {
            found = Some(node.id());
        }
    });
    found
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use std::sync::Arc;

    use gpui::{AppContext as _, TestAppContext};
    use gpui_component::dock::{BasePanelView, DockArea, DockLayout, DockPlacement, panel_handle};

    use crate::panel_registry::PanelRegistry;
    use crate::panels::{AgentPanel, DetailsPanel};
    use diagnostics::ProblemsPanel;

    /// Covers both halves of the registry — that `sync` reads the real arrangement out of the
    /// pane tree, and that a move updates it and takes the emptied dock down with it.
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
                let details: Arc<dyn BasePanelView> =
                    panel_handle(cx.new(|cx| DetailsPanel::new(window, cx)));
                let agent: Arc<dyn BasePanelView> =
                    panel_handle(cx.new(|cx| AgentPanel::new(window, cx)));
                let problems: Arc<dyn BasePanelView> =
                    panel_handle(cx.new(|cx| ProblemsPanel::new(window, cx)));
                for (placement, panel) in [
                    (DockPlacement::Left, details),
                    (DockPlacement::Right, agent),
                    (DockPlacement::Bottom, problems),
                ] {
                    area.set_dock(
                        placement,
                        DockLayout::tabs().panel_view(panel, cx),
                        window,
                        cx,
                    );
                }
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
            assert!(!area.is_dock_open(DockPlacement::Left), "left emptied");
            assert!(area.is_dock_open(DockPlacement::Right), "right open");
        });
    }
}

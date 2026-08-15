//! The centre's view system: which body the main area draws, the switch that changes it, and the
//! per-view dock arrangement.
//!
//! It lives here rather than in `table` because a view mode is a *workspace* concern — it decides
//! what occupies the dock centre and where the side panels sit. `table` stays the grid and nothing
//! more. [`ViewsPanel`] is the dock's centre panel; the grid is one child it can mount.
//!
//! The load-bearing rule: every view renders the same `TableState<QrateTableDelegate>` and moves
//! the same selection cursor, reached through the `TableStateHandle` global exactly as the Details
//! panel reaches it. No view gets its own copy of the data or its own selection.

mod gallery;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::table::TableState;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::Button,
    dock::{DockArea, Panel, PanelControl, PanelEvent},
    h_flex,
    slider::{Slider, SliderEvent, SliderState},
    tab::{Tab, TabBar},
    v_flex,
};
use table::{QrateTableDelegate, TableChanged, TablePanel, TableStateHandle};

use crate::ViewerScope;

/// Settings key for the active view, in either scope.
pub const VIEW_MODE_KEY: &str = "view_mode";

/// Key context of the centre panel, for the keys that act on whatever view is up — Escape to drop
/// the selection. The grid has its own deeper context, so a key bound to both means the grid's.
pub const VIEWS_CONTEXT: &str = "ViewsPanel";

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug, serde::Deserialize, schemars::JsonSchema)]
pub enum ViewMode {
    #[default]
    Table,
    Gallery,
}

/// Show one view. Carries the mode rather than there being an action per view, so adding a view
/// is still one `ViewMode::ALL` entry and nothing else — `app` binds a key and a menu item per
/// mode from that same list.
#[derive(Clone, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = qrate)]
#[serde(deny_unknown_fields)]
pub struct ShowView {
    pub mode: ViewMode,
}

/// The live centre panel, so a menu item or a shortcut can switch views from anywhere in the
/// window — actions dispatch up the *focus* chain, which doesn't pass through the centre when
/// focus is in a side panel. Republished whenever a restored layout rebuilds the panel, the same
/// contract as `table::TableStateHandle`.
pub struct ActiveViewsPanel(pub WeakEntity<ViewsPanel>);
impl Global for ActiveViewsPanel {}

/// Switch the centre to `mode` from outside it — the View menu and its shortcuts. No-op before
/// the workspace has built a centre panel.
pub fn show_view(mode: ViewMode, window: &mut Window, cx: &mut App) {
    let Some(panel) = cx
        .try_global::<ActiveViewsPanel>()
        .and_then(|p| p.0.upgrade())
    else {
        return;
    };
    switch(&panel, mode, window, cx);
}

impl ViewMode {
    /// Switcher order. Adding a view is an entry here, a `match` arm in [`ViewsPanel::render`],
    /// and a default arrangement in [`default_layout`] — nothing else.
    pub const ALL: [Self; 2] = [Self::Table, Self::Gallery];

    /// The persisted spelling, kept apart from [`label`](Self::label) so renaming the button
    /// can't strand everyone's saved setting.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Gallery => "gallery",
        }
    }

    /// Anything unrecognized reads as the grid, which is also what a project with no setting gets.
    pub fn parse(text: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == text)
            .unwrap_or_default()
    }

    /// The bundled icon set has no grid or table glyph, so these are the nearest pair that reads
    /// as "rows" against "tiles".
    fn icon(self) -> IconName {
        match self {
            Self::Table => IconName::Menu,
            Self::Gallery => IconName::LayoutDashboard,
        }
    }

    /// The switcher tab's text, and the View menu's item name — one spelling for both.
    pub fn label(self) -> &'static str {
        match self {
            Self::Table => "Table",
            Self::Gallery => "Gallery",
        }
    }
}

/// The dock's centre: the view switcher over whichever view is active.
///
/// Holds the grid as an owned entity rather than rebuilding it per switch — a fresh `TablePanel`
/// would mean a fresh `TableState`, losing the undo history, any unsaved edits and the selection.
/// The grid is constructed once and simply not rendered while another view is up.
pub struct ViewsPanel {
    focus_handle: FocusHandle,
    table: Entity<TablePanel>,
    dock_area: WeakEntity<DockArea>,
    view: ViewMode,
    /// The body's measured width, for the gallery's column count. Written by the canvas below.
    body_width: Pixels,
    /// Live table state, for the views that read it directly (the gallery). Bound through the
    /// global rather than reaching into `TablePanel`, the same route `DetailsPanel` takes.
    state: Option<WeakEntity<TableState<QrateTableDelegate>>>,
    /// Re-binds `state` when `TablePanel` publishes a new table (project reload).
    _handle_sub: Subscription,
    /// Repaints the active view when the data or the selection changes.
    _changed_sub: Option<Subscription>,
    /// Mounts and unmounts the centre-scoped photo overlay as cards are clicked and dismissed.
    _viewer_sub: Subscription,
    /// Cards per row in the gallery. Owned here rather than in `gallery::render`, which is a free
    /// function with no state of its own to keep between frames.
    thumb: Entity<SliderState>,
    /// Persists the count as it is dragged, and repaints the cards to match.
    _thumb_sub: Subscription,
}

impl ViewsPanel {
    pub fn new(
        dock_area: WeakEntity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let table = cx.new(|cx| TablePanel::new(window, cx));
        let cols = gallery::columns(cx) as f32;
        // `max` before `min`: each setter re-clamps the current value against the *other* bound as
        // it stands, and the default min is 0 — so lowering the ceiling first would trap the value.
        let thumb = cx.new(|_| {
            SliderState::new()
                .max(gallery::COLS_MAX)
                .min(gallery::COLS_MIN)
                .step(1.)
                .default_value(cols)
        });
        let _thumb_sub = cx.subscribe(&thumb, |_this, _slider, event: &SliderEvent, cx| {
            let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
            let text = SharedString::from(format!("{}", value.start().round()));
            if cx.has_global::<settings::project::CurrentProject>() {
                settings::project::CurrentProject::set_text(gallery::COLUMNS_KEY, text, cx);
            } else {
                settings::AppSettings::set_text(gallery::COLUMNS_KEY, text, cx);
            }
            // The slider is drawn in the panel *title*, which the parent `TabPanel` owns and which
            // does not observe this panel — `cx.notify()` alone would repaint the cards while the
            // thumb stayed put under the pointer. Dragging it is one deliberate gesture, so a full
            // redraw is the cheap honest fix, the same trade `switch` makes.
            cx.refresh_windows();
            cx.notify();
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            table,
            dock_area,
            // The project's last view. `TablePanel::new` above has already loaded that project,
            // so the arrangement restored alongside it already matches this.
            view: ViewMode::parse(&settings::effective_text(VIEW_MODE_KEY, cx)),
            body_width: px(0.),
            state: None,
            _handle_sub: cx.observe_global::<TableStateHandle>(|this, cx| {
                this.bind(cx);
                cx.notify();
            }),
            _changed_sub: None,
            _viewer_sub: cx.observe_global::<crate::viewer::ActiveViewer>(|_, cx| cx.notify()),
            thumb,
            _thumb_sub,
        };
        this.bind(cx);
        // Publish before the first render so the View menu works from the moment the window is up.
        cx.set_global(ActiveViewsPanel(cx.entity().downgrade()));
        this
    }

    /// Point `state` at the live table and repaint whenever it changes.
    fn bind(&mut self, cx: &mut Context<Self>) {
        self.state = cx.try_global::<TableStateHandle>().map(|h| h.0.clone());
        self._changed_sub = self
            .state
            .as_ref()
            .and_then(|state| state.upgrade())
            .map(|state| cx.subscribe(&state, |_this, _state, _: &TableChanged, cx| cx.notify()));
    }

    /// The view switcher: one segmented tab per mode, the same control the Settings window's
    /// error/warning switch uses. Labelled as well as iconed — `Tab` has no tooltip, and nothing
    /// in the bundled icon set reads unmistakably as "grid" on its own.
    ///
    /// Returned from [`Panel::title`], which `TabPanel` lays out as the leftmost thing in the
    /// title row (`[title (flex_1)] [title_suffix] [toolbar: Find, ⋯]`) — so it sits inline with
    /// the Find button and the ⋯ menu, at the far left. `small` because that row is 30px tall and
    /// a default segmented tab is 32px, which the title cell would clip.
    fn switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        TabBar::new("view-mode")
            .segmented()
            .small()
            .selected_index(
                ViewMode::ALL
                    .iter()
                    .position(|mode| *mode == self.view)
                    .unwrap_or(0),
            )
            .on_click(move |ix: &usize, window, cx: &mut App| {
                let (Some(this), Some(&mode)) = (this.upgrade(), ViewMode::ALL.get(*ix)) else {
                    return;
                };
                switch(&this, mode, window, cx);
            })
            .children(
                ViewMode::ALL
                    .into_iter()
                    .map(|mode| Tab::new().icon(mode.icon()).label(mode.label())),
            )
    }
}

/// Show `mode`: remember it, re-arrange the docks for it, repaint. A no-op when it is already the
/// active view, so a stray click can't churn the saved arrangements.
///
/// A free function taking the entity rather than a `&mut self` method, and that is load-bearing.
/// Every dock mutation ends in `Workspace::persist_layout` → `DockArea::dump`, and `dump` reads
/// *every* panel in the dock — this one included. Run any of it inside `Entity::update` and the
/// panel is still mutably leased when `dump` tries to read it, which is a double-lease panic
/// inside a Windows window proc: a non-unwinding abort, not a catchable error. So the panel's own
/// state changes under one short lease that is released before anything touches the dock.
fn switch(panel: &Entity<ViewsPanel>, mode: ViewMode, window: &mut Window, cx: &mut App) {
    let Some((leaving, dock_area)) = panel.update(cx, |this, cx| {
        (this.view != mode).then(|| {
            let leaving = this.view;
            this.view = mode;
            cx.notify();
            (leaving, this.dock_area.clone())
        })
    }) else {
        return;
    };

    // Nothing below holds a lease on `panel`.
    //
    // A photo opened from a card belongs to the gallery; leaving it up would cover whatever the
    // next view draws.
    if crate::viewer::viewer_in(ViewerScope::Centre, cx).is_some() {
        crate::viewer::close_viewer(cx);
    }
    let text = SharedString::from(mode.as_str());
    if cx.has_global::<settings::project::CurrentProject>() {
        settings::project::CurrentProject::set_text(VIEW_MODE_KEY, text, cx);
    } else {
        settings::AppSettings::set_text(VIEW_MODE_KEY, text, cx);
    }
    if let Some(area) = dock_area.upgrade() {
        // Snapshot where the panels sit before moving them, as the view being left.
        layout::capture(leaving, &area, cx);
        layout::apply(mode, &area, window, cx);
    }
    // The switcher is drawn by the parent `TabPanel` (via `Panel::title`), which does not observe
    // this panel — `cx.notify()` above repaints the body but leaves the highlight on the old tab.
    // Switching views is a rare, deliberate click, so a full redraw is the cheap honest fix.
    window.refresh();
}

impl Focusable for ViewsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for ViewsPanel {}

impl Panel for ViewsPanel {
    fn panel_name(&self) -> &'static str {
        "ViewsPanel"
    }

    /// The centre has no name worth showing, so the title cell carries the view switcher instead —
    /// which puts it at the far left of the same row as the Find button and the ⋯ menu.
    ///
    /// Wrapped in an `h_flex`, which is what keeps the switcher the width of its own tabs. The
    /// title cell is a plain `div` and gpui's default display is *block*, so a bare `TabBar` fills
    /// it — painting the segmented background all the way across to the Find icon.
    ///
    /// The gallery's density control rides here rather than in a strip of its own above the cards:
    /// it belongs to the same row as everything else that says what the centre is showing, and it
    /// costs the cards no height. `toolbar_buttons` can't hold it — the library restricts that side
    /// to `Button`s — so it is pushed right by a spacer instead, landing beside the ⋯ menu.
    fn title(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().w_full().gap_2().child(self.switcher(cx)).when(
            self.view == ViewMode::Gallery,
            |title| {
                title
                    .child(div().flex_1())
                    .child(
                        Icon::new(IconName::LayoutDashboard)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().w(px(96.)).child(Slider::new(&self.thumb)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} per row", gallery::columns(cx))),
                    )
            },
        )
    }

    /// Centre panel: not closable, so the main view always keeps its body.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    /// A zoom control makes no sense for the main body. Note this *does* render: `zoomable: None`
    /// only greys the ⋯ menu's "Zoom In" entry and drops the zoom toolbar button — the ⋯ itself
    /// is unconditional in `TabPanel::render_toolbar`.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// Rendered by `TabPanel::render_toolbar` immediately left of the ⋯ menu, forced to
    /// `.xsmall().ghost()` by the library. Only the grid has anything to find in.
    fn toolbar_buttons(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> Option<Vec<Button>> {
        if self.view != ViewMode::Table {
            return None;
        }
        // Clicked from the title bar, which is outside this panel's focus — so drive the grid
        // through its entity rather than dispatching the `Search` action.
        let table = self.table.downgrade();
        Some(vec![
            Button::new("table-search")
                .icon(IconName::Search)
                .tooltip("Find in table")
                .on_click(move |_, window, cx| {
                    if let Some(table) = table.upgrade() {
                        table.update(cx, |table, cx| table.toggle_search(window, cx));
                    }
                }),
        ])
    }
}

impl Render for ViewsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        v_flex()
            .size_full()
            .key_context(VIEWS_CONTEXT)
            .track_focus(&self.focus_handle)
            .id("views-panel")
            .role(Role::Group)
            .aria_label("Views")
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // The gallery needs to know how wide it is to pick a column count; nothing
                    // else measures this panel. Guarded so writing the width can't re-trigger
                    // itself into a repaint loop.
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                let Some(this) = this.upgrade() else { return };
                                this.update(cx, |this, cx| {
                                    if this.body_width != bounds.size.width {
                                        this.body_width = bounds.size.width;
                                        cx.notify();
                                    }
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    // Every view draws the same table state. Erased to `AnyElement` because two
                    // chained-builder types meeting in one `match` overflows rustc's stack.
                    .child(match self.view {
                        ViewMode::Table => self.table.clone().into_any_element(),
                        ViewMode::Gallery => gallery::render(
                            self.state.as_ref().and_then(WeakEntity::upgrade),
                            self.body_width,
                            &self.focus_handle,
                            cx,
                        ),
                    })
                    // The clicked card's photo, over the grid of thumbnails but under nothing
                    // else — the docked panels stay put, which is the whole point of scoping it
                    // here instead of over the workspace. Escape or the ✕ puts the cards back.
                    .children(crate::viewer::viewer_in(ViewerScope::Centre, cx)),
            )
    }
}

/// Per-view dock arrangements: which dock each panel sits in for this view, and whether that dock
/// is open.
///
/// Deliberately *not* a `DockAreaState` snapshot replayed through `DockArea::load`: `load`
/// reconstructs every panel from its registered name, which would build a fresh `ViewsPanel` and
/// with it a fresh `TablePanel` and `TableState` — throwing away the undo history, any unsaved
/// edits and the selection on every switch. `move_panel`/`toggle_dock` re-arrange the panels that
/// already exist, so nothing is rebuilt.
///
/// ponytail: placements and open/closed only, no dock sizes — those live in `DockAreaState` and
/// already persist only at quit. Widen this if per-view widths ever matter.
mod layout {
    use gpui::{App, Entity, Window};
    use gpui_component::dock::{DockArea, DockPlacement};
    use serde::{Deserialize, Serialize};

    use super::ViewMode;
    use crate::panel_registry::{PANELS, PanelRegistry};

    /// Settings key holding every view's arrangement as one JSON object, keyed by
    /// [`ViewMode::as_str`].
    const VIEW_LAYOUTS_KEY: &str = "view_layouts";

    /// Where one panel sits in one view. A flat list rather than two maps keyed by panel name:
    /// `PANELS` has three entries, and one record per panel can't disagree with itself the way
    /// two parallel maps could. Written in `PANELS` order, so the JSON is deterministic.
    #[derive(Serialize, Deserialize)]
    struct PanelSlot {
        name: String,
        placement: String,
        open: bool,
    }

    /// `DockPlacement` is the library's type and carries no serde impl we control, so the two
    /// conversions are spelled out here. An unknown string reads as the panel's own default.
    fn placement_name(placement: DockPlacement) -> &'static str {
        match placement {
            DockPlacement::Left => "left",
            DockPlacement::Right => "right",
            DockPlacement::Bottom => "bottom",
            DockPlacement::Center => "center",
        }
    }

    fn parse_placement(text: &str, fallback: DockPlacement) -> DockPlacement {
        match text {
            "left" => DockPlacement::Left,
            "right" => DockPlacement::Right,
            "bottom" => DockPlacement::Bottom,
            _ => fallback,
        }
    }

    /// What a view looks like the first time it is entered. The grid keeps whatever arrangement
    /// the user already had; the gallery wants the row's details beside the thumbnails and has no
    /// use for the problems list while browsing.
    fn default_layout(mode: ViewMode) -> Option<Vec<PanelSlot>> {
        match mode {
            ViewMode::Table => None,
            ViewMode::Gallery => Some(vec![
                PanelSlot {
                    name: "DetailsPanel".into(),
                    placement: "right".into(),
                    open: true,
                },
                PanelSlot {
                    name: "ProblemsPanel".into(),
                    placement: "bottom".into(),
                    open: false,
                },
            ]),
        }
    }

    fn read_all(cx: &App) -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(&settings::effective_text(VIEW_LAYOUTS_KEY, cx)).unwrap_or_default()
    }

    fn write_all(all: serde_json::Map<String, serde_json::Value>, cx: &mut App) {
        let Ok(json) = serde_json::to_string(&all) else {
            return;
        };
        let json = gpui::SharedString::from(json);
        if cx.has_global::<settings::project::CurrentProject>() {
            settings::project::CurrentProject::set_text(VIEW_LAYOUTS_KEY, json, cx);
        } else {
            settings::AppSettings::set_text(VIEW_LAYOUTS_KEY, json, cx);
        }
    }

    /// Record where the panels sit right now as `mode`'s arrangement.
    pub(super) fn capture(mode: ViewMode, dock_area: &Entity<DockArea>, cx: &mut App) {
        let area = dock_area.read(cx);
        let slots: Vec<PanelSlot> = PANELS
            .iter()
            .filter_map(|meta| {
                let placement = PanelRegistry::placement(meta.name, cx)?;
                Some(PanelSlot {
                    name: meta.name.to_string(),
                    placement: placement_name(placement).to_string(),
                    open: area.is_dock_open(placement, cx),
                })
            })
            .collect();
        let Ok(value) = serde_json::to_value(&slots) else {
            return;
        };
        let mut all = read_all(cx);
        all.insert(mode.as_str().to_string(), value);
        write_all(all, cx);
    }

    /// Put the panels where `mode` last had them, or where its default says. Nothing saved and no
    /// default means leave the arrangement alone — the grid's case.
    pub(super) fn apply(
        mode: ViewMode,
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let saved = read_all(cx)
            .remove(mode.as_str())
            .and_then(|value| serde_json::from_value::<Vec<PanelSlot>>(value).ok());
        let Some(slots) = saved.or_else(|| default_layout(mode)) else {
            return;
        };

        for slot in &slots {
            let Some(meta) = PANELS.iter().find(|meta| meta.name == slot.name) else {
                log::warn!("saved view layout names an unknown panel {}", slot.name);
                continue;
            };
            let to = parse_placement(&slot.placement, meta.default_placement);
            // Moving already opens the target dock and closes an emptied source, so ask about the
            // open state only once the panel is where it belongs.
            PanelRegistry::move_panel(meta.name, to, dock_area, window, cx);
            let open = dock_area.read(cx).is_dock_open(to, cx);
            if open != slot.open {
                dock_area.update(cx, |area, cx| area.toggle_dock(to, window, cx));
            }
        }
        crate::Workspace::persist_layout(dock_area, cx);
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would shadow `#[test]`.
    use std::sync::Arc;

    use gpui::{AppContext as _, TestAppContext};
    use gpui_component::dock::{DockArea, DockItem, DockPlacement, PanelView};

    use crate::panel_registry::PanelRegistry;
    use crate::views::{ViewMode, ViewsPanel};

    /// The persisted spelling has to survive a round trip, and an unknown one has to degrade to
    /// the grid rather than leaving the centre blank.
    #[test]
    fn every_mode_round_trips_and_junk_falls_back_to_the_grid() {
        for mode in ViewMode::ALL {
            assert_eq!(ViewMode::parse(mode.as_str()), mode);
        }
        assert_eq!(ViewMode::parse("kanban"), ViewMode::Table);
        assert_eq!(ViewMode::parse(""), ViewMode::Table);
    }

    /// Switching views re-arranges the docks, and every dock mutation ends in `DockArea::dump`,
    /// which reads *every* panel — including the centre one driving the switch. Doing that while
    /// the centre panel is still mutably leased is a double-lease panic, and because it unwinds
    /// through a Windows window proc it aborts the process rather than failing a frame. It
    /// compiles clean and passes every unit test that doesn't own a real dock, so this builds one:
    /// a centre `ViewsPanel` plus a Details panel for the Gallery arrangement to actually move.
    #[gpui::test]
    fn switching_views_rearranges_the_docks_without_double_leasing_the_centre(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::SettingsPersistence::default());
            cx.set_global(diagnostics::Diagnostics::default());
        });

        let (dock_area, cx) =
            cx.add_window_view(|window, cx| DockArea::new("views-test", None, window, cx));

        let panel = cx.update(|window, cx| {
            let weak = dock_area.downgrade();
            let panel = cx.new(|cx| ViewsPanel::new(weak.clone(), window, cx));
            dock_area.update(cx, |area, cx| {
                let centre: Arc<dyn PanelView> = Arc::new(panel.clone());
                area.set_center(DockItem::panel(centre), window, cx);
                let details: Arc<dyn PanelView> =
                    Arc::new(cx.new(|cx| crate::panels::DetailsPanel::new(window, cx)));
                let left = DockItem::tabs(vec![details], &weak, window, cx);
                area.set_left_dock(left, None, true, window, cx);
            });
            PanelRegistry::sync(&dock_area, cx);
            panel
        });

        assert_eq!(panel.read_with(cx, |panel, _| panel.view), ViewMode::Table);

        // The line that aborted: Gallery's default arrangement moves Details left → right, and
        // that move persists the layout, which dumps the dock, which reads this very panel.
        cx.update(|window, cx| super::switch(&panel, ViewMode::Gallery, window, cx));

        assert_eq!(
            panel.read_with(cx, |panel, _| panel.view),
            ViewMode::Gallery
        );
        assert_eq!(
            cx.update(|_, cx| PanelRegistry::placement("DetailsPanel", cx)),
            Some(DockPlacement::Right),
            "Gallery's default arrangement puts Details on the right"
        );
    }
}

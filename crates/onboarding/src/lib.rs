//! Getting started: a small card of at most three tasks chosen from what the project was made
//! from, and one-time tips pinned to the part of the window they explain.
//!
//! Nothing here dims or blocks the grid, and nothing takes focus on arrival. The card floats in
//! the centre's bottom-right corner; when the centre is too small for it to sit there without
//! covering the grid, or when the archivist says "Show me later", it folds into a label in the
//! status bar that opens upward. Tips are shown once each, app-wide, the first time their subject
//! appears; "Show me" on a task can bring one back.
//!
//! Progress is read, not reported: most tasks tick from the grid's own state
//! ([`table::guide_facts`]), so a task done before it was suggested still counts. The two that
//! can't be read — reviewing a finding and visiting Gallery — are told to the guide by the code
//! that does them ([`note_finding_reviewed`], [`note_view`]).
//!
//! State lives in the project (`settings::onboarding`), so progress follows the `.qrate` file. A
//! project without it predates the guide and gets no card until Help ▸ Getting Started asks.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::DockArea;
use gpui_component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use settings::onboarding::{
    GUIDE_DONE_KEY, GUIDE_KEY, GUIDE_KIND_KEY, GUIDE_OPEN, GuideKind, TIPS_SEEN_KEY,
};

use workspace::{DETAILS_META, PROBLEMS_META, PanelRegistry, ViewMode, WorkspaceExtension};

actions!(
    qrate,
    [
        /// Help ▸ Getting Started: bring the card back with its progress, or switch it on for a
        /// project that predates it. `app` handles it, since with no project open it opens the
        /// launcher instead.
        ShowGettingStarted
    ]
);

/// Below this the floating card would cover the grid it is teaching, so the guide falls back to
/// the status-bar label. Measured on the centre, not the window: docks eat into it.
const COMPACT_WIDTH: Pixels = px(640.);
const COMPACT_HEIGHT: Pixels = px(360.);
const CARD_WIDTH: Pixels = px(292.);
const POPOVER_WIDTH: Pixels = px(300.);
const TIP_WIDTH: Pixels = px(268.);
/// Tips narrow to this when the centre is compact, and stay pinned to their anchor.
const TIP_WIDTH_COMPACT: Pixels = px(220.);
/// How long the "Getting started hidden" note stays up after Dismiss, with its Undo.
const TOAST_FOR: Duration = Duration::from_secs(6);
const BODY: Pixels = px(13.);
const BODY_LINE: Pixels = px(18.);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// No guide for this project: it predates the feature, or no project is open.
    Off,
    Open,
    /// "Show me later": folded into the status-bar label.
    Later,
    Dismissed,
}

impl Mode {
    fn parse(text: &str) -> Self {
        match text {
            GUIDE_OPEN => Self::Open,
            "later" => Self::Later,
            "dismissed" => Self::Dismissed,
            _ => Self::Off,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "",
            Self::Open => GUIDE_OPEN,
            Self::Later => "later",
            Self::Dismissed => "dismissed",
        }
    }

    fn active(self) -> bool {
        matches!(self, Self::Open | Self::Later)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Task {
    NameFirstRecord,
    AddFields,
    AddFilesFolder,
    OpenRow,
    ReviewFindings,
    SelectRow,
    BrowseGallery,
}

const ALL_TASKS: [Task; 7] = [
    Task::NameFirstRecord,
    Task::AddFields,
    Task::AddFilesFolder,
    Task::OpenRow,
    Task::ReviewFindings,
    Task::SelectRow,
    Task::BrowseGallery,
];

fn tasks_for(kind: GuideKind) -> [Task; 3] {
    match kind {
        GuideKind::Blank => [Task::NameFirstRecord, Task::AddFields, Task::AddFilesFolder],
        GuideKind::Import => [Task::OpenRow, Task::ReviewFindings, Task::AddFilesFolder],
        GuideKind::Media => [Task::SelectRow, Task::BrowseGallery, Task::ReviewFindings],
    }
}

impl Task {
    fn key(self) -> &'static str {
        match self {
            Self::NameFirstRecord => "name-first-record",
            Self::AddFields => "add-fields",
            Self::AddFilesFolder => "add-files-folder",
            Self::OpenRow => "open-row",
            Self::ReviewFindings => "review-findings",
            Self::SelectRow => "select-row",
            Self::BrowseGallery => "browse-gallery",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        ALL_TASKS.into_iter().find(|task| task.key() == text)
    }

    fn title(self, compact: bool) -> &'static str {
        match self {
            Self::NameFirstRecord => "Name your first record",
            Self::AddFields => "Add more fields",
            Self::AddFilesFolder => "Add a files folder",
            Self::OpenRow if compact => "Open a row and check your columns",
            Self::OpenRow => "Open a row and check how your columns came across",
            Self::ReviewFindings => "Review what qrate found",
            Self::SelectRow => "Select a row to see its file and fields",
            Self::BrowseGallery => "Browse in Gallery",
        }
    }

    fn hint(self, kind: GuideKind, findings: usize) -> String {
        match self {
            Self::NameFirstRecord => {
                "Type in the first Title cell. The row is already there.".into()
            }
            Self::AddFields => {
                "Add a column for each thing you record, like a date or a creator.".into()
            }
            Self::AddFilesFolder if kind == GuideKind::Blank => {
                "Bring in files or a folder. Each file arrives as its own row.".into()
            }
            Self::AddFilesFolder => "Link the folder that holds your photos and scans.".into(),
            Self::OpenRow => "Select any row. Details shows every field it came in with.".into(),
            Self::ReviewFindings if findings == 0 => {
                "Problems lists anything qrate notices. Nothing so far.".into()
            }
            Self::ReviewFindings => format!(
                "{findings} finding{} in Problems. Reviewing counts; fixing is up to you.",
                if findings == 1 { "" } else { "s" }
            ),
            Self::SelectRow => "Details shows its file above the fields.".into(),
            Self::BrowseGallery => "Every linked file as a thumbnail.".into(),
        }
    }
}

/// A one-time callout pinned to what it explains.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tip {
    TitleCell,
    AddFields,
    Problems,
    Gallery,
    DetailsMoved,
}

impl Tip {
    fn key(self) -> &'static str {
        match self {
            Self::TitleCell => "title-cell",
            Self::AddFields => "add-fields",
            Self::Problems => "problems",
            Self::Gallery => "gallery",
            Self::DetailsMoved => "details-moved",
        }
    }
}

/// The one-time export tip's key, shown from `app` the first time something is exported.
const EXPORT_TIP: &str = "export";

pub struct Guide {
    project: Option<PathBuf>,
    mode: Mode,
    kind: GuideKind,
    done: BTreeSet<Task>,
    facts: table::GuideFacts,
    tip: Option<Tip>,
    /// The dismiss note's generation while it is up, so a timer from an earlier Dismiss can't
    /// take down a newer note.
    toast: Option<u64>,
    toast_generation: u64,
    /// The status-bar label's popover.
    popover: bool,
    centre: Size<Pixels>,
    view: ViewMode,
    dock_area: WeakEntity<DockArea>,
    _project_sub: Subscription,
    _handle_sub: Subscription,
    _diagnostics_sub: Subscription,
    _table_sub: Option<Subscription>,
}

/// The live guide, reached by the centre (card and tips), the status bar (label) and `app`
/// (Help menu, export tip, a revealed finding). Held here rather than by the workspace, which
/// doesn't know it; replaced whenever a main window's workspace is built.
struct ActiveGuide(Entity<Guide>);
impl Global for ActiveGuide {}

fn active(cx: &App) -> Option<Entity<Guide>> {
    cx.try_global::<ActiveGuide>().map(|guide| guide.0.clone())
}

/// Plug the guide into the workspace. Call once at startup, before the main window opens.
pub fn init(cx: &mut App) {
    workspace::extension::register(
        WorkspaceExtension {
            attach: install,
            centre_overlay,
            window_overlay,
            view_changed: note_view,
            centre_resized: set_centre_size,
        },
        cx,
    );
}

fn tips_seen(cx: &App) -> BTreeSet<String> {
    settings::AppSettings::get(cx)
        .values
        .get(TIPS_SEEN_KEY)
        .map(|value| {
            value
                .text()
                .split(',')
                .filter(|key| !key.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn mark_tip_seen(key: &str, cx: &mut App) {
    let mut seen = tips_seen(cx);
    if seen.insert(key.to_string()) {
        let joined = seen.into_iter().collect::<Vec<_>>().join(",");
        settings::AppSettings::set_text(TIPS_SEEN_KEY, joined.into(), cx);
    }
}

impl Guide {
    fn new(dock_area: WeakEntity<DockArea>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            project: None,
            mode: Mode::Off,
            kind: GuideKind::Import,
            done: BTreeSet::new(),
            facts: table::GuideFacts::default(),
            tip: None,
            toast: None,
            toast_generation: 0,
            popover: false,
            centre: Size::default(),
            view: ViewMode::parse(&settings::effective_text(workspace::VIEW_MODE_KEY, cx)),
            dock_area,
            _project_sub: cx.observe_global::<settings::project::CurrentProject>(|this, cx| {
                this.load(cx);
                this.refresh(cx);
            }),
            _handle_sub: cx.observe_global::<table::TableStateHandle>(|this, cx| {
                this.bind(cx);
                this.refresh(cx);
            }),
            _diagnostics_sub: cx.observe_global::<diagnostics::Diagnostics>(|this, cx| {
                this.refresh(cx);
            }),
            _table_sub: None,
        };
        this.load(cx);
        this.bind(cx);
        this.refresh(cx);
        this
    }

    fn bind(&mut self, cx: &mut Context<Self>) {
        self._table_sub = cx
            .try_global::<table::TableStateHandle>()
            .and_then(|handle| handle.0.upgrade())
            .map(|state| {
                cx.subscribe(&state, |this, _, _: &table::TableChanged, cx| {
                    this.refresh(cx)
                })
            });
    }

    /// Re-read the guide's state when the current project changes. `CurrentProject` is written
    /// by every project-scoped setting, this guide's own included, so a same-file reload is
    /// skipped — the in-memory state is already what was written.
    fn load(&mut self, cx: &mut Context<Self>) {
        let project = cx.try_global::<settings::project::CurrentProject>();
        let file = project.map(|project| project.file.clone());
        if file == self.project {
            return;
        }
        self.project = file;
        self.tip = None;
        self.toast = None;
        self.popover = false;
        let text = |key: &str| {
            project
                .and_then(|project| project.data.values.get(key))
                .map(|value| value.text().to_string())
                .unwrap_or_default()
        };
        self.mode = Mode::parse(&text(GUIDE_KEY));
        self.kind = GuideKind::parse(&text(GUIDE_KIND_KEY)).unwrap_or(GuideKind::Import);
        self.done = text(GUIDE_DONE_KEY)
            .split(',')
            .filter_map(Task::parse)
            .collect();
        cx.notify();
    }

    fn tasks(&self) -> [Task; 3] {
        tasks_for(self.kind)
    }

    fn current(&self) -> Option<Task> {
        self.tasks()
            .into_iter()
            .find(|task| !self.done.contains(task))
    }

    fn progress(&self) -> usize {
        self.tasks()
            .iter()
            .filter(|task| self.done.contains(task))
            .count()
    }

    fn compact(&self) -> bool {
        // Unmeasured yet — assume there's room rather than flashing the label on the first frame.
        self.centre.width > px(0.)
            && (self.centre.width < COMPACT_WIDTH || self.centre.height < COMPACT_HEIGHT)
    }

    fn files_folder_set(cx: &App) -> bool {
        cx.try_global::<settings::project::CurrentProject>()
            .and_then(|project| project.data.values.get(settings::project::FILES_FOLDER_KEY))
            .is_some_and(|value| !value.text().trim().is_empty())
    }

    /// Read the grid again, tick whatever it shows is done, and bring up the tip the current task
    /// asks for if it hasn't been shown yet.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let facts = table::guide_facts(cx).unwrap_or_default();
        let changed = facts != self.facts;
        self.facts = facts;
        if !self.mode.active() {
            if changed {
                cx.notify();
            }
            return;
        }
        let files_folder = Self::files_folder_set(cx);
        let reached: Vec<Task> = self
            .tasks()
            .into_iter()
            .filter(|task| match task {
                Task::NameFirstRecord => facts.any_title,
                // A blank project starts with its two required columns.
                Task::AddFields => facts.columns > 2,
                Task::AddFilesFolder => files_folder || facts.linked_files > 0,
                Task::OpenRow | Task::SelectRow => facts.selected,
                Task::BrowseGallery => self.view == ViewMode::Gallery,
                Task::ReviewFindings => false,
            })
            .collect();
        for task in reached {
            self.complete(task, cx);
        }
        if self.tip.is_none()
            && self.mode == Mode::Open
            && !self.compact()
            && let Some(tip) = self.due_tip(cx)
        {
            self.show_tip(tip, cx);
        }
        cx.notify();
    }

    /// The tip the current task wants on its own, if its subject is on screen and it hasn't
    /// been seen.
    fn due_tip(&self, cx: &App) -> Option<Tip> {
        let seen = tips_seen(cx);
        let tip = match self.current()? {
            Task::ReviewFindings
                if !diagnostics::Diagnostics::all(cx).is_empty()
                    && self.dock_area.upgrade().is_some_and(|dock| {
                        PanelRegistry::visible(PROBLEMS_META.name, &dock, cx)
                    }) =>
            {
                Tip::Problems
            }
            Task::BrowseGallery if self.view == ViewMode::Table && self.facts.selected => {
                Tip::Gallery
            }
            _ => return None,
        };
        (!seen.contains(tip.key())).then_some(tip)
    }

    fn complete(&mut self, task: Task, cx: &mut Context<Self>) {
        if !self.mode.active() || !self.done.insert(task) {
            return;
        }
        if self.tip == Some(Tip::Problems) && task == Task::ReviewFindings {
            self.tip = None;
        }
        let joined = self
            .done
            .iter()
            .map(|task| task.key())
            .collect::<Vec<_>>()
            .join(",");
        if cx.has_global::<settings::project::CurrentProject>() {
            settings::project::CurrentProject::set_text(GUIDE_DONE_KEY, joined.into(), cx);
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        if mode != Mode::Open && mode != Mode::Later {
            self.tip = None;
            self.popover = false;
        }
        if cx.has_global::<settings::project::CurrentProject>() {
            settings::project::CurrentProject::set_text(GUIDE_KEY, mode.as_str().into(), cx);
        }
        // The status bar decides its dividers from each item's occupancy when it draws, so a
        // label appearing or leaving has to repaint the bar itself, not just the label.
        cx.refresh_windows();
        cx.notify();
    }

    fn show_tip(&mut self, tip: Tip, cx: &mut Context<Self>) {
        self.tip = Some(tip);
        mark_tip_seen(tip.key(), cx);
        cx.notify();
    }

    fn close_tip(&mut self, cx: &mut Context<Self>) {
        if self.tip.take() == Some(Tip::Problems) {
            // "Looking through the list counts as progress."
            self.complete(Task::ReviewFindings, cx);
        }
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        let finished = self.current().is_none();
        self.set_mode(Mode::Dismissed, cx);
        if finished {
            return;
        }
        self.toast_generation += 1;
        let generation = self.toast_generation;
        self.toast = Some(generation);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            this.update(cx, |this, cx| {
                if this.toast == Some(generation) {
                    this.toast = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn undo_dismiss(&mut self, cx: &mut Context<Self>) {
        self.toast = None;
        self.set_mode(Mode::Open, cx);
    }

    /// Help ▸ Getting Started. A project that predates the guide gets one now, its tasks
    /// guessed from what it holds.
    fn reopen(&mut self, cx: &mut Context<Self>) {
        if self.mode == Mode::Off && cx.has_global::<settings::project::CurrentProject>() {
            self.kind = if Self::files_folder_set(cx) || self.facts.linked_files > 0 {
                GuideKind::Media
            } else if self.facts.rows <= 1 && !self.facts.any_title {
                GuideKind::Blank
            } else {
                GuideKind::Import
            };
            settings::project::CurrentProject::set_text(
                GUIDE_KIND_KEY,
                self.kind.as_str().into(),
                cx,
            );
        }
        self.toast = None;
        self.set_mode(Mode::Open, cx);
        self.popover = self.compact();
        self.refresh(cx);
    }

    fn label_visible(&self) -> bool {
        self.project.is_some()
            && (self.mode == Mode::Later || (self.mode == Mode::Open && self.compact()))
    }

    fn subtitle(&self, cx: &App) -> String {
        let rows = self.facts.rows;
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        match self.kind {
            GuideKind::Blank => {
                let file = cx
                    .try_global::<settings::project::CurrentProject>()
                    .and_then(|project| project.file.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let rows = if self.facts.any_title {
                    plural(rows, "row", "rows")
                } else {
                    plural(rows, "empty row", "empty rows")
                };
                format!("{file} · {rows}")
            }
            GuideKind::Import => format!("{} imported", plural(rows, "row", "rows")),
            GuideKind::Media => format!(
                "{} · {}",
                plural(rows, "row", "rows"),
                plural(self.facts.linked_files, "linked file", "linked files")
            ),
        }
    }
}

/// Build the guide for a new workspace and make it the one every surface reads.
fn install(dock_area: WeakEntity<DockArea>, cx: &mut App) {
    let guide = cx.new(|cx| Guide::new(dock_area, cx));
    // Every change to the guide is a change to what the workspace draws for it.
    cx.observe(&guide, |_, cx| workspace::extension::repaint(cx))
        .detach();
    cx.set_global(ActiveGuide(guide));
}

/// The centre's measured size, from `ViewsPanel`'s canvas.
fn set_centre_size(size: Size<Pixels>, cx: &mut App) {
    let Some(guide) = active(cx) else { return };
    if guide.read(cx).centre == size {
        return;
    }
    guide.update(cx, |guide, cx| {
        let was = guide.compact();
        guide.centre = size;
        if was != guide.compact() {
            guide.popover = false;
            // The label lives in another view; it has to hear the fold too.
            cx.refresh_windows();
        }
        cx.notify();
    });
}

/// The centre switched views. Visiting Gallery ticks its task, and the first visit explains why
/// Details jumped to the other side.
fn note_view(mode: ViewMode, cx: &mut App) {
    let Some(guide) = active(cx) else { return };
    guide.update(cx, |guide, cx| {
        let entering_gallery = mode == ViewMode::Gallery && guide.view != mode;
        guide.view = mode;
        if guide.tip == Some(Tip::Gallery) || guide.tip == Some(Tip::DetailsMoved) {
            guide.tip = None;
        }
        if entering_gallery
            && guide.mode.active()
            && !guide.compact()
            && !tips_seen(cx).contains(Tip::DetailsMoved.key())
            && PanelRegistry::placement(DETAILS_META.name, cx)
                == Some(gpui_component::dock::DockPlacement::Right)
        {
            guide.show_tip(Tip::DetailsMoved, cx);
        }
        guide.refresh(cx);
    });
}

/// A finding was opened from the Problems panel — the "review" half of that task.
pub fn note_finding_reviewed(cx: &mut App) {
    if let Some(guide) = active(cx) {
        guide.update(cx, |guide, cx| guide.complete(Task::ReviewFindings, cx));
    }
}

/// Help ▸ Getting Started. `false` when there's no workspace with a project to show it in, for
/// the caller to open the launcher instead.
pub fn reopen(cx: &mut App) -> bool {
    // The guide outlives its window; a dock area that's gone means there's no workspace to show
    // it in.
    let Some(guide) = active(cx).filter(|guide| guide.read(cx).dock_area.upgrade().is_some())
    else {
        return false;
    };
    if !cx.has_global::<settings::project::CurrentProject>() {
        return false;
    }
    guide.update(cx, |guide, cx| guide.reopen(cx));
    cx.refresh_windows();
    true
}

/// Whether the status bar's Getting started label has anything to show.
pub fn status_label_visible(cx: &App) -> bool {
    active(cx).is_some_and(|guide| guide.read(cx).label_visible())
}

/// The export tip, shown once, the first time anything is exported: every row goes out, filtered
/// or not, and Google Sheets sync is something to switch on first rather than a sign-in here.
pub fn show_export_tip(window: &mut Window, cx: &mut App) {
    if tips_seen(cx).contains(EXPORT_TIP) {
        return;
    }
    mark_tip_seen(EXPORT_TIP, cx);
    let rows = table::guide_facts(cx).map_or(0, |facts| facts.rows);
    use gpui_component::WindowExt as _;
    window.push_notification(
        gpui_component::notification::Notification::new()
            .title("Share or move your catalog")
            .message(format!(
                "Every export includes all {rows} row{}, including rows hidden by a filter. To keep \
                 a Google Sheet in sync, turn it on in Settings ▸ Google first.",
                if rows == 1 { "" } else { "s" }
            ))
            .autohide(false),
        cx,
    );
}

// ---------------------------------------------------------------------------------------------
// Actions behind the buttons. Free functions over the entity, each holding the guide for one
// short update: "Show Gallery" re-arranges the docks, which dumps every panel and ends in
// `note_view` updating this guide again, so no lease may still be out when it runs.

fn focus_centre(guide: &Entity<Guide>, window: &mut Window, cx: &mut App) {
    if let Some(dock) = guide.read(cx).dock_area.upgrade() {
        PanelRegistry::focus_frontmost(
            gpui_component::dock::DockPlacement::Center,
            &dock,
            window,
            cx,
        );
    }
}

fn ensure_visible(guide: &Entity<Guide>, panel: &str, window: &mut Window, cx: &mut App) {
    if let Some(dock) = guide.read(cx).dock_area.upgrade()
        && !PanelRegistry::visible(panel, &dock, cx)
    {
        PanelRegistry::toggle(panel, &dock, window, cx);
    }
}

fn show_me(guide: &Entity<Guide>, task: Task, window: &mut Window, cx: &mut App) {
    guide.update(cx, |guide, cx| {
        guide.popover = false;
        cx.notify();
    });
    match task {
        Task::NameFirstRecord => {
            focus_centre(guide, window, cx);
            table::select_first_row(true, cx);
            guide.update(cx, |guide, cx| guide.show_tip(Tip::TitleCell, cx));
        }
        Task::AddFields => {
            focus_centre(guide, window, cx);
            guide.update(cx, |guide, cx| guide.show_tip(Tip::AddFields, cx));
        }
        Task::AddFilesFolder => {
            let blank = guide.read(cx).kind == GuideKind::Blank;
            if let Some(table) = cx
                .try_global::<table::TablePanelHandle>()
                .and_then(|handle| handle.0.upgrade())
            {
                table.update(cx, |table, cx| {
                    if blank {
                        table.choose_import_paths(window, cx)
                    } else {
                        table.choose_files_root(window, cx)
                    }
                });
            }
        }
        Task::OpenRow | Task::SelectRow => {
            ensure_visible(guide, DETAILS_META.name, window, cx);
            focus_centre(guide, window, cx);
            table::select_first_row(false, cx);
        }
        Task::ReviewFindings => {
            ensure_visible(guide, PROBLEMS_META.name, window, cx);
            guide.update(cx, |guide, cx| guide.show_tip(Tip::Problems, cx));
        }
        Task::BrowseGallery => {
            guide.update(cx, |guide, cx| guide.show_tip(Tip::Gallery, cx));
        }
    }
}

fn close_tip(guide: &Entity<Guide>, window: &mut Window, cx: &mut App) {
    guide.update(cx, |guide, cx| guide.close_tip(cx));
    // Esc and every button return focus to what the tip pointed at, which is the centre for all
    // but Problems — and the centre is the one that keeps the keyboard working either way.
    focus_centre(guide, window, cx);
}

// ---------------------------------------------------------------------------------------------
// Rendering.

fn text_body() -> Div {
    div().text_size(BODY).line_height(BODY_LINE)
}

/// Everything the card needs, read out once so the builders below never hold the guide.
struct CardModel {
    progress: usize,
    total: usize,
    title: &'static str,
    subtitle: String,
    rows: Vec<(Task, TaskState, String)>,
    current_hint: Option<String>,
    finished: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum TaskState {
    Done,
    Current,
    Todo,
}

fn card_model(guide: &Guide, compact: bool, cx: &App) -> CardModel {
    let current = guide.current();
    let findings = diagnostics::Diagnostics::all(cx).len();
    let finished = current.is_none();
    CardModel {
        progress: guide.progress(),
        total: 3,
        title: if finished {
            "You're all set"
        } else {
            "Your project is ready"
        },
        subtitle: if finished {
            "Help ▸ User Guide covers the rest.".into()
        } else {
            guide.subtitle(cx)
        },
        rows: guide
            .tasks()
            .into_iter()
            .map(|task| {
                let state = if guide.done.contains(&task) {
                    TaskState::Done
                } else if Some(task) == current {
                    TaskState::Current
                } else {
                    TaskState::Todo
                };
                (task, state, task.title(compact).to_string())
            })
            .collect(),
        current_hint: current.map(|task| task.hint(guide.kind, findings)),
        finished,
    }
}

fn status_dot(state: TaskState, cx: &App) -> Div {
    let base = div().size(px(16.)).flex_none().rounded_full().mt(px(1.));
    match state {
        TaskState::Done => base
            .bg(cx.theme().success)
            .text_color(gpui::white())
            .text_size(px(11.))
            .line_height(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .child("✓"),
        TaskState::Current => base.border_2().border_color(cx.theme().primary),
        TaskState::Todo => base
            .border(px(1.5))
            .border_color(cx.theme().muted_foreground),
    }
}

/// The card's content, shared by the floating card (6a) and the status-bar popover (6c/11b).
/// `compact` drops the subtitle and tightens the rows so the list fits 238px of body.
fn card_body(
    guide: &Entity<Guide>,
    model: CardModel,
    compact: bool,
    header_suffix: Option<AnyElement>,
    cx: &App,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let current_bg = cx.theme().primary.opacity(0.07);
    let hint = model.current_hint.clone();
    v_flex()
        .text_color(cx.theme().foreground)
        .child(
            v_flex()
                .px_3()
                .pt_3()
                .pb_2()
                .gap_0p5()
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .text_xs()
                        .text_color(muted)
                        .child(format!(
                            "Getting started · {} of {}",
                            model.progress, model.total
                        ))
                        .children(header_suffix),
                )
                .when(!compact, |el| {
                    el.child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(model.title),
                    )
                    .child(text_body().text_color(muted).child(model.subtitle.clone()))
                }),
        )
        .child(v_flex().px_1().pb_1().children(model.rows.into_iter().map(
            |(task, state, title)| {
                let current = state == TaskState::Current;
                h_flex()
                    .id(SharedString::from(format!("guide-task-{}", task.key())))
                    .gap_2p5()
                    .items_start()
                    .p(if compact { px(6.) } else { px(8.) })
                    .rounded(px(6.))
                    .when(current, |el| el.bg(current_bg))
                    .child(status_dot(state, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                text_body()
                                    .when(state == TaskState::Done, |el| el.text_color(muted))
                                    .when(current, |el| el.font_weight(FontWeight::SEMIBOLD))
                                    .child(title),
                            )
                            .when(current && !compact, |el| {
                                el.children(
                                    hint.clone()
                                        .map(|hint| text_body().text_color(muted).child(hint)),
                                )
                            }),
                    )
                    .when(current, |el| {
                        let guide = guide.clone();
                        el.child(
                            Button::new(SharedString::from(format!(
                                "guide-show-me-{}",
                                task.key()
                            )))
                            .label("Show me")
                            .link()
                            .small()
                            .on_click(move |_, window, cx| show_me(&guide, task, window, cx)),
                        )
                    })
            },
        )))
        .child(
            h_flex()
                .justify_end()
                .gap_1()
                .px_2()
                .py_1p5()
                .border_t_1()
                .border_color(cx.theme().border)
                .map(|footer| {
                    if model.finished {
                        let guide = guide.clone();
                        footer.child(
                            Button::new("guide-finish")
                                .label("Close")
                                .ghost()
                                .xsmall()
                                .on_click(move |_, _, cx| {
                                    guide.update(cx, |guide, cx| guide.dismiss(cx))
                                }),
                        )
                    } else {
                        let later = guide.clone();
                        let dismiss = guide.clone();
                        footer
                            .child(
                                Button::new("guide-later")
                                    .label("Show me later")
                                    .ghost()
                                    .xsmall()
                                    .on_click(move |_, _, cx| {
                                        later.update(cx, |guide, cx| {
                                            guide.popover = false;
                                            guide.set_mode(Mode::Later, cx);
                                        });
                                        cx.refresh_windows();
                                    }),
                            )
                            .child(
                                Button::new("guide-dismiss")
                                    .label("Dismiss")
                                    .ghost()
                                    .xsmall()
                                    .on_click(move |_, _, cx| {
                                        dismiss.update(cx, |guide, cx| guide.dismiss(cx));
                                        cx.refresh_windows();
                                    }),
                            )
                    }
                }),
        )
}

fn surface(cx: &App) -> Div {
    div()
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(8.))
        .shadow_lg()
}

/// Which way a tip's arrow points, and how far along its edge.
#[derive(Clone, Copy)]
enum Arrow {
    Up(Pixels),
    Down(Pixels),
    Right(Pixels),
}

struct TipCopy {
    title: &'static str,
    lines: Vec<String>,
    /// `(label, shortcut)` for a primary action other than "Got it".
    primary: Option<(&'static str, Option<&'static str>)>,
    secondary: Option<&'static str>,
}

fn tip_copy(tip: Tip) -> TipCopy {
    let got_it = |title, lines: &[&str]| TipCopy {
        title,
        lines: lines.iter().map(|line| line.to_string()).collect(),
        primary: None,
        secondary: None,
    };
    match tip {
        Tip::TitleCell => got_it(
            "Name your first record",
            &["Type a title and press Enter to keep it. This row is already here for you."],
        ),
        Tip::AddFields => got_it(
            "Add a field",
            &[
                "Use Insert ▸ Column Right, then Data ▸ Rename Column… to name it for what you \
                 record, like Date or Creator.",
            ],
        ),
        Tip::Problems => got_it(
            "Review what qrate found",
            &[
                "Each finding points to a cell. A suggested correction changes the value only if \
                 you accept it.",
                "Looking through the list counts as progress. You don't have to fix anything.",
            ],
        ),
        Tip::Gallery => TipCopy {
            title: "Browse in Gallery",
            lines: vec![
                "See every linked file as a thumbnail. Selecting one selects its row, and Details \
                 moves to the right while you browse."
                    .into(),
            ],
            primary: Some((
                "Show Gallery",
                Some(if cfg!(target_os = "macos") {
                    "⌘2"
                } else {
                    "Ctrl+2"
                }),
            )),
            secondary: Some("Not now"),
        },
        Tip::DetailsMoved => got_it(
            "Details moved here",
            &[
                "In Gallery, the selected item's file and fields sit on the right. Switch back to \
                 Table and your layout returns.",
            ],
        ),
    }
}

fn render_tip(guide: &Entity<Guide>, tip: Tip, compact: bool, cx: &App) -> AnyElement {
    let copy = tip_copy(tip);
    let muted = cx.theme().muted_foreground;
    let width = if compact {
        TIP_WIDTH_COMPACT
    } else {
        TIP_WIDTH
    };
    let popover = cx.theme().popover;
    let border = cx.theme().border;
    // Where it's pinned, in the centre body's coordinates (below the view switcher). The grid's
    // header and first row are 32px each, so the Title cell's tip hangs just under row 1.
    let (placed, arrow) = match tip {
        Tip::TitleCell => (div().left(px(62.)).top(px(74.)), Arrow::Up(px(30.))),
        Tip::AddFields => (div().left(px(62.)).top(px(42.)), Arrow::Up(px(30.))),
        Tip::Gallery => (div().left(px(48.)).top(px(8.)), Arrow::Up(px(40.))),
        Tip::Problems => (div().left(px(10.)).bottom(px(10.)), Arrow::Down(px(28.))),
        Tip::DetailsMoved => (div().right(px(12.)).top(px(40.)), Arrow::Right(px(16.))),
    };
    let arrow_el = {
        let base = div()
            .absolute()
            .size(px(10.))
            .bg(popover)
            .border_color(border);
        // A rotated square is the usual caret; gpui has no transforms on divs, so the caret is a
        // small bordered notch instead, drawn over the edge it points through.
        match arrow {
            Arrow::Up(at) => base
                .left(at)
                .top(px(-6.))
                .border_l_1()
                .border_t_1()
                .rounded_tl(px(2.)),
            Arrow::Down(at) => base
                .left(at)
                .bottom(px(-6.))
                .border_r_1()
                .border_b_1()
                .rounded_br(px(2.)),
            Arrow::Right(at) => base
                .top(at)
                .right(px(-6.))
                .border_r_1()
                .border_t_1()
                .rounded_tr(px(2.)),
        }
    };
    let escape = guide.clone();
    let primary = guide.clone();
    placed
        .absolute()
        .w(width)
        .child(
            surface(cx)
                .id(SharedString::from(format!("guide-tip-{}", tip.key())))
                .relative()
                .p_3()
                .flex()
                .flex_col()
                .gap_1p5()
                .occlude()
                // Esc closes the tip and hands focus back to what it points at.
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        close_tip(&escape, window, cx);
                    }
                })
                .child(arrow_el)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(copy.title),
                )
                .children(
                    copy.lines
                        .into_iter()
                        .map(|line| text_body().text_color(muted).child(line)),
                )
                .child(
                    h_flex()
                        .justify_end()
                        .items_center()
                        .gap_1p5()
                        .mt_1()
                        .when_some(copy.primary.and_then(|(_, key)| key), |row, key| {
                            row.child(
                                div()
                                    .px_1()
                                    .rounded(px(3.))
                                    .border_1()
                                    .border_color(border)
                                    .text_xs()
                                    .text_color(muted)
                                    .child(key),
                            )
                            .child(div().flex_1())
                        })
                        .when_some(copy.secondary, |row, label| {
                            let guide = guide.clone();
                            row.child(
                                Button::new("guide-tip-secondary")
                                    .label(label)
                                    .ghost()
                                    .xsmall()
                                    .on_click(move |_, window, cx| close_tip(&guide, window, cx)),
                            )
                        })
                        .child(match copy.primary {
                            Some((label, _)) => Button::new("guide-tip-primary")
                                .label(label)
                                .primary()
                                .xsmall()
                                .on_click(move |_, window, cx| {
                                    primary.update(cx, |guide, cx| guide.close_tip(cx));
                                    workspace::show_view(ViewMode::Gallery, window, cx);
                                }),
                            None => Button::new("guide-tip-primary")
                                .label("Got it")
                                .primary()
                                .xsmall()
                                .on_click(move |_, window, cx| close_tip(&primary, window, cx)),
                        }),
                ),
        )
        .into_any_element()
}

fn render_toast(guide: &Entity<Guide>, cx: &App) -> AnyElement {
    let undo = guide.clone();
    div()
        .absolute()
        .right(px(16.))
        .bottom(px(16.))
        .child(
            surface(cx)
                .id("guide-toast")
                .occlude()
                .flex()
                .items_center()
                .gap_3()
                .pl_3()
                .pr_1p5()
                .py_1p5()
                .child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .text_size(BODY)
                        .child("Getting started hidden. Reopen it from")
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Help ▸ Getting Started"),
                        )
                        .child("."),
                )
                .child(
                    Button::new("guide-undo")
                        .label("Undo")
                        .link()
                        .small()
                        .on_click(move |_, _, cx| {
                            undo.update(cx, |guide, cx| guide.undo_dismiss(cx));
                            cx.refresh_windows();
                        }),
                ),
        )
        .into_any_element()
}

/// What the centre draws over its view: the floating card (6a), the current tip, and the note
/// that follows Dismiss. `None` when there is nothing, which is most of the time.
fn centre_overlay(cx: &App) -> Option<AnyElement> {
    let guide = active(cx)?;
    let state = guide.read(cx);
    // No project, no guide — the empty workspace before one opens.
    state.project.as_ref()?;
    let compact = state.compact();
    let mut layers: Vec<AnyElement> = Vec::new();
    if state.mode == Mode::Open && !compact {
        let model = card_model(state, false, cx);
        layers.push(
            div()
                .absolute()
                .right(px(16.))
                .bottom(px(16.))
                .w(CARD_WIDTH)
                .child(
                    surface(cx)
                        .id("guide-card")
                        .occlude()
                        .child(card_body(&guide, model, false, None, cx)),
                )
                .into_any_element(),
        );
    }
    if let Some(tip) = state.tip.filter(|_| state.mode.active()) {
        layers.push(render_tip(&guide, tip, compact, cx));
    }
    if state.toast.is_some() {
        layers.push(render_toast(&guide, cx));
    }
    (!layers.is_empty()).then(|| {
        div()
            .absolute()
            .inset_0()
            .children(layers)
            .into_any_element()
    })
}

/// The status-bar label's popover (6c, 11b), drawn by the workspace so it can open upward over
/// the window body. Pinned just above the label at the bar's left end.
fn window_overlay(cx: &App) -> Option<AnyElement> {
    let guide = active(cx)?;
    let state = guide.read(cx);
    if !state.popover || !state.label_visible() {
        return None;
    }
    let compact = state.compact();
    let model = card_model(state, compact, cx);
    let collapse = guide.clone();
    let chevron = Button::new("guide-popover-close")
        .icon(IconName::ChevronDown)
        .ghost()
        .xsmall()
        .on_click(move |_, _, cx| {
            collapse.update(cx, |guide, cx| {
                guide.popover = false;
                cx.notify();
            });
            cx.refresh_windows();
        })
        .into_any_element();
    Some(
        div()
            .absolute()
            .left(px(8.))
            .bottom(px(6.))
            .w(POPOVER_WIDTH)
            .child(
                surface(cx)
                    .id("guide-popover")
                    .occlude()
                    .max_h(px(238. + 60.))
                    .overflow_hidden()
                    .child(card_body(&guide, model, compact, Some(chevron), cx)),
            )
            .into_any_element(),
    )
}

/// The status bar's "Getting started · n of 3" label. Registered by `app`, which owns the bar.
pub struct GuideStatusItem {
    _sub: Option<Subscription>,
}

impl GuideStatusItem {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            _sub: active(cx).map(|guide| cx.observe(&guide, |_, _, cx| cx.notify())),
        }
    }
}

impl Render for GuideStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(guide) = active(cx) else {
            return div().into_any_element();
        };
        if self._sub.is_none() {
            self._sub = Some(cx.observe(&guide, |_, _, cx| cx.notify()));
        }
        let state = guide.read(cx);
        if !state.label_visible() {
            return div().into_any_element();
        }
        let label = format!("Getting started · {} of 3", state.progress());
        let primary = cx.theme().primary;
        h_flex()
            .id("guide-status-label")
            .cursor_pointer()
            .gap_1p5()
            .items_center()
            .h(px(20.))
            .px_2()
            .rounded(px(10.))
            .bg(primary.opacity(0.14))
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(div().size(px(7.)).rounded_full().bg(primary))
            .child(label)
            .on_click(move |_, _, cx| {
                guide.update(cx, |guide, cx| {
                    guide.popover = !guide.popover;
                    cx.notify();
                });
                cx.refresh_windows();
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{ALL_TASKS, Mode, Task, tasks_for};
    use settings::onboarding::GuideKind;

    #[test]
    fn every_task_and_mode_round_trips_through_its_setting() {
        for task in ALL_TASKS {
            assert_eq!(Task::parse(task.key()), Some(task));
        }
        for mode in [Mode::Open, Mode::Later, Mode::Dismissed] {
            assert_eq!(Mode::parse(mode.as_str()), mode);
        }
        assert_eq!(
            Mode::parse(""),
            Mode::Off,
            "no key means the project predates the guide"
        );
    }

    #[test]
    fn each_kind_offers_three_distinct_tasks() {
        for kind in [GuideKind::Blank, GuideKind::Import, GuideKind::Media] {
            let tasks = tasks_for(kind);
            assert!(tasks[0] != tasks[1] && tasks[1] != tasks[2] && tasks[0] != tasks[2]);
        }
    }
}

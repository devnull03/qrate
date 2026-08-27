//! The multi-step "New Project" wizard window (Stage 2-6 of the design).
//! Opened from the launcher's "Create New" cards. A single long-lived
//! `ProjectWizard` view holds all step state; `steps/*.rs` each add a
//! `render_*` method via a separate `impl ProjectWizard` block.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::combobox::{ComboboxEvent, ComboboxState};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::label::Label;
use gpui_component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_component::stepper::{Stepper, StepperItem};
use gpui_component::{
    ActiveTheme, Disableable, Root, Sizable, StyledExt, TitleBar, h_flex, v_flex,
};
use window_wrapper::WindowRegistry;

use crate::data::{ColumnConfigPreview, FolderMatch, SheetCheckResult};
use crate::launcher;
use data_exchange::SpreadsheetPreview;

pub const WIZARD_WINDOW_KIND: &str = "project-creation";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Blank,
    Csv,
    Sheet,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WizardStep {
    Name,
    Files,
    Link,
    Columns,
    Review,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkMethod {
    ExactFilename,
    CustomPattern,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColumnSource {
    AutoFromSpreadsheet,
    LoadFromFileOrSheet,
    /// Blank projects have no spreadsheet to derive columns from — defer setup.
    SkipForNow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadConfigTab {
    File,
    Sheet,
}

#[derive(Clone, PartialEq)]
pub(crate) struct ColumnChoice {
    pub(crate) value: SharedString,
    pub(crate) label: SharedString,
}

impl SearchableListItem for ColumnChoice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}
pub struct ProjectWizard {
    pub(crate) step: WizardStep,
    pub(crate) entry_kind: EntryKind,

    // Name step
    pub(crate) name_input: Entity<InputState>,
    pub(crate) save_path: String,
    /// Display half of `save_path`, because [`PathPickerApp`] renders a readonly `Input`. Written
    /// wherever `save_path` is.
    pub(crate) save_path_input: Entity<InputState>,
    pub(crate) name_error: Option<SharedString>,
    /// Live-validates the name as it's typed; kept alive by holding it here.
    _name_sub: Subscription,

    // Files step - CSV
    pub(crate) csv_path: String,
    pub(crate) csv_preview: Option<SpreadsheetPreview>,
    pub(crate) csv_error: Option<SharedString>,
    pub(crate) folder_path: String,
    pub(crate) folder_match: Option<FolderMatch>,
    pub(crate) folder_error: Option<SharedString>,
    /// "I'll add files later" — skips folder matching and the whole Link step.
    pub(crate) skip_files: bool,

    // Files step - Sheet
    pub(crate) sheet_link_input: Entity<InputState>,
    pub(crate) sheet_check: Option<SheetCheckResult>,
    pub(crate) sheet_error: Option<SharedString>,

    // Link step
    pub(crate) link_method: LinkMethod,
    pub(crate) link_pattern_input: Entity<InputState>,
    pub(crate) show_advanced_pattern: bool,
    /// Look for filenames in subfolders too, not just directly in the files folder.
    pub(crate) recurse_subfolders: bool,

    // Columns step
    pub(crate) column_source: ColumnSource,
    pub(crate) show_advanced_mapping: bool,
    pub(crate) load_config_tab: LoadConfigTab,
    pub(crate) config_file_path: String,
    pub(crate) config_preview: Option<ColumnConfigPreview>,
    pub(crate) config_error: Option<SharedString>,
    pub(crate) title_column: Option<String>,
    pub(crate) file_column: Option<String>,
    pub(crate) title_picker: Entity<ComboboxState<SearchableVec<ColumnChoice>>>,
    pub(crate) file_picker: Entity<ComboboxState<SearchableVec<ColumnChoice>>>,
    pub(crate) required_column_choices: Vec<ColumnChoice>,
    _title_picker_sub: Subscription,
    _file_picker_sub: Subscription,
}

fn required_columns(title: Option<&str>, file: Option<&str>) -> Result<(), &'static str> {
    let title = title.unwrap_or_default();
    let file = file.unwrap_or_default();
    if title.is_empty() {
        return Err("Choose the column that contains each row's title");
    }
    if file.is_empty() {
        return Err("Choose the column that contains each row's file");
    }
    if title == file {
        return Err("Title and File must use different columns");
    }
    Ok(())
}
impl ProjectWizard {
    pub fn new(entry_kind: EntryKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("New Project — qrate");
        let name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. Aderman Family Collection"));
        // Live-validate the name on every keystroke so the inline error and the
        // Next button react as the user types, not just on click.
        let name_sub = cx.subscribe(&name_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.validate_name(cx);
                cx.notify();
            }
        });
        let sheet_link_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("docs.google.com/spreadsheets/d/…"));
        let link_pattern_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. {id}_*.jpg"));

        let default_save_dir = dirs::document_dir()
            .map(|d| d.join("qrate"))
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let save_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Choose a folder…")
                .default_value(default_save_dir.clone())
        });
        let required_column_choices = vec![ColumnChoice {
            value: "".into(),
            label: "Choose a column…".into(),
        }];
        let title_picker = cx.new(|cx| {
            ComboboxState::new(
                SearchableVec::new(required_column_choices.clone()),
                vec![],
                window,
                cx,
            )
            .multiple(false)
            .searchable(false)
        });
        let file_picker = cx.new(|cx| {
            ComboboxState::new(
                SearchableVec::new(required_column_choices.clone()),
                vec![],
                window,
                cx,
            )
            .multiple(false)
            .searchable(false)
        });
        let title_picker_sub = cx.subscribe(
            &title_picker,
            |this, _, event: &ComboboxEvent<SearchableVec<ColumnChoice>>, cx| {
                if let ComboboxEvent::Change(values) = event {
                    let selected = values.first().map(ToString::to_string);
                    if selected.as_deref() == this.file_column.as_deref() {
                        this.file_column = Some(String::new());
                    }
                    this.title_column = selected;
                    cx.notify();
                }
            },
        );
        let file_picker_sub = cx.subscribe(
            &file_picker,
            |this, _, event: &ComboboxEvent<SearchableVec<ColumnChoice>>, cx| {
                if let ComboboxEvent::Change(values) = event {
                    let selected = values.first().map(ToString::to_string);
                    if selected.as_deref() == this.title_column.as_deref() {
                        this.title_column = Some(String::new());
                    }
                    this.file_column = selected;
                    cx.notify();
                }
            },
        );

        Self {
            step: WizardStep::Name,
            entry_kind,
            name_input,
            save_path: default_save_dir,
            save_path_input,
            name_error: None,
            _name_sub: name_sub,
            csv_path: String::new(),
            csv_preview: None,
            csv_error: None,
            folder_path: String::new(),
            folder_match: None,
            folder_error: None,
            skip_files: false,
            sheet_link_input,
            sheet_check: None,
            sheet_error: None,
            link_method: LinkMethod::ExactFilename,
            link_pattern_input,
            show_advanced_pattern: false,
            // On by default: `table::photos` resolves rows against the whole tree, so an
            // off-by-default check would report fewer matches than the app will actually find.
            recurse_subfolders: true,
            // Blank has no spreadsheet to auto-derive columns from.
            column_source: if entry_kind == EntryKind::Blank {
                ColumnSource::SkipForNow
            } else {
                ColumnSource::AutoFromSpreadsheet
            },
            show_advanced_mapping: false,
            load_config_tab: LoadConfigTab::File,
            config_file_path: String::new(),
            config_preview: None,
            config_error: None,
            title_column: (entry_kind == EntryKind::Blank).then(|| "Title".to_string()),
            file_column: (entry_kind == EntryKind::Blank).then(|| "File".to_string()),
            title_picker,
            file_picker,
            required_column_choices,
            _title_picker_sub: title_picker_sub,
            _file_picker_sub: file_picker_sub,
        }
    }

    pub(crate) fn project_name(&self, cx: &App) -> String {
        self.name_input.read(cx).value().to_string()
    }

    pub(crate) fn spreadsheet_headers(&self) -> Vec<String> {
        match self.entry_kind {
            // Sheet reuses `csv_preview` too — its fetched CSV is parsed the
            // same way (see steps/files.rs `check_sheet_link`).
            EntryKind::Csv | EntryKind::Sheet => self
                .csv_preview
                .as_ref()
                .map(|p| p.headers.clone())
                .unwrap_or_default(),
            EntryKind::Blank => vec!["Title".into(), "File".into()],
        }
    }

    /// The Link step is skipped when there's no folder to link against
    /// (files skipped) or no spreadsheet rows to link (Blank).
    pub(crate) fn skips_link(&self) -> bool {
        self.skip_files || self.entry_kind == EntryKind::Blank
    }

    /// Whether the current step is valid enough to advance. `Ok(())` means the
    /// Next button is live; `Err(reason)` carries the human-readable reason it's
    /// blocked, which the footer shows beside the greyed-out button. Reads
    /// existing per-step state — the actual validators run elsewhere.
    pub(crate) fn can_advance(&self, cx: &App) -> Result<(), SharedString> {
        match self.step {
            WizardStep::Name => {
                let name = self.project_name(cx);
                if name.trim().is_empty() {
                    return Err("Give your project a name to continue".into());
                }
                if self.save_path.trim().is_empty() {
                    return Err("Choose where to save this project".into());
                }
                if crate::project::name_taken(&self.save_path, &name) {
                    return Err("A project with this name already exists here".into());
                }
                Ok(())
            }
            WizardStep::Files => match self.entry_kind {
                // `skip_files` waives only the folder requirement, not the spreadsheet/sheet check.
                EntryKind::Csv => {
                    if self.csv_preview.is_none() {
                        return Err("Choose a valid CSV spreadsheet".into());
                    }
                    if !self.skip_files && self.folder_match.is_none() {
                        return Err("Choose a files folder that matches your spreadsheet".into());
                    }
                    Ok(())
                }
                EntryKind::Sheet => {
                    if self.sheet_check.is_none() {
                        return Err("Check your Google Sheet link first".into());
                    }
                    if !self.skip_files && self.folder_match.is_none() {
                        return Err("Choose a files folder that matches your sheet".into());
                    }
                    Ok(())
                }
                EntryKind::Blank => Ok(()),
            },
            WizardStep::Link => {
                if self.link_method == LinkMethod::CustomPattern
                    && self.link_pattern_input.read(cx).value().trim().is_empty()
                {
                    return Err("Enter a naming pattern, or switch back to exact filename".into());
                }
                Ok(())
            }
            WizardStep::Columns => {
                if self.column_source == ColumnSource::LoadFromFileOrSheet
                    && self.config_preview.is_none()
                {
                    return Err("Load a column config, or pick a different source".into());
                }
                required_columns(self.title_column.as_deref(), self.file_column.as_deref())
                    .map_err(Into::into)
            }
            WizardStep::Review => Ok(()),
        }
    }

    /// On the Files step for a Google Sheet, pressing Next fetches the sheet
    /// itself if it hasn't been checked yet — the separate "Check" button
    /// becomes optional. True while a link is typed but not yet fetched;
    /// checking never depends on a folder being chosen (`skip_files` doesn't
    /// exempt it either — see the note on `can_advance`), so `check_sheet_link`'s
    /// own `can_advance` re-check after the fetch is what decides whether to
    /// auto-advance or leave the (now folder-only) blocker showing.
    fn needs_sheet_check(&self, cx: &App) -> bool {
        self.step == WizardStep::Files
            && self.entry_kind == EntryKind::Sheet
            && self.sheet_check.is_none()
            && !self.sheet_link_input.read(cx).value().trim().is_empty()
    }

    /// Shared by the manual Next click and the auto-advance after a
    /// just-succeeded sheet check (see `steps/files.rs::check_sheet_link`).
    pub(crate) fn advance_past_files(&mut self) {
        self.step = if self.entry_kind == EntryKind::Blank {
            WizardStep::Review
        } else if self.skips_link() {
            WizardStep::Columns
        } else {
            WizardStep::Link
        };
    }

    fn breadcrumb_items(&self) -> Vec<(&'static str, WizardStep)> {
        let mut items = vec![("Name", WizardStep::Name), ("Files", WizardStep::Files)];
        if !self.skips_link() {
            items.push(("Link", WizardStep::Link));
        }
        if self.entry_kind != EntryKind::Blank {
            items.push(("Columns", WizardStep::Columns));
        }
        items.push(("Create", WizardStep::Review));
        items
    }

    fn step_index(&self) -> usize {
        self.breadcrumb_items()
            .iter()
            .position(|(_, s)| *s == self.step)
            .unwrap_or(0)
    }

    /// Only a step already completed is clickable — jumping *forward* would skip the validation
    /// `can_advance` runs on the way out of each step.
    fn render_breadcrumb(&self, cx: &Context<Self>) -> impl IntoElement {
        let items = self.breadcrumb_items();
        let current_ix = self.step_index();
        Stepper::new("wizard-steps")
            .small()
            .selected_index(current_ix)
            .items(
                items
                    .iter()
                    .map(|(label, _)| StepperItem::new().child(*label)),
            )
            .on_click(cx.listener(move |this, ix: &usize, _window, cx| {
                if let Some((_, step)) = items.get(*ix).filter(|_| *ix < current_ix) {
                    this.step = *step;
                    cx.notify();
                }
            }))
    }

    pub(crate) fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = match (self.entry_kind, self.step) {
            (_, WizardStep::Name) => {
                // Back to the launcher: close this window and show it again.
                launcher::open_launcher_window(cx);
                window.remove_window();
                return;
            }
            (_, WizardStep::Files) => WizardStep::Name,
            (_, WizardStep::Link) => WizardStep::Files,
            (_, WizardStep::Columns) => {
                if self.skips_link() {
                    WizardStep::Files
                } else {
                    WizardStep::Link
                }
            }
            (EntryKind::Blank, WizardStep::Review) => WizardStep::Files,
            (_, WizardStep::Review) => WizardStep::Columns,
        };
        cx.notify();
    }

    fn go_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Google Sheet: Next doubles as "Check" until the sheet is fetched.
        // The async result re-renders; the user then presses Next to advance.
        if self.needs_sheet_check(cx) {
            self.check_sheet_link(true, cx);
            return;
        }
        if self.can_advance(cx).is_err() {
            cx.notify();
            return;
        }
        match self.step {
            WizardStep::Name => {
                if !self.validate_name(cx) {
                    cx.notify();
                    return;
                }
                self.step = WizardStep::Files;
            }
            WizardStep::Files => self.advance_past_files(),
            WizardStep::Link => self.step = WizardStep::Columns,
            WizardStep::Columns => self.step = WizardStep::Review,
            WizardStep::Review => self.create_project(window, cx),
        }
        cx.notify();
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let next_label = if self.step == WizardStep::Review {
            "Create Project"
        } else {
            "Next →"
        };
        // While the sheet needs checking, Next acts as "Check" (no blocker); otherwise a blocker greys it out.
        let blocker = if self.needs_sheet_check(cx) {
            None
        } else {
            self.can_advance(cx).err()
        };
        let disabled = blocker.is_some();
        h_flex()
            .justify_between()
            .items_center()
            .mt_4()
            .child(
                div()
                    .id("wizard-back")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .cursor_pointer()
                    .child("← Back")
                    .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .when_some(blocker, |el, reason| {
                        el.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(reason),
                        )
                    })
                    .child(
                        Button::new("wizard-next")
                            .label(next_label)
                            .primary()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| this.go_next(window, cx))),
                    ),
            )
    }
}

impl Render for ProjectWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);

        let body = match self.step {
            WizardStep::Name => self.render_name_step(window, cx).into_any_element(),
            WizardStep::Files => self.render_files_step(window, cx).into_any_element(),
            WizardStep::Link => self.render_link_step(window, cx).into_any_element(),
            WizardStep::Columns => self.render_columns_step(window, cx).into_any_element(),
            WizardStep::Review => self.render_review_step(window, cx).into_any_element(),
        };

        // Shared scaffold: pinned breadcrumb, scrolling body, pinned footer; each step supplies only `body`.
        let breadcrumb = self.render_breadcrumb(cx).into_any_element();
        let footer = self.render_footer(cx).into_any_element();

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                TitleBar::new()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .child(Label::new("qrate — New Project").font_semibold()),
            )
            .child(
                v_flex()
                    .id("wizard-body")
                    .flex_1()
                    // min_h(0) overrides flex min-height:auto so the scroll region grows, not the page.
                    .min_h(px(0.))
                    .p_5()
                    .gap_3()
                    .child(breadcrumb)
                    // Middle region scrolls; breadcrumb and footer stay pinned.
                    .child(
                        div()
                            .id("wizard-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .child(body),
                    )
                    .child(footer),
            )
            .children(dialog_layer)
    }
}

pub fn open_project_wizard(entry_kind: EntryKind, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(560.0), px(680.0)), cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(Size::new(px(480.0), px(520.0))),
        ..Default::default()
    };

    // Open synchronously: gpui quits when the window list is empty (non-macOS), so a window
    // spawned from an async task would leave a zero-window gap that kills the app mid-transition.
    if let Ok(window_handle) = cx.open_window(window_options, |window, cx| {
        let view = cx.new(|cx| ProjectWizard::new(entry_kind, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        WindowRegistry::register(WIZARD_WINDOW_KIND, window_handle.into(), cx);
    }
}

/// Small bordered, clickable "radio card" used for the entry-kind picker,
/// link-method picker, and column-source picker — matches the prototype's
/// ◉/○ bordered option cards.
pub(crate) fn option_card(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let border = if selected {
        cx.theme().primary
    } else {
        cx.theme().border
    };
    let dot = div()
        .size_3()
        .rounded_full()
        .border_2()
        .border_color(if selected {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        })
        .when(selected, |el| el.bg(cx.theme().primary));
    v_flex()
        .id(id.into())
        .cursor_pointer()
        .gap_1()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(border)
        .when(selected, |el| el.bg(cx.theme().primary.opacity(0.06)))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(dot)
                .child(div().font_semibold().text_sm().child(title.into())),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(description.into()),
        )
}

#[cfg(test)]
mod tests {
    use crate::wizard::required_columns;

    #[test]
    fn columns_step_requires_two_distinct_roles() {
        assert!(required_columns(Some("Title"), Some("File")).is_ok());
        assert!(required_columns(None, Some("File")).is_err());
        assert!(required_columns(Some("Title"), None).is_err());
        assert!(required_columns(Some("Same"), Some("Same")).is_err());
    }
}

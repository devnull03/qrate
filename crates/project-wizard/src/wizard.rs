//! The multi-step "New Project" wizard window (Stage 2-6 of the design).
//! Opened from the launcher's "Create New" cards. A single long-lived
//! `ProjectWizard` view holds all step state; `steps/*.rs` each add a
//! `render_*` method via a separate `impl ProjectWizard` block.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Disableable, Root, StyledExt, TitleBar, h_flex, v_flex};
use window_wrapper::WindowRegistry;

use crate::data::{ColumnConfigPreview, FolderMatch, SheetCheckResult, SpreadsheetPreview};
use crate::launcher;

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

pub struct ProjectWizard {
    pub(crate) step: WizardStep,
    pub(crate) entry_kind: EntryKind,

    // Name step
    pub(crate) name_input: Entity<InputState>,
    pub(crate) save_path: String,
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

    // Columns step
    pub(crate) column_source: ColumnSource,
    pub(crate) show_advanced_mapping: bool,
    pub(crate) load_config_tab: LoadConfigTab,
    pub(crate) config_file_path: String,
    pub(crate) config_preview: Option<ColumnConfigPreview>,
    pub(crate) config_error: Option<SharedString>,
}

impl ProjectWizard {
    pub fn new(entry_kind: EntryKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
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

        Self {
            step: WizardStep::Name,
            entry_kind,
            name_input,
            save_path: default_save_dir,
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
            EntryKind::Blank => Vec::new(),
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
            WizardStep::Files => {
                if self.skip_files || self.entry_kind == EntryKind::Blank {
                    return Ok(());
                }
                match self.entry_kind {
                    EntryKind::Csv => {
                        if self.csv_preview.is_none() {
                            return Err("Choose a valid CSV spreadsheet".into());
                        }
                        if self.folder_match.is_none() {
                            return Err(
                                "Choose a files folder that matches your spreadsheet".into()
                            );
                        }
                        Ok(())
                    }
                    EntryKind::Sheet => {
                        if self.sheet_check.is_none() {
                            return Err("Check your Google Sheet link first".into());
                        }
                        if self.folder_match.is_none() {
                            return Err("Choose a files folder that matches your sheet".into());
                        }
                        Ok(())
                    }
                    EntryKind::Blank => Ok(()),
                }
            }
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
                Ok(())
            }
            WizardStep::Review => Ok(()),
        }
    }

    /// On the Files step for a Google Sheet, pressing Next fetches the sheet
    /// itself if it hasn't been checked yet — the separate "Check" button
    /// becomes optional. True while a link is typed but not yet fetched.
    fn needs_sheet_check(&self, cx: &App) -> bool {
        self.step == WizardStep::Files
            && self.entry_kind == EntryKind::Sheet
            && !self.skip_files
            && self.sheet_check.is_none()
            && !self.sheet_link_input.read(cx).value().trim().is_empty()
    }

    fn breadcrumb_items(&self) -> Vec<(&'static str, WizardStep)> {
        let mut items = vec![("Name", WizardStep::Name), ("Files", WizardStep::Files)];
        if !self.skips_link() {
            items.push(("Link", WizardStep::Link));
        }
        items.push(("Columns", WizardStep::Columns));
        items.push(("Create", WizardStep::Review));
        items
    }

    fn step_index(&self) -> usize {
        self.breadcrumb_items()
            .iter()
            .position(|(_, s)| *s == self.step)
            .unwrap_or(0)
    }

    fn render_breadcrumb(&self, cx: &Context<Self>) -> impl IntoElement {
        let items = self.breadcrumb_items();
        let current_ix = self.step_index();
        let mut row = h_flex().gap_2().items_center().text_sm().flex_wrap();
        row = row.child(div().text_color(cx.theme().success).child("✓ Start"));
        for (ix, (label, _)) in items.iter().enumerate() {
            row = row.child(div().text_color(cx.theme().muted_foreground).child("·"));
            let el = if ix < current_ix {
                div()
                    .text_color(cx.theme().success)
                    .child(format!("✓ {label}"))
            } else if ix == current_ix {
                div()
                    .font_semibold()
                    .text_color(cx.theme().primary)
                    .child(*label)
            } else {
                div().text_color(cx.theme().muted_foreground).child(*label)
            };
            row = row.child(el);
        }
        row
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
            (_, WizardStep::Review) => WizardStep::Columns,
        };
        cx.notify();
    }

    fn go_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Google Sheet: Next doubles as "Check" until the sheet is fetched.
        // The async result re-renders; the user then presses Next to advance.
        if self.needs_sheet_check(cx) {
            self.check_sheet_link(cx);
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
            WizardStep::Files => {
                self.step = if self.skips_link() {
                    WizardStep::Columns
                } else {
                    WizardStep::Link
                };
            }
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
        // When the sheet still needs checking, Next doubles as "Check" and is
        // live regardless of the blocker. Otherwise a blocker greys out Next and
        // its reason is shown beside it.
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

        // Shared scaffold for every step: pinned breadcrumb on top, scrolling
        // body in the middle, pinned Back/Next footer at the bottom. Each step
        // only supplies `body`; navigation and validation live in the footer.
        let breadcrumb = self.render_breadcrumb(cx).into_any_element();
        let footer = self.render_footer(cx).into_any_element();

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(TitleBar::new().child(Label::new("qrate — New Project").font_semibold()))
            .child(
                v_flex()
                    .id("wizard-body")
                    .flex_1()
                    // min_h(0) bounds this to the window height (below the title
                    // bar) so the scroll region — not the whole page — grows.
                    // Without it the flex item's default min-height:auto expands
                    // to fit all steps' content, pushing the footer off-screen.
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

    cx.spawn(async move |cx| {
        let result = cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| ProjectWizard::new(entry_kind, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Ok(window_handle) = result {
            cx.update(|cx| {
                WindowRegistry::register(WIZARD_WINDOW_KIND, window_handle.into(), cx);
            })
            .ok();
        }
    })
    .detach();
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

//! The multi-step "New Project" wizard window (Stage 2-6 of the design).
//! Opened from the launcher's "Create New" cards. A single long-lived
//! `ProjectWizard` view holds all step state; `steps/*.rs` each add a
//! `render_*` method via a separate `impl ProjectWizard` block.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::InputState;
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Root, StyledExt, TitleBar, h_flex, v_flex};
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
    Success,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkMethod {
    ExactFilename,
    CustomPattern,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColumnSource {
    AutoFromSpreadsheet,
    RecentlyUsed,
    LoadFromFileOrSheet,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadConfigTab {
    File,
    Sheet,
}

pub struct RecentConfig {
    pub name: &'static str,
    pub column_count: usize,
    pub last_used: &'static str,
}

pub const RECENT_CONFIGS: &[RecentConfig] = &[
    RecentConfig {
        name: "Aderman Collection",
        column_count: 7,
        last_used: "2 days ago",
    },
    RecentConfig {
        name: "Photo Donations 2025",
        column_count: 5,
        last_used: "1 month ago",
    },
    RecentConfig {
        name: "Oral History Intake",
        column_count: 9,
        last_used: "3 months ago",
    },
];

pub struct ProjectWizard {
    pub(crate) step: WizardStep,
    pub(crate) entry_kind: EntryKind,

    // Name step
    pub(crate) name_input: Entity<InputState>,
    pub(crate) save_path: String,
    pub(crate) name_error: Option<SharedString>,

    // Files step - CSV
    pub(crate) csv_path: String,
    pub(crate) csv_preview: Option<SpreadsheetPreview>,
    pub(crate) csv_error: Option<SharedString>,
    pub(crate) folder_path: String,
    pub(crate) folder_match: Option<FolderMatch>,
    pub(crate) folder_error: Option<SharedString>,

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
    pub(crate) recent_config_selected: usize,
    pub(crate) load_config_tab: LoadConfigTab,
    pub(crate) config_file_path: String,
    pub(crate) config_preview: Option<ColumnConfigPreview>,
    pub(crate) config_error: Option<SharedString>,

    // Review / Success
    pub(crate) created_dir: Option<String>,
}

impl ProjectWizard {
    pub fn new(entry_kind: EntryKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("e.g. Aderman Family Collection")
        });
        let sheet_link_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("docs.google.com/spreadsheets/d/…")
        });
        let link_pattern_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("e.g. {id}_*.jpg")
        });

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
            csv_path: String::new(),
            csv_preview: None,
            csv_error: None,
            folder_path: String::new(),
            folder_match: None,
            folder_error: None,
            sheet_link_input,
            sheet_check: None,
            sheet_error: None,
            link_method: LinkMethod::ExactFilename,
            link_pattern_input,
            show_advanced_pattern: false,
            column_source: ColumnSource::AutoFromSpreadsheet,
            show_advanced_mapping: false,
            recent_config_selected: 0,
            load_config_tab: LoadConfigTab::File,
            config_file_path: String::new(),
            config_preview: None,
            config_error: None,
            created_dir: None,
        }
    }

    pub(crate) fn project_name(&self, cx: &App) -> String {
        self.name_input.read(cx).value().to_string()
    }

    pub(crate) fn spreadsheet_headers(&self) -> Vec<String> {
        match self.entry_kind {
            EntryKind::Csv => self
                .csv_preview
                .as_ref()
                .map(|p| p.headers.clone())
                .unwrap_or_default(),
            EntryKind::Sheet => Vec::new(),
            EntryKind::Blank => Vec::new(),
        }
    }

    fn breadcrumb_items(&self) -> Vec<(&'static str, WizardStep)> {
        match self.entry_kind {
            EntryKind::Blank => vec![
                ("Name", WizardStep::Name),
                ("Create", WizardStep::Review),
            ],
            _ => vec![
                ("Name", WizardStep::Name),
                ("Files", WizardStep::Files),
                ("Link", WizardStep::Link),
                ("Columns", WizardStep::Columns),
                ("Create", WizardStep::Review),
            ],
        }
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
        row = row.child(
            div()
                .text_color(cx.theme().success)
                .child("✓ Start"),
        );
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
            (_, WizardStep::Columns) => WizardStep::Link,
            (EntryKind::Blank, WizardStep::Review) => WizardStep::Name,
            (_, WizardStep::Review) => WizardStep::Columns,
            (_, WizardStep::Success) => WizardStep::Success,
        };
        cx.notify();
    }

    fn go_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step {
            WizardStep::Name => {
                if !self.validate_name(cx) {
                    cx.notify();
                    return;
                }
                self.step = if self.entry_kind == EntryKind::Blank {
                    WizardStep::Review
                } else {
                    WizardStep::Files
                };
            }
            WizardStep::Files => self.step = WizardStep::Link,
            WizardStep::Link => self.step = WizardStep::Columns,
            WizardStep::Columns => self.step = WizardStep::Review,
            WizardStep::Review => self.create_project(window, cx),
            WizardStep::Success => {}
        }
        cx.notify();
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let next_label = if self.step == WizardStep::Review {
            "Create Project"
        } else {
            "Next →"
        };
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
                Button::new("wizard-next")
                    .label(next_label)
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| this.go_next(window, cx))),
            )
    }
}

impl Render for ProjectWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let title = match self.step {
            WizardStep::Success => "qrate".into(),
            _ => "qrate — New Project".to_string(),
        };

        let body = match self.step {
            WizardStep::Name => self.render_name_step(window, cx).into_any_element(),
            WizardStep::Files => self.render_files_step(window, cx).into_any_element(),
            WizardStep::Link => self.render_link_step(window, cx).into_any_element(),
            WizardStep::Columns => self.render_columns_step(window, cx).into_any_element(),
            WizardStep::Review => self.render_review_step(window, cx).into_any_element(),
            WizardStep::Success => self.render_success_step(window, cx).into_any_element(),
        };

        let show_chrome = self.step != WizardStep::Success;
        let breadcrumb = if show_chrome {
            Some(self.render_breadcrumb(cx).into_any_element())
        } else {
            None
        };
        // The Review step renders its own big "Create Project" button and
        // centered back link (matching the prototype), so the shared footer
        // only appears on the other steps.
        let footer = if show_chrome && self.step != WizardStep::Review {
            Some(self.render_footer(cx).into_any_element())
        } else {
            None
        };

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(TitleBar::new().child(Label::new(title).font_semibold()))
            .child(
                v_flex()
                    .id("wizard-body")
                    .flex_1()
                    .p_5()
                    .gap_3()
                    .children(breadcrumb)
                    .child(body)
                    .children(footer),
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

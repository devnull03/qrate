use gpui::*;
use window_wrapper::OpenBrowser;

use crate::actions::{NewProject, Save, ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::export::{EXPORT_FORMATS, Export, ExportFormat};
use crate::theming::{SwitchTheme, theme_choices};

// The Edit menu's items act on the grid, so they're the grid's actions — `table` declares and
// handles them, and this menu only names them.
use table::{
    Clear, Copy, Cut, DeleteColumn, DeleteRow, DuplicateRow, InsertColumnLeft, InsertColumnRight,
    InsertNote, InsertRowAbove, InsertRowBelow, Paste, Redo, RenameColumn, Undo, UnfreezeColumns,
};

actions!(
    nav,
    [
        OpenProjects,
        OpenSettings,
        OpenPluginsFolder,
        ReloadPlugins,
        Quit,
        CopyDebugInfo,
        ReportIssue,
        OpenLogsFolder,
        /// What every greyed-out menu item below dispatches. Deliberately unhandled: the label is
        /// the promise, and the task named beside it is what makes the promise good.
        Planned
    ]
);

pub const REPO_URL: &str = "https://github.com/devnull03/qrate";

/// A menu item for a feature qrate doesn't have yet — greyed out, so the shape of the app is
/// visible before every part of it works. Each call site names the task that switches it on.
fn planned(name: &'static str) -> MenuItem {
    MenuItem::Action {
        name: name.into(),
        action: Box::new(Planned),
        os_action: None,
        checked: false,
        disabled: true,
    }
}

/// Installs the menu bar. Called at startup and again once the theme registry has loaded, since
/// the Theme submenu lists what the registry holds — see `theming::on_themes_loaded`.
///
/// Both, not one: `set_menus` feeds the macOS system menu bar, `set_app_menus` feeds the
/// in-window `AppMenuBar` we draw on Windows and Linux.
pub fn install(cx: &mut gpui::App) {
    cx.set_menus(app_menus(cx));
    let owned = app_menus(cx).into_iter().map(|menu| menu.owned()).collect();
    gpui_component::GlobalState::global_mut(cx).set_app_menus(owned);
}

fn app_menus(cx: &gpui::App) -> Vec<Menu> {
    vec![
        Menu {
            name: "File".into(),
            disabled: false,
            items: vec![
                MenuItem::action("New Project…", NewProject),
                MenuItem::Separator,
                MenuItem::action("Open Projects…", OpenProjects),
                MenuItem::action("Save", Save),
                MenuItem::submenu(Menu {
                    name: "Export".into(),
                    disabled: false,
                    items: EXPORT_FORMATS
                        .iter()
                        .map(|(format, label, _)| MenuItem::Action {
                            name: (*label).into(),
                            action: Box::new(Export { format: *format }),
                            os_action: None,
                            checked: false,
                            // ponytail: written but switched off until qrate has a Google OAuth
                            // client — see the "Google Sheets export" tracker task.
                            disabled: *format == ExportFormat::GoogleSheet,
                        })
                        .collect(),
                }),
                MenuItem::Separator,
                MenuItem::action("Settings", OpenSettings),
                MenuItem::Separator,
                MenuItem::action("Quit", Quit),
            ],
        },
        Menu {
            name: "Edit".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Undo", Undo),
                MenuItem::action("Redo", Redo),
                MenuItem::Separator,
                MenuItem::action("Cut", Cut),
                MenuItem::action("Copy", Copy),
                MenuItem::action("Paste", Paste),
                MenuItem::action("Clear", Clear),
                MenuItem::Separator,
                MenuItem::action("Find in Table", table::Search),
                MenuItem::action("Replace in Table", table::Replace),
            ],
        },
        Menu {
            name: "Insert".into(),
            disabled: false,
            items: vec![
                // These act on the selection: the menu bar has no clicked target to go on.
                MenuItem::action("Row Above", InsertRowAbove),
                MenuItem::action("Row Below", InsertRowBelow),
                MenuItem::action("Duplicate Row", DuplicateRow),
                MenuItem::Separator,
                MenuItem::action("Column Left", InsertColumnLeft),
                MenuItem::action("Column Right", InsertColumnRight),
                MenuItem::Separator,
                MenuItem::action("Note…", InsertNote),
            ],
        },
        Menu {
            name: "View".into(),
            disabled: false,
            items: vec![
                MenuItem::submenu(Menu {
                    name: "Theme".into(),
                    disabled: false,
                    items: theme_choices(cx)
                        .into_iter()
                        .map(|name| {
                            let action = SwitchTheme {
                                name: name.to_string(),
                            };
                            MenuItem::action(name, action)
                        })
                        .collect(),
                }),
                MenuItem::Separator,
                MenuItem::action("Toggle Left Dock", ToggleLeftDock),
                MenuItem::action("Toggle Bottom Dock", ToggleBottomDock),
                MenuItem::action("Toggle Right Dock", ToggleRightDock),
                MenuItem::Separator,
                // Freezing *to* a column needs one to point at, which is the column header's
                // menu; only the release is a global command.
                MenuItem::action("Unfreeze All Columns", UnfreezeColumns),
                // ASNT-44: cells are single-line until the row height can follow the text.
                planned("Wrap Cell Text"),
                MenuItem::Separator,
                // Built from `ViewMode::ALL` so a new view appears here for free, in the same
                // order as the centre's switcher tabs and the Ctrl+N keys. Not `checked` — the
                // menus are built once at startup (`set_menus` in `main.rs`), so a tick would
                // freeze on whichever view was active then.
                MenuItem::submenu(Menu {
                    name: "Switch View".into(),
                    disabled: false,
                    items: workspace::ViewMode::ALL
                        .into_iter()
                        .map(|mode| MenuItem::action(mode.label(), workspace::ShowView { mode }))
                        .collect(),
                }),
            ],
        },
        Menu {
            name: "Data".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Column Settings…", OpenSettings),
                MenuItem::action("Rename Column…", RenameColumn),
                MenuItem::Separator,
                MenuItem::action("Delete Row", DeleteRow),
                MenuItem::action("Delete Column", DeleteColumn),
                MenuItem::Separator,
                // ASNT-79: fold several rows describing one item together, and split them back.
                planned("Merge Rows…"),
                planned("Explode Row…"),
            ],
        },
        Menu {
            name: "Extensions".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Plugins Folder", OpenPluginsFolder),
                MenuItem::action("Reload Plugins", ReloadPlugins),
                MenuItem::Separator,
                // ASNT-73: the `ai` crate has the traits and a mock; no provider answers yet.
                planned("AI Review…"),
            ],
        },
        Menu {
            name: "Help".into(),
            disabled: false,
            items: vec![
                MenuItem::submenu(Menu {
                    name: "GitHub".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::action(
                            "Repository",
                            OpenBrowser {
                                url: REPO_URL.into(),
                            },
                        ),
                        MenuItem::action(
                            "Issues",
                            OpenBrowser {
                                url: format!("{REPO_URL}/issues"),
                            },
                        ),
                        MenuItem::action(
                            "Pull Requests",
                            OpenBrowser {
                                url: format!("{REPO_URL}/pulls"),
                            },
                        ),
                    ],
                }),
                MenuItem::action(
                    "Releases",
                    OpenBrowser {
                        url: "https://qrate.dvnl.work/releases".into(),
                    },
                ),
                MenuItem::Separator,
                // Unit actions, not `OpenBrowser { url }`: this runs once at startup, while the
                // debug info those items carry changes with every project opened and plugin loaded.
                MenuItem::action("Copy Debug Info", CopyDebugInfo),
                MenuItem::action("Report an Issue", ReportIssue),
                MenuItem::action("Open Logs Folder", OpenLogsFolder),
            ],
        },
    ]
}

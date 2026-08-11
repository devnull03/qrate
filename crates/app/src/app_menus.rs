use gpui::*;
use window_wrapper::OpenBrowser;

use crate::actions::{NewProject, Save, ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::export::{EXPORT_FORMATS, Export, ExportFormat};
use crate::theming::{SwitchTheme, THEME_CHOICES};

// The Edit menu's items act on the grid, so they're the grid's actions — `table` declares and
// handles them, and this menu only names them.
use table::{Clear, Copy, Cut, Paste, Redo, Undo, UnfreezeColumns};

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
        OpenLogsFolder
    ]
);

pub const REPO_URL: &str = "https://github.com/devnull03/qrate";

pub fn app_menus() -> Vec<Menu> {
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
            ],
        },
        Menu {
            name: "View".into(),
            disabled: false,
            items: vec![
                MenuItem::submenu(Menu {
                    name: "Theme".into(),
                    disabled: false,
                    items: THEME_CHOICES
                        .iter()
                        .map(|name| {
                            MenuItem::action(
                                *name,
                                SwitchTheme {
                                    name: name.to_string(),
                                },
                            )
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
            ],
        },
        Menu {
            name: "Data".into(),
            disabled: false,
            items: vec![MenuItem::action("Column Settings…", OpenSettings)],
        },
        Menu {
            name: "Extensions".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Plugins Folder", OpenPluginsFolder),
                MenuItem::action("Reload Plugins", ReloadPlugins),
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

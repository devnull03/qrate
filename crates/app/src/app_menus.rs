use gpui::*;
use window_wrapper::OpenBrowser;

use crate::actions::{NewProject, Save, ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::theming::{SwitchTheme, THEME_CHOICES};

// ponytail: Undo/Redo/Cut/Copy/Paste are declared and wired into the Edit menu but have no
// handlers yet — they dispatch to nothing (harmless no-ops) until real edit actions exist.
actions!(
    nav,
    [
        OpenProjects,
        OpenSettings,
        OpenPluginsFolder,
        ReloadPlugins,
        Quit,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste
    ]
);

const REPO_URL: &str = "https://github.com/devnull03/qrate";

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
                MenuItem::Separator,
                MenuItem::action("Settings", OpenSettings),
                MenuItem::Separator,
                MenuItem::action("Plugins Folder", OpenPluginsFolder),
                MenuItem::action("Reload Plugins", ReloadPlugins),
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
            ],
        },
    ]
}

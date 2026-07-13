use gpui::*;

use crate::actions::NewProject;
use crate::theming::{SwitchTheme, THEME_CHOICES};

actions!(nav, [OpenProjects, OpenSettings, Quit]);

pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "App".into(),
            items: vec![
                MenuItem::action("New Project…", NewProject),
                MenuItem::action("Projects…", OpenProjects),
                MenuItem::action("Settings", OpenSettings),
                MenuItem::Separator,
                MenuItem::action("Quit", Quit),
            ],
        },
        Menu {
            name: "Theme".into(),
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
        },
    ]
}

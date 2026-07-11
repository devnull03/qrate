use gpui::*;

use crate::actions::NewProject;

actions!(nav, [OpenSettings, Quit]);

pub fn app_menus() -> Vec<Menu> {
    vec![Menu {
        name: "App".into(),
        items: vec![
            MenuItem::action("New Project…", NewProject),
            MenuItem::action("Settings", OpenSettings),
            MenuItem::Separator,
            MenuItem::action("Quit", Quit),
        ],
    }]
}

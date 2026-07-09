use gpui::*;

use crate::debug::DebugOpenBoth; // DEBUG (ASNT-13 manual test scaffolding)

actions!(nav, [OpenSettings, Quit]);

pub fn app_menus() -> Vec<Menu> {
    vec![Menu {
        name: "App".into(),
        items: vec![
            MenuItem::action("Settings", OpenSettings),
            // DEBUG (ASNT-13 manual test scaffolding): remove once ASNT-13 is verified.
            MenuItem::action("Debug: Open Settings + Project", DebugOpenBoth),
            MenuItem::Separator,
            MenuItem::action("Quit", Quit),
        ],
    }]
}

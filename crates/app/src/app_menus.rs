use gpui::*;

actions!(nav, [OpenSettings, Quit]);

pub fn app_menus() -> Vec<Menu> {
    vec![Menu {
        name: "App".into(),
        items: vec![
            MenuItem::action("Settings", OpenSettings),
            MenuItem::Separator,
            MenuItem::action("Quit", Quit),
        ],
    }]
}

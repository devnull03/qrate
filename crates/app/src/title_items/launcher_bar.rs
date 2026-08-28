//! The launcher window's title-bar control: updater state, and the one menu a window with no
//! project open still needs.
//!
//! It lives here rather than in `project_wizard` because every entry in it is an `app` action, and
//! that crate can't depend on this one. The launcher renders whatever
//! `LauncherHooks::title_items` hands it — the same inversion `open_main_window` already uses.

use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::{IconName, Sizable as _, h_flex};

use window_wrapper::OpenBrowser;

use crate::actions::NewProject;
use crate::app_menus::{
    CopyDebugInfo, OpenAbout, OpenLogsFolder, OpenPluginsFolder, OpenSettings, REPO_URL,
    ReloadPlugins, ReportIssue,
};
use crate::title_items::update_notice::UpdateNotice;

pub struct LauncherBar {
    update: Entity<UpdateNotice>,
}

impl LauncherBar {
    /// Built fresh per launcher window: the update notice is a view, and a view rendered by two
    /// windows at once is a bug waiting for the second one to close.
    pub fn view(cx: &mut App) -> AnyView {
        let update = cx.new(UpdateNotice::new);
        cx.new(|_| Self { update }).into()
    }
}

impl Render for LauncherBar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // The full menu bar belongs to a window with a project in it — File, Edit and Insert have
        // nothing to act on here. What's left is the app itself: what it can open, what it runs,
        // and where to report it when it misbehaves.
        h_flex()
            .gap_1()
            .items_center()
            .child(self.update.clone())
            .child(
                Button::new("launcher-menu")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Menu)
                    .tooltip("qrate")
                    .dropdown_menu(|menu, _, _| {
                        menu.menu("New Project…", Box::new(NewProject))
                            .menu("Settings…", Box::new(OpenSettings))
                            .item(
                                PopupMenuItem::new("Check for Updates")
                                    .on_click(|_, _, cx| crate::update_check::check_now(cx)),
                            )
                            .separator()
                            .menu("Plugins Folder", Box::new(OpenPluginsFolder))
                            .menu("Reload Plugins", Box::new(ReloadPlugins))
                            .separator()
                            .menu(
                                "Repository",
                                Box::new(OpenBrowser {
                                    url: REPO_URL.into(),
                                }),
                            )
                            .menu(
                                "Releases",
                                Box::new(OpenBrowser {
                                    url: "https://qrate.dvnl.work/releases".into(),
                                }),
                            )
                            .separator()
                            .menu("Copy Debug Info", Box::new(CopyDebugInfo))
                            .menu("Report an Issue", Box::new(ReportIssue))
                            .menu("Open Logs Folder", Box::new(OpenLogsFolder))
                            .separator()
                            .menu(
                                format!("Version {}", env!("CARGO_PKG_VERSION")),
                                Box::new(OpenAbout),
                            )
                    }),
            )
    }
}

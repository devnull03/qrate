//! The launcher window's title-bar control: updater state, and the one menu a window with no
//! project open still needs.
//!
//! It lives here rather than in `project_wizard` because every entry in it is an `app` action, and
//! that crate can't depend on this one. The launcher renders whatever
//! `LauncherHooks::title_items` hands it — the same inversion `open_main_window` already uses.

use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::{Sizable as _, h_flex};

use window_wrapper::OpenBrowser;

use crate::actions::NewProject;
use crate::app_menus::{
    CopyDebugInfo, DiscoverPlugins, ManagePlugins, OpenAbout, OpenLogsFolder, OpenPluginsFolder,
    OpenSettings, REPO_URL, ReloadPlugins, ReportBug, ReportUxIssue, RequestFeature,
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
        // One menu with submenus, since `AppMenuBar` can only show the main window's menu list.
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                Button::new("launcher-menu")
                    .small()
                    .py_0p5()
                    .compact()
                    .ghost()
                    .label("qrate")
                    .dropdown_menu(|menu, window, cx| {
                        menu.menu("New Project…", Box::new(NewProject))
                            .menu("Settings…", Box::new(OpenSettings))
                            .separator()
                            .submenu("Plugins", window, cx, |menu, _, _| {
                                menu.menu("Discover Plugins…", Box::new(DiscoverPlugins))
                                    .menu("Manage Plugins…", Box::new(ManagePlugins))
                                    .separator()
                                    .menu("Plugins Folder", Box::new(OpenPluginsFolder))
                                    .menu("Reload Plugins", Box::new(ReloadPlugins))
                            })
                            .submenu("Help", window, cx, |menu, window, cx| {
                                menu.menu(
                                    "Repository",
                                    Box::new(OpenBrowser {
                                        url: REPO_URL.into(),
                                    }),
                                )
                                .menu(
                                    "Releases",
                                    Box::new(OpenBrowser {
                                        url: crate::site::url("/releases"),
                                    }),
                                )
                                .separator()
                                .menu("Copy Debug Info", Box::new(CopyDebugInfo))
                                .submenu("Send Feedback", window, cx, |menu, _, _| {
                                    menu.menu("Report a Bug…", Box::new(ReportBug))
                                        .menu("Request a Feature…", Box::new(RequestFeature))
                                        .menu("Report a UI/UX Issue…", Box::new(ReportUxIssue))
                                })
                                .menu("Open Logs Folder", Box::new(OpenLogsFolder))
                            })
                            .separator()
                            .item(
                                PopupMenuItem::new("Check for Updates")
                                    .on_click(|_, _, cx| crate::update_check::check_now(cx)),
                            )
                            .menu(
                                format!("Version {}", env!("CARGO_PKG_VERSION")),
                                Box::new(OpenAbout),
                            )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("launcher-feedback")
                            .small()
                            .compact()
                            .ghost()
                            .label("Feedback")
                            .occlude()
                            .on_click(|_, _, cx| {
                                cx.open_url(&crate::logging::feedback_url(cx, None))
                            }),
                    )
                    .child(self.update.clone()),
            )
    }
}

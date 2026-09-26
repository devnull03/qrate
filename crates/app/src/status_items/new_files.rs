//! Left status-bar button for files that turned up in the files folder with no row linking them.
//! Clicking it opens the import prompt for exactly those files. Hidden while there are none.

use gpui::*;
use gpui_component::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use table::{NewFiles, TablePanelHandle};

pub struct NewFilesButton {
    _sub: Subscription,
}

impl NewFilesButton {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            _sub: cx.observe_global::<NewFiles>(|_, cx| cx.notify()),
        }
    }

    pub fn occupied(cx: &App) -> bool {
        cx.try_global::<NewFiles>()
            .is_some_and(|new_files| !new_files.paths.is_empty())
    }
}

impl Render for NewFilesButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = cx
            .try_global::<NewFiles>()
            .map_or(0, |new_files| new_files.paths.len());
        if count == 0 {
            return div().into_any_element();
        }
        Button::new("new-files")
            .ghost()
            .xsmall()
            .icon(IconName::FolderOpen)
            .label(format!("New files ({count})"))
            .tooltip("Files in the files folder that no row links to")
            .on_click(|_, window, cx| {
                if let Some(table) = cx
                    .try_global::<TablePanelHandle>()
                    .and_then(|handle| handle.0.upgrade())
                {
                    table.update(cx, |table, cx| table.import_new_files(window, cx));
                }
            })
            .into_any_element()
    }
}

//! Stage 6 · Success.

use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt, v_flex};

use crate::launcher;
use crate::wizard::ProjectWizard;

impl ProjectWizard {
    pub(crate) fn render_success_step(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = self.project_name(cx);
        v_flex()
            .items_center()
            .text_center()
            .gap_2()
            .py_6()
            .child(
                div()
                    .size_12()
                    .rounded_full()
                    .border_2()
                    .border_color(cx.theme().success)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().success)
                    .child(Icon::new(IconName::Check)),
            )
            .child(div().text_lg().font_semibold().child("Your project is ready"))
            .child(
                Label::new(format!("{name} is set up and ready to go."))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                Button::new("open-project")
                    .label("Open Project")
                    .primary()
                    .on_click(cx.listener(|_this, _, window, cx| {
                        if let Some(hooks) = cx.try_global::<launcher::LauncherHooks>().copied() {
                            (hooks.open_main_window)(cx);
                        }
                        window.remove_window();
                    })),
            )
            .child(
                div()
                    .id("create-another")
                    .cursor_pointer()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Create another")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let entry_kind = this.entry_kind;
                        window.remove_window();
                        crate::wizard::open_project_wizard(entry_kind, cx);
                    })),
            )
    }
}

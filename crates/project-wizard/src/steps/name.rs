//! Stage 2 · Name — every entry path goes through this step.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt, h_flex, v_flex};

use crate::project;
use crate::wizard::ProjectWizard;

impl ProjectWizard {
    pub(crate) fn validate_name(&mut self, cx: &App) -> bool {
        let name = self.project_name(cx);
        if name.trim().is_empty() {
            self.name_error = Some("Give your project a name to continue".into());
            return false;
        }
        // ponytail: fs .exists() runs per keystroke via the live subscription;
        // debounce if it ever stutters.
        if project::name_taken(&self.save_path, &name) {
            self.name_error = Some(
                "This name is already taken — try another, or pick a different save folder".into(),
            );
            return false;
        }
        self.name_error = None;
        true
    }

    pub(crate) fn render_name_step(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(div().text_lg().font_semibold().child("Name your project"))
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Project name").text_sm())
                    .child(Input::new(&self.name_input).w_full()),
            )
            .when_some(self.name_error.clone(), |el, err| {
                el.child(
                    h_flex()
                        .gap_1()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().danger)
                        .bg(cx.theme().danger.opacity(0.06))
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(Icon::new(IconName::TriangleAlert))
                        .child(err),
                )
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Save project to").text_sm())
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .px_2()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if self.save_path.is_empty() {
                                        "Choose a folder…".to_string()
                                    } else {
                                        self.save_path.clone()
                                    }),
                            )
                            .child(
                                gpui_component::button::Button::new("browse-save-path")
                                    .label("Browse…")
                                    .outline()
                                    .on_click(cx.listener(|_this, _, _, cx| {
                                        let receiver = cx.prompt_for_paths(PathPromptOptions {
                                            files: false,
                                            directories: true,
                                            multiple: false,
                                            prompt: Some(
                                                "Choose where to save this project".into(),
                                            ),
                                        });
                                        cx.spawn(async move |this, cx| {
                                            if let Ok(Ok(Some(paths))) = receiver.await
                                                && let Some(path) = paths.first()
                                            {
                                                let s = path.to_string_lossy().to_string();
                                                this.update(cx, |this, cx| {
                                                    this.save_path = s;
                                                    this.name_error = None;
                                                    cx.notify();
                                                })
                                                .ok();
                                            }
                                        })
                                        .detach();
                                    })),
                            ),
                    )
                    .child(
                        Label::new("This is where your project file and settings live.")
                            .text_sm()
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
    }
}

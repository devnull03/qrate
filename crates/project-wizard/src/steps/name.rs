use std::sync::Arc;

use gpui::{prelude::FluentBuilder, *};
use gpui_component::alert::Alert;
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Sizable, Size, StyledExt, v_flex};
use settings::path_picker::PathPickerApp;

use crate::project;
use crate::wizard::ProjectWizard;

impl ProjectWizard {
    pub(crate) fn validate_name(&mut self, cx: &App) -> bool {
        let name = self.project_name(cx);
        if name.trim().is_empty() {
            self.name_error = Some("Give your project a name to continue".into());
            return false;
        }
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let want = self.save_path.clone();
        self.save_path_input.update(cx, |input, cx| {
            if input.value() != want {
                input.set_value(want, window, cx);
            }
        });

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
                el.child(Alert::error("name-error", err).small())
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Save project to").text_sm())
                    .child(PathPickerApp {
                        field_size: Size::Medium,
                        button_size: None,
                        button_id: "browse-save-path".into(),
                        files: false,
                        directories: true,
                        prompt: "Choose where to save this project".into(),
                        input: self.save_path_input.clone(),
                        on_pick: {
                            let this = cx.entity().downgrade();
                            Arc::new(move |path, cx| {
                                this.update(cx, |this, cx| {
                                    this.save_path = path.to_string();
                                    this.name_error = None;
                                    cx.notify();
                                })
                                .ok();
                            })
                        },
                    })
                    .child(
                        Label::new("This is where your project file and settings live.")
                            .text_sm()
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
    }
}

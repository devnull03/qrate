use gpui::{prelude::FluentBuilder, *};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState},
    label::Label,
    v_flex,
};

#[derive(IntoElement)]
pub struct LabeledInput {
    label: SharedString,
    description: SharedString,
    input: Entity<InputState>,
    disabled: bool,
}

impl LabeledInput {
    pub fn new(label: impl Into<SharedString>, input: &Entity<InputState>) -> Self {
        Self {
            label: label.into(),
            description: SharedString::default(),
            input: input.clone(),
            disabled: false,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = description.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for LabeledInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .gap_1()
            .justify_start()
            .child(Label::new(self.label).text_sm())
            .child(Input::new(&self.input).disabled(self.disabled).w_full())
            .when(!self.description.is_empty(), |s| {
                s.child(
                    Label::new(self.description)
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                )
            })
    }
}

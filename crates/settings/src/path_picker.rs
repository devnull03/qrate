//! Path row with a readonly [`Input`] plus browse (same layout as Settings `FilePicker`).
//!
//! Uses [`RenderOnce`] + [`IntoElement`] (see `window-wrapper` `AppTitleBar`).
//!
//! Settings file pickers must use [`Window::use_keyed_state`] (see gpui-component `StringField`) so
//! [`InputState`] is scoped to the settings window — not a process-wide cache.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    AxisExt as _, IconName, Sizable, Size,
    button::Button,
    h_flex,
    input::{Input, InputState},
    setting::{SettingField, SettingItem},
};

use crate::AppSettings;

/// Callback after the user picks a path in settings (`PathPickerApp`).
pub type AppPathPickFn = Arc<dyn Fn(SharedString, &mut App) + Send + Sync>;

#[derive(IntoElement)]
pub struct PathPickerApp {
    pub layout: Axis,
    pub field_size: Size,
    pub button_size: Option<Size>,
    pub button_id: SharedString,
    pub files: bool,
    pub directories: bool,
    pub prompt: SharedString,
    pub input: Entity<InputState>,
    pub on_pick: AppPathPickFn,
}

impl RenderOnce for PathPickerApp {
    fn render(self, _: &mut Window, _cx: &mut App) -> impl IntoElement {
        let prompt = self.prompt.clone();
        let files = self.files;
        let directories = self.directories;
        let mut btn = Button::new(self.button_id)
            .icon(IconName::FolderOpen)
            .outline();

        btn = match self.button_size {
            Some(s) => btn.with_size(s),
            None => btn.small(),
        };

        let on_pick = Arc::clone(&self.on_pick);

        let layout = self.layout;
        let field_size = self.field_size;

        h_flex()
            .gap_2()
            .w_full()
            .child(
                Input::new(&self.input)
                    .disabled(true)
                    .with_size(field_size)
                    .map(move |this| {
                        if layout.is_horizontal() {
                            this.w_64()
                        } else {
                            this.w_full()
                        }
                    }),
            )
            .child(btn.on_click(move |_, _, cx| {
                let receiver = cx.prompt_for_paths(PathPromptOptions {
                    files,
                    directories,
                    multiple: false,
                    prompt: Some(prompt.clone()),
                });
                let on_pick = Arc::clone(&on_pick);
                cx.spawn(async move |cx| {
                    if let Ok(Ok(Some(paths))) = receiver.await
                        && let Some(path) = paths.first()
                    {
                        let s: SharedString = path.to_string_lossy().to_string().into();
                        cx.update(|cx| on_pick(s, cx)).ok();
                    }
                })
                .detach();
            }))
    }
}

#[derive(IntoElement)]
pub struct PathPickerBrowseRow<B: IntoElement + 'static> {
    pub input: Entity<InputState>,
    pub browse: B,
}

impl<B: IntoElement + 'static> RenderOnce for PathPickerBrowseRow<B> {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(Input::new(&self.input).disabled(true).w_full()),
            )
            .child(self.browse)
    }
}

pub fn picker_with_path_button(
    key: &'static str,
    label: &'static str,
    description: &'static str,
    prompt: &'static str,
    path_candidates: Vec<&'static str>,
) -> SettingItem {
    let prompt: SharedString = prompt.into();
    SettingItem::new(
        label,
        SettingField::render(move |options, window, cx| {
            let want = AppSettings::get(cx)
                .values
                .get(key)
                .map(|v| v.text())
                .unwrap_or_default();

            let input = window.use_keyed_state(
                SharedString::from(format!(
                    "path-picker-pathbtn-{}-{}-{}",
                    options.page_ix, options.group_ix, options.item_ix
                )),
                cx,
                |window, cx| {
                    InputState::new(window, cx)
                        .placeholder("No file selected...")
                        .default_value(want.clone())
                },
            );

            input.update(cx, |state, cx| {
                if state.value() != want {
                    state.set_value(want.to_string(), window, cx);
                }
            });

            let on_pick_key = key;
            let on_path_key = key;
            let path_candidates = path_candidates.clone();

            h_flex()
                .gap_2()
                .w_full()
                .child(PathPickerApp {
                    layout: options.layout,
                    field_size: options.size,
                    button_size: Some(options.size),
                    button_id: SharedString::from(format!("browse-{}", key)),
                    files: true,
                    directories: false,
                    prompt: prompt.clone(),
                    input: input.clone(),
                    on_pick: std::sync::Arc::new(move |val, cx| {
                        AppSettings::set_text(on_pick_key, val, cx);
                    }),
                })
                .child(
                    Button::new(SharedString::from(format!("get-from-path-{}", key)))
                        .outline()
                        .icon(IconName::Redo2)
                        .tooltip("Get from PATH")
                        .with_size(options.size)
                        .on_click(move |_, _, cx| {
                            if let Some(p) = crate::find_on_path(&path_candidates) {
                                AppSettings::set_text(
                                    on_path_key,
                                    p.to_string_lossy().to_string().into(),
                                    cx,
                                );
                            }
                        }),
                )
        }),
    )
    .description(description)
}

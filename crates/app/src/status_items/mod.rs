use gpui::*;
use gpui_component::{IconName, Sizable, button::Button};
use settings::{AppSettings, load_app_settings};
use window_wrapper::status_bar::StatusBarRegistry;

pub fn build_status_bar_registry(cx: &mut App) -> StatusBarRegistry {
    let mut registry = StatusBarRegistry::new();

    registry.add_right(cx.new(|_| ReloadConfigs));
    registry.add_right(cx.new(|_| OpenTerminal));

    registry
}

pub struct OpenTerminal;

impl Render for OpenTerminal {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(
            Button::new("Open Terminal")
                .icon(IconName::SquareTerminal)
                .label("Open Terminal")
                .small()
                .px_0()
                .cursor_pointer(),
        )
    }
}

pub struct ReloadConfigs;

impl Render for ReloadConfigs {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(
            Button::new("reload-configs")
                .icon(IconName::Redo2)
                .label("Reload")
                .small()
                .px_0()
                .cursor_pointer()
                .tooltip("Reload settings from disk")
                .on_click(cx.listener(|_, _, _, cx| {
                    let current_bounds = AppSettings::get(cx).main_window_bounds.clone();
                    cx.spawn(async move |_this, cx| {
                        let loaded = cx
                            .background_executor()
                            .spawn(async move { load_app_settings().unwrap_or_default() })
                            .await;
                        cx.update(|cx| {
                            let mut new_settings = loaded;
                            new_settings.main_window_bounds = current_bounds;
                            cx.set_global(new_settings);
                        })
                        .ok();
                    })
                    .detach();
                })),
        )
    }
}

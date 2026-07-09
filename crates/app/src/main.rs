mod actions;
mod app_menus;
mod app_settings;
mod components;
mod helpers;
mod status_items;
mod title_items;

use gpui::*;
use gpui_component::{Root, TitleBar, v_flex};
use settings::{
    AppSettings, MainWindowBounds, SettingsPersistence, SettingsWindow, SettingsWriter,
    load_app_settings,
};
use window_wrapper::{
    OpenBrowser, WindowLock, WindowRegistry, status_bar::StatusBar, title_bar::AppTitleBar,
};

const SETTINGS_WINDOW_KIND: &str = "settings";

use crate::app_settings::build_pages;
use crate::{
    actions::{ToggleBottomDock, ToggleLeftDock, ToggleRightDock},
    app_menus::{OpenSettings, Quit, app_menus},
    status_items::build_status_bar_registry,
    title_items::build_title_bar_registry,
};
use gpui_component::dock::DockPlacement;
use workspace::Workspace;

pub struct App {
    workspace: Entity<Workspace>,
    status_bar: Entity<StatusBar>,
    _main_window_bounds_sub: Subscription,
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        let status_bar = cx.new(|_| StatusBar::new());

        let dock = workspace.read(cx).dock_area();
        let status_registry = build_status_bar_registry(&mut *cx, dock.clone());
        cx.set_global(status_registry);
        let title_registry = build_title_bar_registry(&mut *cx, dock);
        cx.set_global(title_registry);

        let _main_window_bounds_sub = cx.observe_window_bounds(window, |_, window, cx| {
            let b = MainWindowBounds::capture_from_window(window, cx);
            AppSettings::update(cx, |s| {
                s.main_window_bounds = Some(b);
            });
        });

        // Block the OS close button while a background task is running.
        window.on_window_should_close(cx, |_, cx| !WindowLock::is_locked(cx));

        Self {
            workspace,
            status_bar,
            _main_window_bounds_sub,
        }
    }

    fn toggle_dock(
        &mut self,
        placement: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |ws, cx| ws.toggle_dock(placement, window, cx));
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);

        div()
            .size_full()
            // Dock toggles are handled here on the root so the shortcuts work window-wide,
            // regardless of which panel currently holds focus.
            .key_context("qrate")
            .on_action(cx.listener(|this, _: &ToggleLeftDock, window, cx| {
                this.toggle_dock(DockPlacement::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleBottomDock, window, cx| {
                this.toggle_dock(DockPlacement::Bottom, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRightDock, window, cx| {
                this.toggle_dock(DockPlacement::Right, window, cx)
            }))
            .child(
                v_flex()
                    .size_full()
                    .child(AppTitleBar::new())
                    .child(
                        div()
                            .id("window-body")
                            .w_full()
                            .flex_1()
                            .child(self.workspace.clone()),
                    )
                    .child(self.status_bar.clone()),
            )
            .children(dialog_layer)
    }
}

fn main() {
    let app = Application::new().with_assets(gpui_component_assets::Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        cx.set_global(WindowLock::default());

        // Settings ------------------------------------
        let settings = load_app_settings().unwrap_or_default();
        cx.set_global(settings);
        let (main_bounds, main_display) = AppSettings::get(cx).main_window_startup_placement(cx);
        cx.set_global(SettingsPersistence {
            writer: Some(SettingsWriter::start()),
        });
        cx.set_global(WindowRegistry::default());

        cx.on_action(|_: &OpenSettings, cx| {
            if WindowRegistry::focus_or_clear(SETTINGS_WINDOW_KIND, cx) {
                return;
            }
            let bounds = Bounds::centered(None, size(px(1000.0), px(800.0)), cx);
            let window_options = WindowOptions {
                titlebar: Some(TitleBar::title_bar_options()),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(Size::new(px(600.0), px(400.0))),
                ..Default::default()
            };

            cx.spawn(async move |cx| {
                let result = cx.open_window(window_options, |window, cx| {
                    let view = cx.new(|cx| SettingsWindow::new(window, cx, build_pages));
                    cx.new(|cx| Root::new(view, window, cx))
                });

                if let Ok(window_handle) = result {
                    cx.update(|cx| {
                        WindowRegistry::register(SETTINGS_WINDOW_KIND, window_handle.into(), cx);
                    })
                    .ok();
                }
            })
            .detach();
        });
        // ----------------------------------------------

        cx.set_menus(app_menus());

        // Keyboard shortcuts (Layer 1). See `actions.rs` to add more.
        cx.bind_keys(actions::key_bindings());
        actions::register_global_handlers(cx);

        cx.on_action(|action: &OpenBrowser, cx| {
            cx.open_url(&action.url);
        });

        cx.on_action(|_: &Quit, cx| {
            cx.quit();
        });
        let min_size = Size::new(px(520.0), px(300.0));

        let window_options = WindowOptions {
            titlebar: Some(TitleBar::title_bar_options()),
            window_bounds: Some(WindowBounds::Windowed(main_bounds)),
            display_id: main_display,
            window_min_size: Some(min_size),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| App::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}

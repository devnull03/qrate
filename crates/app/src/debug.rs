//! DEBUG (ASNT-13 manual test scaffolding — delete this module and its two
//! call sites in `main.rs`/`app_menus.rs` once ASNT-13's multi-window
//! behavior has been manually verified, and before ASNT-15 lands the real
//! project-creation wizard): a mock startup window with a button to open the
//! real main window, plus a menu action that opens Settings and a stub
//! "project" window together so two window kinds can be exercised live at
//! once.

use gpui::*;
use gpui_component::{Root, StyledExt, TitleBar, button::Button, label::Label, v_flex};
use settings::AppSettings;
use window_wrapper::WindowRegistry;

// Aliased: a bare `App` import here would shadow `gpui::App` (from the glob
// import above) for the whole module, since explicit imports win over globs.
use crate::App as MainWindow;
use crate::open_settings_window;

actions!(debug, [DebugOpenBoth]);

const MAIN_WINDOW_KIND: &str = "main";
const DEBUG_PROJECT_KIND: &str = "debug-project-stub";

/// The real main-window-opening body, previously inline at the end of
/// `main()`. Now reusable so the mock startup window's button can open it on
/// demand, with the same focus-existing-or-open-new dedup Settings already
/// gets from `WindowRegistry`.
pub fn open_main_window(cx: &mut App) {
    if WindowRegistry::focus_or_clear(MAIN_WINDOW_KIND, cx) {
        return;
    }

    let (main_bounds, main_display) = AppSettings::get(cx).main_window_startup_placement(cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(main_bounds)),
        display_id: main_display,
        window_min_size: Some(Size::new(px(520.0), px(300.0))),
        ..Default::default()
    };

    cx.spawn(async move |cx| {
        let result = cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| MainWindow::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });

        if let Ok(window_handle) = result {
            cx.update(|cx| {
                WindowRegistry::register(MAIN_WINDOW_KIND, window_handle.into(), cx);
            })
            .ok();
        }
    })
    .detach();
}

/// Bare-bones window standing in for the not-yet-built project-creation
/// wizard (ASNT-15) — just enough to be a second, visibly distinct window
/// kind to test alongside Settings.
struct DebugProjectStub;

impl Render for DebugProjectStub {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(TitleBar::new().child(Label::new("Project Creation (debug)").font_semibold()))
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child("Project Creation (debug stub) — ASNT-15 not built yet"),
            )
    }
}

fn open_debug_project_stub(cx: &mut App) {
    if WindowRegistry::focus_or_clear(DEBUG_PROJECT_KIND, cx) {
        return;
    }

    let bounds = Bounds::centered(None, size(px(500.0), px(300.0)), cx);
    let window_options = WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    };

    cx.spawn(async move |cx| {
        let result = cx.open_window(window_options, |window, cx| {
            let view = cx.new(|_| DebugProjectStub);
            cx.new(|cx| Root::new(view, window, cx))
        });

        if let Ok(window_handle) = result {
            cx.update(|cx| {
                WindowRegistry::register(DEBUG_PROJECT_KIND, window_handle.into(), cx);
            })
            .ok();
        }
    })
    .detach();
}

/// Registers the `DebugOpenBoth` menu action: opens Settings and the stub
/// project window at once, so their `WindowRegistry` dedup can be exercised
/// with two windows live simultaneously.
pub fn register_debug_menu_action(cx: &mut App) {
    cx.on_action(|_: &DebugOpenBoth, cx| {
        open_settings_window(cx);
        open_debug_project_stub(cx);
    });
}

/// Mock "project management" window shown at startup instead of the real
/// main window. Its one button opens the real main window and closes this
/// one.
pub struct DebugMockProjectWindow;

impl Render for DebugMockProjectWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(TitleBar::new().child(Label::new("Project Management (debug)").font_semibold()))
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .child("Debug: mock project management window")
                    .child(
                        Button::new("open-main-project")
                            .label("Open Main Project")
                            .on_click(|_, window, cx| {
                                open_main_window(cx);
                                window.remove_window();
                            }),
                    ),
            )
    }
}

pub fn mock_startup_window_options(cx: &mut App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(400.0), px(200.0)), cx);
    WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    }
}

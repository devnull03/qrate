//! The browser entry point. Phase 0 of the web port: proves gpui and gpui-component render from
//! this workspace before any qrate crate is made to compile for wasm.
#![cfg(target_family = "wasm")]

use std::borrow::Cow;

use gpui::*;
use gpui_component::{ActiveTheme as _, Root, StyledExt as _, h_flex, v_flex};
use wasm_bindgen::prelude::*;

const FONTS: [&[u8]; 2] = [
    include_bytes!("../fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../fonts/IBMPlexSans-SemiBold.ttf"),
];

thread_local! {
    static APP: std::cell::OnceCell<ApplicationHandle> = const { std::cell::OnceCell::new() };
}

struct Spike;

impl Render for Spike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .p_4()
            .gap_2()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family("IBM Plex Sans")
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .child("qrate in the browser"),
            )
            .children((1..=5).map(|row| {
                h_flex()
                    .gap_4()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(format!("Row {row}"))
                    .child(format!("Item title {row}"))
            }))
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    let handle = gpui_platform::application()
        .with_assets(gpui_component_assets::Assets::default())
        .run_embedded(|cx| {
            let fonts = FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
            if let Err(error) = cx.text_system().add_fonts(fonts) {
                log::error!("web: could not register the bundled fonts: {error:#}");
            }
            gpui_component::init(cx);
            if let Err(error) = cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| Spike);
                cx.new(|cx| Root::new(view, window, cx))
            }) {
                log::error!("web: could not open the browser window: {error:#}");
            }
        });
    APP.with(|app| app.set(handle).ok());
}

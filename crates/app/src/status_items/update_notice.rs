//! Right-side status-bar widget: shown only while `update_check::AvailableUpdate` is set. Clicking
//! the text opens the release's platform installer; the close button dismisses it for that
//! version.

use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{IconName, Sizable as _};

use crate::update_check::AvailableUpdate;

pub struct UpdateNotice {
    _sub: Subscription,
}

impl UpdateNotice {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            _sub: cx.observe_global::<AvailableUpdate>(|_, cx| cx.notify()),
        }
    }

    pub fn occupied(cx: &App) -> bool {
        cx.has_global::<AvailableUpdate>()
    }
}

impl Render for UpdateNotice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(update) = cx.try_global::<AvailableUpdate>().cloned() else {
            return div();
        };
        let url = update.download_url.clone();
        div().child(
            gpui_component::h_flex()
                .id("update-notice")
                .gap_1()
                .items_center()
                .cursor_pointer()
                .on_click(move |_, _, cx| cx.open_url(&url))
                .child(format!("Update available: v{}", update.version))
                .child(
                    Button::new("dismiss-update")
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .on_click(|_, _, cx| {
                            crate::update_check::dismiss(cx);
                        }),
                ),
        )
    }
}

//! Debug status-bar button: fills the center table with fake rows so the virtualized table can
//! be exercised. Modeled on `workspace::DockToggleButton` (raw `h_flex`, not the library Button,
//! which hardcodes `cursor_default`).

use gpui::*;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable, h_flex};
use table::TableStateHandle;

/// How many fake rows one click appends.
const FAKE_ROWS: usize = 2000;

pub struct FakeDataButton;

impl FakeDataButton {
    pub fn new() -> Self {
        Self
    }
}

impl Render for FakeDataButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hover_bg = cx.theme().secondary_hover;

        h_flex()
            .id("status-fake-data")
            .gap_1()
            .px(px(4.))
            .py(px(2.))
            .rounded_md()
            .cursor_pointer()
            .hover(move |this| this.bg(hover_bg))
            .child(Icon::new(IconName::Plus).small())
            .child("fake data")
            .on_click(|_, _window, cx| {
                // Read the handle fresh at click-time (don't cache a weak) so a rebuilt center
                // panel is still driven correctly.
                let Some(weak) = cx.try_global::<TableStateHandle>().map(|h| h.0.clone()) else {
                    return;
                };
                weak.update(cx, |state, cx| {
                    state.delegate_mut().fill_fake(FAKE_ROWS);
                    state.refresh(cx); // recompute virtual-list sizing for the new row count
                    cx.notify();
                })
                .ok();
            })
    }
}

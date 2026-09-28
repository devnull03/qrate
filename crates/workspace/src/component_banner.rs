//! The prompt to install an optional part, shown where the need shows: a PDF or a video opened
//! without the part that draws it, or the assistant with no runtime. Never per thumbnail, so a
//! gallery of PDFs does not nag. Settings ▸ Components is the way back in after "Not now".

use std::collections::HashSet;

use components::{ComponentId, State};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Global, IntoElement as _, ParentElement as _, SharedString, Styled as _, div,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};

/// The parts the archivist put off for this session.
#[derive(Default)]
struct NotNow(HashSet<ComponentId>);

impl Global for NotNow {}

/// The prompt for `id`, or `None` when there is nothing to offer or it was put off. Reads the
/// manifest the first time a size is needed, so the prompt can say how big the download is.
pub(crate) fn banner(id: ComponentId, cx: &mut App) -> Option<AnyElement> {
    if cx
        .try_global::<NotNow>()
        .is_some_and(|not_now| not_now.0.contains(&id))
    {
        return None;
    }
    let state = components::state(id, cx);
    if state == (State::Missing { download: None }) {
        components::refresh(cx);
    }
    let need = match id {
        ComponentId::Pdfium => "PDF pages need PDF preview",
        ComponentId::Ffmpeg => "This file needs Video preview",
        ComponentId::Agent => "The assistant needs its runtime",
        ComponentId::Clip => "Visual search needs its model",
    };
    let size = preview::file_size;
    let busy = matches!(state, State::Downloading { .. } | State::Installing);
    let (text, action) = match state {
        State::Missing {
            download: Some(bytes),
        } => (
            format!("{need}, a {} download.", size(bytes)),
            Some("Install"),
        ),
        State::Missing { download: None } => (format!("{need}."), Some("Install")),
        State::UpdateRequired => (
            format!("{} needs an update for this version of qrate.", id.label()),
            Some("Update"),
        ),
        State::Downloading { received, total } => (
            format!(
                "Downloading {}: {} / {}",
                id.label(),
                size(received),
                size(total)
            ),
            Some("Cancel"),
        ),
        State::Installing => (format!("Installing {}…", id.label()), None),
        State::Failed(reason) => (reason.to_string(), Some("Try again")),
        State::Unavailable(reason) => (reason.to_string(), None),
        State::Bundled | State::System | State::Installed { .. } => return None,
    };
    Some(
        h_flex()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(div().flex_1().min_w_0().child(SharedString::from(text)))
            .children(action.map(|label| {
                Button::new(SharedString::from(format!("component-{}", id.name())))
                    .small()
                    .when(label != "Cancel", |button| button.primary())
                    .label(label)
                    .on_click(move |_, _, cx| match label {
                        "Cancel" => components::cancel(id, cx),
                        _ => components::install(id, cx).detach(),
                    })
            }))
            .when(!busy && id != ComponentId::Agent, |bar| {
                bar.child(
                    Button::new(SharedString::from(format!("component-{}-later", id.name())))
                        .small()
                        .ghost()
                        .label("Not now")
                        .on_click(move |_, _, cx| {
                            cx.default_global::<NotNow>().0.insert(id);
                            cx.refresh_windows();
                        }),
                )
            })
            .into_any_element(),
    )
}

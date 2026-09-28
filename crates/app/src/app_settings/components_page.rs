//! Settings ▸ Components: every optional part, where it comes from, and the buttons to install,
//! update or remove it. An entity rather than plain setting items, because a download's progress
//! changes the `Components` global, which the Settings window does not observe.

use components::{ComponentId, Removal, State};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, Context, IntoElement, ParentElement as _, PromptLevel, Render, SharedString,
    Styled as _, Subscription, Window, div,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

pub(super) struct ComponentRows {
    _sub: Subscription,
}

impl ComponentRows {
    /// Reads the manifest again each time the page opens, for current sizes.
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        components::refresh(cx);
        Self {
            _sub: cx.observe_global::<components::Components>(|_, cx| cx.notify()),
        }
    }
}

impl Render for ComponentRows {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = preview::file_size;
        let muted = cx.theme().muted_foreground;
        v_flex()
            .w_full()
            .gap_3()
            .children(ComponentId::ALL.map(|id| {
                let state = components::state(id, cx);
                let status: SharedString = match &state {
                    State::Bundled => "Included with this install".into(),
                    State::System => "Using the copy installed on this computer".into(),
                    State::Installed { version } => format!("Installed, version {version}").into(),
                    State::Missing {
                        download: Some(bytes),
                    } => format!("Not installed, a {} download", size(*bytes)).into(),
                    State::Missing { download: None } => "Not installed".into(),
                    State::Downloading { received, total } => {
                        format!("Downloading {} / {}", size(*received), size(*total)).into()
                    }
                    State::Installing => "Installing…".into(),
                    State::UpdateRequired => "Needs an update for this version of qrate".into(),
                    State::Unavailable(reason) | State::Failed(reason) => reason.clone(),
                };
                let about = match id {
                    ComponentId::Pdfium => "Draws the pages of PDFs and Illustrator files.",
                    ComponentId::Ffmpeg => {
                        "Draws video frames, and JPEG 2000, AVIF and HEIC images."
                    }
                    ComponentId::Agent => "Runs the assistant in the Agent panel.",
                    ComponentId::Clip => "Lets visual search find images by what they show.",
                };
                let install = match state {
                    State::Missing { .. } | State::Failed(_) => Some("Install"),
                    State::UpdateRequired => Some("Update"),
                    _ => None,
                };
                let removable = matches!(state, State::Installed { .. } | State::UpdateRequired);
                let downloading = matches!(state, State::Downloading { .. });
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().font_semibold().child(id.label()))
                            .child(div().text_xs().text_color(muted).child(about))
                            .child(div().text_xs().child(status)),
                    )
                    .children(install.map(|label| {
                        Button::new(SharedString::from(format!("install-{}", id.name())))
                            .small()
                            .primary()
                            .label(label)
                            .on_click(move |_, _, cx| components::install(id, cx).detach())
                    }))
                    .when(downloading, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("cancel-{}", id.name())))
                                .small()
                                .label("Cancel")
                                .on_click(move |_, _, cx| components::cancel(id, cx)),
                        )
                    })
                    .when(removable, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("remove-{}", id.name())))
                                .small()
                                .label("Remove…")
                                .on_click(move |_, window, cx| {
                                    let answer = window.prompt(
                                        PromptLevel::Warning,
                                        &format!("Remove {}?", id.label()),
                                        Some(
                                            "It can be installed again from here, or when \
                                             something first needs it.",
                                        ),
                                        &["Remove", "Cancel"],
                                        cx,
                                    );
                                    let handle = window.window_handle();
                                    cx.spawn(async move |cx| {
                                        if answer.await.unwrap_or(1) != 0 {
                                            return;
                                        }
                                        let removed = cx.update(|cx| match id {
                                            ComponentId::Clip => table::remove_visual_model(cx),
                                            _ => components::remove(id, cx),
                                        });
                                        let told = match removed {
                                            Ok(Removal::Removed) => None,
                                            Ok(Removal::AfterRestart) => Some((
                                                "Removed after restart",
                                                format!(
                                                    "{} is in use. Its files are deleted when \
                                                     qrate next starts.",
                                                    id.label()
                                                ),
                                            )),
                                            Err(err) => {
                                                log::warn!("did not remove {}: {err:#}", id.name());
                                                Some(("It was not removed", err.to_string()))
                                            }
                                        };
                                        if let Some((title, detail)) = told {
                                            let _ = cx.update_window(handle, |_, window, cx| {
                                                window.prompt(
                                                    PromptLevel::Info,
                                                    title,
                                                    Some(&detail),
                                                    &["OK"],
                                                    cx,
                                                )
                                            });
                                        }
                                        cx.update(|cx| cx.refresh_windows());
                                    })
                                    .detach();
                                }),
                        )
                    })
            }))
            .child(
                h_flex().justify_end().child(
                    Button::new("refresh-components")
                        .small()
                        .ghost()
                        .label("Check again")
                        .on_click(|_, _, cx| components::refresh(cx)),
                ),
            )
    }
}

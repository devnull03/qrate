//! Stage 3.5 · Link your files to rows. Default "match by exact filename"
//! reuses the folder match already computed in the Files step; the
//! collapsible advanced section offers a custom naming pattern.

use gpui::{prelude::FluentBuilder, *};
use gpui_component::collapsible::Collapsible;
use gpui_component::input::Input;
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt, h_flex, v_flex};

use crate::wizard::{LinkMethod, ProjectWizard, option_card};

impl ProjectWizard {
    pub(crate) fn render_link_step(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let folder = self.folder_path.clone();
        let exact_selected = self.link_method == LinkMethod::ExactFilename;
        let pattern_selected = self.link_method == LinkMethod::CustomPattern;

        v_flex()
            .gap_3()
            .child(div().text_lg().font_semibold().child("Link your files to rows"))
            .child(
                Label::new(format!(
                    "Tell qrate how each row matches a file in {}.",
                    if folder.is_empty() { "your files folder" } else { &folder }
                ))
                .text_sm()
                .text_color(cx.theme().muted_foreground),
            )
            .child(
                option_card(
                    "link-exact",
                    "Match by exact filename",
                    "Uses the \"Filename\" column — recommended for most collections.",
                    exact_selected,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.link_method = LinkMethod::ExactFilename;
                    cx.notify();
                })),
            )
            .child(match &self.folder_match {
                Some(m) => h_flex()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().success)
                    .child(Icon::new(IconName::CircleCheck))
                    .child(format!("{} of {} rows matched a file", m.matched_rows, m.total_rows))
                    .into_any_element(),
                None => div().into_any_element(),
            })
            .child(match &self.folder_match {
                Some(m) if m.extra_files > 0 => h_flex()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().warning)
                    .child(Icon::new(IconName::TriangleAlert))
                    .child(format!(
                        "{} file{} in this folder aren't mentioned in your spreadsheet — they'll be left out unless you link them",
                        m.extra_files,
                        if m.extra_files == 1 { "" } else { "s" }
                    ))
                    .into_any_element(),
                _ => div().into_any_element(),
            })
            .child(
                Collapsible::new()
                    .open(self.show_advanced_pattern)
                    .child(
                        v_flex()
                            .id("advanced-pattern-toggle")
                            .cursor_pointer()
                            .gap_1()
                            .p_2p5()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                Label::new(if self.show_advanced_pattern {
                                    "▾ Advanced: custom naming pattern (optional)"
                                } else {
                                    "▸ Advanced: custom naming pattern (optional)"
                                })
                                .text_sm(),
                            )
                            .when(!self.show_advanced_pattern, |el| {
                                el.child(
                                    Label::new("e.g. {id}_*.jpg — for files that don't share an exact name.")
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground),
                                )
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_advanced_pattern = !this.show_advanced_pattern;
                                cx.notify();
                            })),
                    )
                    .content(
                        v_flex().gap_2().mt_2().child(
                            // The pattern input lives *inside* the option card
                            // (appended to `option_card`'s own v_flex), revealed
                            // once the card is selected.
                            option_card(
                                "link-pattern",
                                "Use a custom pattern",
                                "Match filenames against a pattern instead of an exact match.",
                                pattern_selected,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.link_method = LinkMethod::CustomPattern;
                                cx.notify();
                            }))
                            .when(pattern_selected, |el| {
                                el.child(Input::new(&self.link_pattern_input).w_full())
                            }),
                        ),
                    ),
            )
    }
}

//! The four containers that render plugin-contributed bar items — status and title, left and
//! right.
//!
//! Containers rather than one view per item, because neither bar registry has a remove API or a
//! stable item id: a plugin loaded after startup could never appear, and an unloaded one could
//! never leave. These four are registered once and never change; what changes is the
//! `BarContributions` global they read, which each one observes.
//!
//! They live in `app` because a bar command's context comes from the table's selection, and `app`
//! is where both crates are already linked.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
};
use plugin_api::{Bar, BarAction, BarContributions, PluginHooks, Side};

use crate::status_items::markup::markup;

pub struct PluginBar {
    bar: Bar,
    side: Side,
    _sub: Subscription,
}

impl PluginBar {
    pub fn new(bar: Bar, side: Side, cx: &mut Context<Self>) -> Self {
        Self {
            bar,
            side,
            _sub: cx.observe_global::<BarContributions>(|_, cx| cx.notify()),
        }
    }
}

/// Run one of a plugin's commands against whatever the table has selected. The command answers on
/// the background executor, so this returns long before the plugin does.
fn run(plugin: &SharedString, command: &SharedString, cx: &mut App) {
    let Some(hooks) = cx.try_global::<PluginHooks>().copied() else {
        return;
    };
    let ctx = table::selected_context(plugin, cx);
    (hooks.invoke)(plugin, command, &ctx, cx);
}

fn entries(
    plugin: SharedString,
    actions: Vec<(SharedString, SharedString)>,
    menu: PopupMenu,
) -> PopupMenu {
    actions.into_iter().fold(menu, |menu, (label, command)| {
        let plugin = plugin.clone();
        menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| run(&plugin, &command, cx)))
    })
}

impl Render for PluginBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = window.text_style();
        gpui_component::h_flex().gap_3().children(
            BarContributions::at(self.bar, self.side, cx)
                .into_iter()
                .map(|(plugin, item)| {
                    let id: SharedString = format!("plugin-bar:{plugin}:{}", item.id).into();
                    let (left, right, text) = (item.left, item.right, item.text);

                    div()
                        .id(ElementId::Name(id))
                        // The title bar wraps its children in a drag hitbox that Windows
                        // hit-tests as the caption, which never delivers a click.
                        .occlude()
                        .text_color(cx.theme().muted_foreground)
                        .when_some(item.tooltip, |this, tip| {
                            this.tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                            })
                        })
                        .map(|this| match left {
                            // A left-click menu carries the label on its own button; every other
                            // shape puts it straight on the item.
                            Some(BarAction::Menu(actions)) => {
                                let owner = plugin.clone();
                                this.child(
                                    Button::new("open")
                                        .ghost()
                                        .child(markup(&text, &style, cx))
                                        .dropdown_menu(move |menu, _, _| {
                                            entries(owner.clone(), actions.clone(), menu)
                                        }),
                                )
                            }
                            Some(BarAction::Command(command)) => {
                                let owner = plugin.clone();
                                this.cursor_pointer()
                                    .on_click(move |_, _, cx| run(&owner, &command, cx))
                                    .child(markup(&text, &style, cx))
                            }
                            None => this.child(markup(&text, &style, cx)),
                        })
                        // `context_menu` wraps rather than extends, so the arms only agree as
                        // `AnyElement`.
                        .map(|this| match right {
                            Some(BarAction::Command(command)) => this
                                .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                                    run(&plugin, &command, cx)
                                })
                                .into_any_element(),
                            Some(BarAction::Menu(actions)) => this
                                .context_menu(move |menu, _, _| {
                                    entries(plugin.clone(), actions.clone(), menu)
                                })
                                .into_any_element(),
                            None => this.into_any_element(),
                        })
                }),
        )
    }
}

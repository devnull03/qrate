//! Optional panel-owned header stages. Providers receive a view only while its header renders;
//! the registry stores function pointers, never panel entities.

use std::collections::HashMap;

use gpui::*;

use crate::responsive_header::{HeaderMenu, HeaderStage};

pub trait PanelHeaders: Render {
    fn header_stages(
        &mut self,
        defaults: Vec<HeaderStage>,
        common_menu: HeaderMenu,
        cx: &mut Context<Self>,
    ) -> Vec<HeaderStage>;
}

type Provider = fn(AnyView, Vec<HeaderStage>, HeaderMenu, &mut App) -> Vec<HeaderStage>;

#[derive(Default)]
pub struct PanelHeaderRegistry(HashMap<&'static str, Provider>);

impl Global for PanelHeaderRegistry {}

impl PanelHeaderRegistry {
    pub fn register<T: PanelHeaders>(name: &'static str, cx: &mut App) {
        fn stages<T: PanelHeaders>(
            view: AnyView,
            defaults: Vec<HeaderStage>,
            common_menu: HeaderMenu,
            cx: &mut App,
        ) -> Vec<HeaderStage> {
            match view.downcast::<T>() {
                Ok(panel) => panel.update(cx, |panel, cx| {
                    panel.header_stages(defaults, common_menu, cx)
                }),
                Err(_) => defaults,
            }
        }
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        cx.global_mut::<Self>().0.insert(name, stages::<T>);
    }

    pub(crate) fn stages(
        name: &str,
        view: AnyView,
        defaults: Vec<HeaderStage>,
        common_menu: HeaderMenu,
        cx: &mut App,
    ) -> Vec<HeaderStage> {
        let provider = cx
            .try_global::<Self>()
            .and_then(|registry| registry.0.get(name))
            .copied();
        match provider {
            Some(provider) => provider(view, defaults, common_menu, cx),
            None => defaults,
        }
    }
}

pub mod bar;
pub mod status_bar;
pub mod title_bar;
pub mod window_registry;

pub use bar::{BarItem, BarItems, BarRegistry};
use gpui::*;
pub use gpui_component::TitleBar;
pub use window_registry::WindowRegistry;

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = this_app)]
#[serde(deny_unknown_fields)]
pub struct OpenBrowser {
    pub url: String,
}

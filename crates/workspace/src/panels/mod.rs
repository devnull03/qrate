//! Dock panels for the main workspace view. Add a new panel by creating a file here,
//! implementing `gpui_component::dock::Panel`, giving it a `PanelMeta`, and listing that meta in
//! `panel_registry::PANELS` — the layout and the status bar build themselves from there.

pub mod agent;
pub mod details;
pub mod history;

pub use agent::{
    AGENT_FONT_KEY, AGENT_FONT_SIZE_KEY, AGENT_FONT_SIZES, AGENT_META, AgentCall, AgentPanel,
    Entry as AgentEntry, record, terminal_key_bindings as agent_terminal_key_bindings,
};
pub use details::{DETAILS_META, DetailsPanel};
pub use history::{HISTORY_META, HistoryPanel};

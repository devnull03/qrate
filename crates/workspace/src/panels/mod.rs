//! Dock panels for the main workspace view. Add a new panel by creating a file here,
//! implementing `gpui_component::dock::Panel`, giving it a `PanelMeta`, and listing that meta in
//! `panel_registry::PANELS` — the layout and the status bar build themselves from there.

pub mod agent;
pub mod details;

pub use agent::{AGENT_META, AgentCall, AgentPanel, Entry as AgentEntry, record};
pub use details::{DETAILS_META, DetailsPanel};

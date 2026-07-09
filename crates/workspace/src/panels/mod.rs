//! Dock panels for the main workspace view. Add a new panel by creating a file here,
//! implementing `gpui_component::dock::Panel`, and wiring it into the layout in `lib.rs`.

pub mod agent;
pub mod details;
pub mod problems;

pub use agent::AgentPanel;
pub use details::DetailsPanel;
pub use problems::ProblemsPanel;

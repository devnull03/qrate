//! The qrate "New Project" wizard: the Photoshop-style [`launcher`] shown on
//! open, and the multi-step [`wizard`] it launches for the Blank / CSV /
//! Google Sheet entry paths. See `crates/project-wizard/../../chats/chat1.md`
//! in the design handoff bundle for the UX rationale behind each step.

mod data;
pub mod launcher;
mod project;
mod recent;
mod steps;
pub mod wizard;

pub use launcher::{LauncherHooks, open_launcher_window};
pub use wizard::{EntryKind, open_project_wizard};

//! The qrate "New Project" wizard: the Photoshop-style [`launcher`] shown on
//! open, and the multi-step [`wizard`] it launches for the Blank / local-spreadsheet /
//! Google Sheet entry paths. See `crates/project-wizard/../../chats/chat1.md`
//! in the design handoff bundle for the UX rationale behind each step.

mod column_config;
mod data;
pub mod launcher;
mod project;
mod recent;
mod steps;
pub mod wizard;

pub use column_config::open_column_config_dialog;
pub use launcher::{LauncherHooks, open_launcher_window, open_launcher_with_error};
pub use project::open_project;
pub use recent::record_opened;
pub use wizard::{EntryKind, open_project_wizard};

/// `AppSettings` key for the folder a new project is saved in: the last one used, or whatever
/// Settings ▸ Application names. Unset or missing on disk falls back to `Documents/qrate`.
pub const NEW_PROJECT_FOLDER_KEY: &str = "new_project_folder";

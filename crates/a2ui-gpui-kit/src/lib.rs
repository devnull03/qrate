//! An [A2UI](https://github.com/google/A2UI) v1.0 renderer for gpui-kit (`gpui-component`).
//!
//! The core — [`protocol`], [`surface`], [`checks`], [`pointer`] — is plain Rust with no gpui:
//! parse agent messages, apply them to a [`SurfaceState`], resolve the tree. [`render::Surface`]
//! is the gpui view over that state; it emits [`SurfaceEvent::Action`] only on a real click.

pub mod checks;
pub mod component;
pub mod error;
pub mod pointer;
pub mod protocol;
pub mod render;
pub mod surface;

pub use error::{A2uiError, ErrorCode};
pub use protocol::{ActionMessage, AgentMessage, BASIC_CATALOG_ID};
pub use render::{Surface, SurfaceEvent};
pub use surface::{Change, Limits, Node, NodeKind, SurfaceState};

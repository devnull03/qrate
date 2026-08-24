//! qrate's embedded agent process: packaged Pi discovery, isolated profile, and fixed-command PTY.

mod runtime;
mod terminal;

pub use runtime::{AgentRuntime, init};
pub use terminal::{AgentTerminal, TerminalPalette, TerminalScreen};

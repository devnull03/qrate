//! qrate's embedded agent process: packaged Pi discovery, isolated profile, and fixed-command PTY.

use std::ops::Range;

use gpui::{HighlightStyle, Hsla};

mod runtime;
#[cfg(not(target_family = "wasm"))]
mod terminal;
#[cfg(target_family = "wasm")]
#[path = "web.rs"]
mod terminal;

pub use runtime::{AgentRuntime, init};
pub use terminal::AgentTerminal;

/// The status while there is no runtime to start, until it is installed.
pub const NOT_INSTALLED: &str = "Pi is not installed. Install the assistant runtime to start it.";

#[derive(Clone, Copy, PartialEq)]
pub struct TerminalPalette {
    pub foreground: Hsla,
    pub background: Hsla,
    pub muted: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub blue: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
}

pub struct TerminalScreen {
    pub text: String,
    pub highlights: Vec<(Range<usize>, HighlightStyle)>,
    /// Where the program printed a hyperlink (OSC 8), with the exact target it named.
    pub links: Vec<(Range<usize>, String)>,
}

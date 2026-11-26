//! A browser tab cannot spawn Pi or open a PTY, so the terminal never starts there. Same surface as
//! `terminal.rs`, so the agent panel compiles unchanged and shows why.

use std::path::{Path, PathBuf};

use gpui::{App, KeyDownEvent};

use crate::{TerminalPalette, TerminalScreen};

const DESKTOP_ONLY: &str = "The assistant runs in the qrate desktop app, not in the browser.";

pub fn warm_global_credential(_: PathBuf) {}

#[derive(Default)]
pub struct AgentTerminal;

impl AgentTerminal {
    pub fn is_running(&self) -> bool {
        false
    }

    pub fn project(&self) -> Option<&Path> {
        None
    }

    pub fn status(&self) -> &str {
        DESKTOP_ONLY
    }

    pub fn start(&mut self, _: bool, _: &App) {}

    pub fn poll(&mut self) -> bool {
        false
    }

    pub fn new_session(&mut self, _: &App) {}

    pub fn stop(&mut self) {}

    pub fn input(&self, _: &KeyDownEvent, _: &App) -> bool {
        false
    }

    pub fn send(&self, _: &'static [u8]) -> bool {
        false
    }

    pub fn set_palette(&mut self, _: TerminalPalette, _: &App) {}

    pub fn resize(&mut self, _: usize, _: usize) {}

    pub fn scroll(&mut self, _: f32) {}

    pub fn screen(&self, _: TerminalPalette) -> TerminalScreen {
        TerminalScreen {
            text: String::new(),
            highlights: Vec::new(),
            links: Vec::new(),
        }
    }
}

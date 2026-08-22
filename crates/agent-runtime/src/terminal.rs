use std::borrow::Cow;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, mpsc};

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

use alacritty_terminal::event::{Event, EventListener, Notify as _, OnResize as _, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Options, Shell};
use gpui::{App, KeyDownEvent};

use crate::AgentRuntime;

const COLS: usize = 100;
const LINES: usize = 32;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ask the user's ordinary Pi installation to resolve its OpenRouter credential. The credential
/// stays in memory and is passed to the qrate-owned Pi process through its environment; it is never
/// copied into qrate's profile or included in logs or command-line arguments.
fn global_openrouter_credential(program: &std::path::Path) -> Option<String> {
    for auth_command in ["print-api-key", "print-bearer-token"] {
        let mut command = Command::new(program);
        command.args(["auth", auth_command, "--provider", "openrouter"]);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let Ok(output) = command.output() else {
            log::debug!("could not query the global Pi OpenRouter credential");
            return None;
        };
        if !output.status.success() {
            continue;
        }
        let credential = String::from_utf8(output.stdout).ok()?;
        let credential = credential.trim();
        if !credential.is_empty() {
            return Some(credential.to_owned());
        }
    }
    None
}

struct TerminalSize;

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        LINES
    }

    fn screen_lines(&self) -> usize {
        LINES
    }

    fn columns(&self) -> usize {
        COLS
    }
}

struct SizedTerminal {
    lines: usize,
    cols: usize,
}

impl Dimensions for SizedTerminal {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Clone)]
struct UiEvents(mpsc::Sender<Event>);

impl EventListener for UiEvents {
    fn send_event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

struct Session {
    terminal: Arc<FairMutex<Term<UiEvents>>>,
    notifier: Notifier,
    events: mpsc::Receiver<Event>,
    running: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

pub struct AgentTerminal {
    session: Option<Session>,
    status: String,
    scroll_remainder: f32,
}

impl Default for AgentTerminal {
    fn default() -> Self {
        Self {
            session: None,
            status: "Pi has not started.".to_owned(),
            scroll_remainder: 0.,
        }
    }
}

impl AgentTerminal {
    pub fn is_running(&self) -> bool {
        self.session.as_ref().is_some_and(|session| session.running)
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn start(&mut self, continue_previous: bool, cx: &App) {
        self.stop();
        self.scroll_remainder = 0.;
        let Some(runtime) = cx.try_global::<AgentRuntime>().cloned() else {
            self.status = "Pi is not installed in this qrate build.".to_owned();
            return;
        };
        let Some(project) = cx.try_global::<settings::project::CurrentProject>() else {
            self.status = "Open a qrate project before starting Pi.".to_owned();
            return;
        };
        let cwd = project
            .file
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        let session_dir = runtime.profile.join("sessions").join("qrate");
        if let Err(err) = std::fs::create_dir_all(&session_dir) {
            log::error!(
                "could not create embedded Pi session directory {}: {err}",
                session_dir.display()
            );
            self.status = format!("Pi's session directory could not be created: {err}");
            return;
        }

        log::info!(
            "starting embedded Pi: program={}, cwd={}, profile={}, session_dir={}, continue_previous={continue_previous}",
            runtime.program.display(),
            cwd.display(),
            runtime.profile.display(),
            session_dir.display()
        );
        log::debug!(
            "embedded Pi resources: extension={} (exists={}), skill={} (exists={}), endpoint={} (exists={})",
            runtime.extension.display(),
            runtime.extension.is_file(),
            runtime.skill.display(),
            runtime.skill.is_file(),
            runtime.endpoint.display(),
            runtime.endpoint.is_file()
        );

        let mut args = runtime.leading_args.clone();
        args.extend([
            "--provider".to_owned(),
            "openrouter".to_owned(),
            "--model".to_owned(),
            "openrouter/free".to_owned(),
            "--no-extensions".to_owned(),
            "--extension".to_owned(),
            runtime.extension.to_string_lossy().into_owned(),
            "--no-skills".to_owned(),
            "--skill".to_owned(),
            runtime.skill.to_string_lossy().into_owned(),
            "--no-context-files".to_owned(),
            "--no-approve".to_owned(),
            "--session-dir".to_owned(),
            session_dir.to_string_lossy().into_owned(),
        ]);
        if continue_previous {
            args.push("--continue".to_owned());
        }

        let mut env: HashMap<String, String> = std::env::vars().collect();
        if let Some(credential) = global_openrouter_credential(&runtime.program) {
            // A qrate-local auth.json still takes priority inside Pi. This is only a fallback to the
            // user's global Pi login, and avoids duplicating a refresh token on disk.
            env.insert("OPENROUTER_API_KEY".to_owned(), credential);
            log::info!("embedded Pi can use the global Pi OpenRouter login as an auth fallback");
        } else {
            log::info!(
                "no global Pi OpenRouter login is available; embedded Pi will use its isolated login or OpenRouter's anonymous free router"
            );
        }
        // Keep Pi's provider login under its isolated profile, but preserve the ordinary process
        // environment for TLS, proxies, locale, and platform support.
        env.extend([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                runtime.profile.to_string_lossy().into_owned(),
            ),
            ("PI_SKIP_VERSION_CHECK".to_owned(), "1".to_owned()),
            (
                "QRATE_AGENT_ENDPOINT".to_owned(),
                runtime.endpoint.to_string_lossy().into_owned(),
            ),
            (
                "QRATE_PROJECT_DIR".to_owned(),
                cwd.to_string_lossy().into_owned(),
            ),
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ]);
        let options = Options {
            shell: Some(Shell::new(
                runtime.program.to_string_lossy().into_owned(),
                args,
            )),
            working_directory: Some(cwd),
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        };
        let size = WindowSize {
            num_lines: LINES as u16,
            num_cols: COLS as u16,
            cell_width: 8,
            cell_height: 16,
        };
        let (tx, events) = mpsc::channel();
        let listener = UiEvents(tx);
        let terminal = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &TerminalSize,
            listener.clone(),
        )));
        let pty = match tty::new(&options, size, 0) {
            Ok(pty) => pty,
            Err(err) => {
                log::error!("could not create embedded Pi PTY: {err}");
                self.status = format!("Pi could not start: {err}");
                return;
            }
        };
        let event_loop = match EventLoop::new(terminal.clone(), listener, pty, true, false) {
            Ok(event_loop) => event_loop,
            Err(err) => {
                log::error!("could not create embedded Pi terminal event loop: {err}");
                self.status = format!("Pi's terminal could not start: {err}");
                return;
            }
        };
        let notifier = Notifier(event_loop.channel());
        let _thread = event_loop.spawn();
        self.session = Some(Session {
            terminal,
            notifier,
            events,
            running: true,
        });
        log::info!("embedded Pi process started");
        self.status = if continue_previous {
            "Resuming this project's Pi session…".to_owned()
        } else {
            "Started a new Pi session.".to_owned()
        };
    }

    /// Drain terminal notifications. Returns whether the panel should repaint.
    pub fn poll(&mut self) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        let mut changed = false;
        let mut exited = None;
        while let Ok(event) = session.events.try_recv() {
            changed = true;
            if let Event::ChildExit(status) = event {
                exited = Some(status);
            }
        }
        if let Some(status) = exited {
            log::warn!("embedded Pi exited with {status}");
            session.running = false;
            self.status = format!(
                "Pi exited with {status}. Its final output is preserved below; launch details are in qrate.log."
            );
        }
        changed
    }

    pub fn stop(&mut self) {
        if self.is_running() {
            log::info!("stopping embedded Pi");
        }
        self.session = None;
        self.status = "Pi is stopped.".to_owned();
    }

    pub fn input(&self, event: &KeyDownEvent, cx: &App) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        if !session.running {
            return false;
        }
        let stroke = &event.keystroke;
        let bytes = if stroke.modifiers.secondary() && stroke.key == "v" {
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .map(|text| text.into_bytes())
        } else if stroke.modifiers.control && stroke.key.len() == 1 {
            stroke
                .key
                .bytes()
                .next()
                .map(|byte| vec![byte.to_ascii_uppercase() & 0x1f])
        } else {
            match stroke.key.as_str() {
                "enter" => Some(vec![b'\r']),
                "backspace" => Some(vec![0x7f]),
                "tab" => Some(vec![b'\t']),
                "escape" => Some(vec![0x1b]),
                "home" => Some(b"\x1b[H".to_vec()),
                "end" => Some(b"\x1b[F".to_vec()),
                "pageup" => Some(b"\x1b[5~".to_vec()),
                "pagedown" => Some(b"\x1b[6~".to_vec()),
                "delete" => Some(b"\x1b[3~".to_vec()),
                "up" => Some(b"\x1b[A".to_vec()),
                "down" => Some(b"\x1b[B".to_vec()),
                "right" => Some(b"\x1b[C".to_vec()),
                "left" => Some(b"\x1b[D".to_vec()),
                _ if !stroke.modifiers.control && !stroke.modifiers.platform => stroke
                    .key_char
                    .as_ref()
                    .map(|text| text.as_bytes().to_vec()),
                _ => None,
            }
        };
        let Some(bytes) = bytes else {
            return false;
        };
        session.notifier.notify(Cow::Owned(bytes));
        true
    }

    pub fn resize(&mut self, cols: usize, lines: usize) {
        let Some(session) = &mut self.session else {
            return;
        };
        let cols = cols.max(2);
        let lines = lines.max(2);
        session
            .terminal
            .lock()
            .resize(SizedTerminal { lines, cols });
        if session.running {
            session.notifier.on_resize(WindowSize {
                num_lines: lines.min(u16::MAX as usize) as u16,
                num_cols: cols.min(u16::MAX as usize) as u16,
                cell_width: 8,
                cell_height: 16,
            });
        }
    }

    pub fn scroll(&mut self, lines: f32) {
        self.scroll_remainder += lines;
        let lines = self.scroll_remainder.trunc() as i32;
        self.scroll_remainder -= lines as f32;
        if lines == 0 {
            return;
        }

        let Some(session) = &self.session else {
            return;
        };
        let mode = *session.terminal.lock().mode();
        if session.running && mode.intersects(TermMode::MOUSE_MODE) {
            // Pi's fullscreen TUI owns its transcript viewport and enables terminal mouse
            // reporting. Send wheel reports into the transcript rather than trying to scroll the
            // alternate screen's empty native history. Row/column 1 is always above Pi's fixed
            // editor and footer.
            let button = if lines > 0 { 64 } else { 65 };
            let report = if mode.contains(TermMode::SGR_MOUSE) {
                format!("\x1b[<{button};1;1M").into_bytes()
            } else {
                vec![0x1b, b'[', b'M', button + 32, 33, 33]
            };
            for _ in 0..lines.unsigned_abs().min(12) {
                session.notifier.notify(Cow::Owned(report.clone()));
            }
        } else {
            session.terminal.lock().scroll_display(Scroll::Delta(lines));
        }
    }

    pub fn screen(&self) -> String {
        let Some(session) = &self.session else {
            return String::new();
        };
        let terminal = session.terminal.lock();
        let content = terminal.renderable_content();
        let cursor = content.cursor.point;
        let mut lines = vec![String::with_capacity(COLS); terminal.screen_lines()];
        let top = -(content.display_offset as i32);
        for indexed in content.display_iter {
            let line = indexed.point.line.0 - top;
            if let Some(output) = usize::try_from(line)
                .ok()
                .and_then(|line| lines.get_mut(line))
            {
                output.push(if indexed.point == cursor {
                    '█'
                } else {
                    indexed.cell.c
                });
                if let Some(extra) = indexed.cell.zerowidth() {
                    output.extend(extra);
                }
            }
        }
        lines
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_owned()
    }
}

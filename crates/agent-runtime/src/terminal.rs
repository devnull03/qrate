use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::process::Command;
use std::sync::{Arc, mpsc};

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

use alacritty_terminal::event::{Event, EventListener, Notify as _, OnResize as _, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::{Cell as TerminalCell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Options, Shell};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use gpui::{
    App, FontStyle, FontWeight, HighlightStyle, Hsla, KeyDownEvent, StrikethroughStyle,
    UnderlineStyle, px, rgb,
};

use crate::AgentRuntime;

const COLS: usize = 100;
const LINES: usize = 32;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy)]
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
    pub backgrounds: Vec<TerminalBackground>,
    pub auth_urls: Vec<String>,
}

#[derive(Clone, Copy)]
pub struct TerminalBackground {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub color: Hsla,
}

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

fn rgb_color(color: Rgb) -> Hsla {
    rgb((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)).into()
}

fn indexed_color(index: u8) -> Hsla {
    const ANSI: [u32; 16] = [
        0x000000, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf, 0x555753,
        0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
    ];
    match index {
        0..=15 => rgb(ANSI[usize::from(index)]).into(),
        16..=231 => {
            let index = index - 16;
            let levels = [0, 95, 135, 175, 215, 255];
            rgb_color(Rgb {
                r: levels[usize::from(index / 36)],
                g: levels[usize::from((index % 36) / 6)],
                b: levels[usize::from(index % 6)],
            })
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            rgb_color(Rgb {
                r: value,
                g: value,
                b: value,
            })
        }
    }
}

fn named_color(color: NamedColor, palette: TerminalPalette) -> Hsla {
    match color {
        NamedColor::Foreground | NamedColor::BrightForeground => palette.foreground,
        NamedColor::Background => palette.background,
        NamedColor::Cursor => palette.foreground,
        NamedColor::DimForeground => palette.muted,
        NamedColor::Red | NamedColor::BrightRed | NamedColor::DimRed => palette.red,
        NamedColor::Green | NamedColor::BrightGreen | NamedColor::DimGreen => palette.green,
        NamedColor::Yellow | NamedColor::BrightYellow | NamedColor::DimYellow => palette.yellow,
        NamedColor::Blue | NamedColor::BrightBlue | NamedColor::DimBlue => palette.blue,
        NamedColor::Magenta | NamedColor::BrightMagenta | NamedColor::DimMagenta => palette.magenta,
        NamedColor::Cyan | NamedColor::BrightCyan | NamedColor::DimCyan => palette.cyan,
        NamedColor::Black | NamedColor::DimBlack => indexed_color(0),
        NamedColor::White | NamedColor::DimWhite => indexed_color(7),
        NamedColor::BrightBlack => indexed_color(8),
        NamedColor::BrightWhite => indexed_color(15),
    }
}

fn resolve_color(color: Color, palette: TerminalPalette, dynamic: &Colors) -> Hsla {
    match color {
        Color::Named(color) => dynamic[color]
            .map(rgb_color)
            .unwrap_or_else(|| named_color(color, palette)),
        Color::Spec(color) => rgb_color(color),
        Color::Indexed(index) => dynamic[usize::from(index)]
            .map(rgb_color)
            .unwrap_or_else(|| indexed_color(index)),
    }
}

fn terminal_style(
    cell: &TerminalCell,
    palette: TerminalPalette,
    dynamic: &Colors,
) -> HighlightStyle {
    let inverse = cell.flags.contains(Flags::INVERSE);
    let foreground = if inverse { cell.bg } else { cell.fg };
    let mut style = HighlightStyle {
        color: Some(resolve_color(foreground, palette, dynamic)),
        ..Default::default()
    };
    if cell.flags.contains(Flags::BOLD) {
        style.font_weight = Some(FontWeight::BOLD);
    }
    if cell.flags.contains(Flags::ITALIC) {
        style.font_style = Some(FontStyle::Italic);
    }
    if cell.flags.intersects(Flags::ALL_UNDERLINES) {
        style.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: cell
                .underline_color()
                .map(|color| resolve_color(color, palette, dynamic)),
            wavy: cell.flags.contains(Flags::UNDERCURL),
        });
    }
    if cell.flags.contains(Flags::STRIKEOUT) {
        style.strikethrough = Some(StrikethroughStyle {
            thickness: px(1.),
            color: None,
        });
    }
    if cell.flags.contains(Flags::DIM)
        || matches!(
            cell.fg,
            Color::Named(
                NamedColor::DimForeground
                    | NamedColor::DimBlack
                    | NamedColor::DimRed
                    | NamedColor::DimGreen
                    | NamedColor::DimYellow
                    | NamedColor::DimBlue
                    | NamedColor::DimMagenta
                    | NamedColor::DimCyan
                    | NamedColor::DimWhite
            )
        )
    {
        style.fade_out = Some(0.45);
    }
    if cell.flags.contains(Flags::HIDDEN) {
        style.color = terminal_background(cell, palette, dynamic).or(Some(palette.background));
    }
    style
}

fn terminal_background(
    cell: &TerminalCell,
    palette: TerminalPalette,
    dynamic: &Colors,
) -> Option<Hsla> {
    let background = if cell.flags.contains(Flags::INVERSE) {
        cell.fg
    } else {
        cell.bg
    };
    (background != Color::Named(NamedColor::Background))
        .then(|| resolve_color(background, palette, dynamic))
}

fn is_auth_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let Some(authority_and_path) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = authority_and_path.split('/').next().unwrap_or_default();
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    let trusted_host = [
        "x.ai",
        "openrouter.ai",
        "openai.com",
        "anthropic.com",
        "claude.ai",
        "google.com",
        "github.com",
        "microsoft.com",
        "qwen.ai",
        "alibabacloud.com",
    ]
    .iter()
    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
    trusted_host
        && [
            "/auth",
            "/oauth",
            "/device",
            "/login",
            "/authorize",
            "accounts.",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
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

    pub fn screen(&self, palette: TerminalPalette) -> TerminalScreen {
        let Some(session) = &self.session else {
            return TerminalScreen {
                text: String::new(),
                highlights: Vec::new(),
                backgrounds: Vec::new(),
                auth_urls: Vec::new(),
            };
        };
        struct RenderedCell {
            text: String,
            style: HighlightStyle,
            trimmable: bool,
        }

        let terminal = session.terminal.lock();
        let content = terminal.renderable_content();
        let cursor = content.cursor.point;
        let mut lines: Vec<Vec<RenderedCell>> = (0..terminal.screen_lines())
            .map(|_| Vec::with_capacity(COLS))
            .collect();
        let mut auth_urls = Vec::new();
        let mut backgrounds: Vec<TerminalBackground> = Vec::new();
        let mut seen_urls = HashSet::new();
        let top = -(content.display_offset as i32);
        for indexed in content.display_iter {
            let line = indexed.point.line.0 - top;
            if let Some(output) = usize::try_from(line)
                .ok()
                .and_then(|line| lines.get_mut(line))
            {
                let line = usize::try_from(line).expect("visible terminal line is non-negative");
                let column = indexed.point.column.0;
                if let Some(color) = terminal_background(indexed.cell, palette, content.colors) {
                    if let Some(previous) = backgrounds.last_mut()
                        && previous.line == line
                        && previous.end_column == column
                        && previous.color == color
                    {
                        previous.end_column += 1;
                    } else {
                        backgrounds.push(TerminalBackground {
                            line,
                            start_column: column,
                            end_column: column + 1,
                            color,
                        });
                    }
                }
                if let Some(hyperlink) = indexed.cell.hyperlink() {
                    let uri = hyperlink.uri();
                    if is_auth_url(uri) && seen_urls.insert(uri.to_owned()) {
                        auth_urls.push(uri.to_owned());
                    }
                }
                if indexed
                    .cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let is_cursor = indexed.point == cursor;
                let mut text = String::new();
                text.push(if is_cursor { '█' } else { indexed.cell.c });
                if let Some(extra) = indexed.cell.zerowidth() {
                    text.extend(extra);
                }
                output.push(RenderedCell {
                    trimmable: !is_cursor
                        && indexed.cell.c == ' '
                        && indexed.cell.bg == Color::Named(NamedColor::Background)
                        && !indexed.cell.flags.contains(Flags::INVERSE),
                    text,
                    style: terminal_style(indexed.cell, palette, content.colors),
                });
            }
        }
        for line in &mut lines {
            while line.last().is_some_and(|cell| cell.trimmable) {
                line.pop();
            }
        }
        while lines.last().is_some_and(Vec::is_empty) {
            lines.pop();
        }

        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        let line_count = lines.len();
        for (line_index, line) in lines.into_iter().enumerate() {
            for cell in line {
                let start = text.len();
                text.push_str(&cell.text);
                let end = text.len();
                if let Some((range, style)) = highlights.last_mut()
                    && range.end == start
                    && *style == cell.style
                {
                    range.end = end;
                } else {
                    highlights.push((start..end, cell.style));
                }
            }
            if line_index + 1 < line_count {
                text.push('\n');
            }
        }

        for token in text.split_whitespace() {
            let url = token
                .trim_start_matches(['<', '(', '[', '{'])
                .trim_end_matches(['>', ')', ']', '}', '.', ',', ';']);
            if is_auth_url(url) && seen_urls.insert(url.to_owned()) {
                auth_urls.push(url.to_owned());
            }
        }
        TerminalScreen {
            text,
            highlights,
            backgrounds,
            auth_urls,
        }
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::term::cell::{Cell, Flags};
    use alacritty_terminal::term::color::Colors;
    use alacritty_terminal::vte::ansi::{Color, Rgb};
    use gpui::hsla;

    use super::{TerminalPalette, is_auth_url, terminal_background, terminal_style};

    fn palette() -> TerminalPalette {
        TerminalPalette {
            foreground: hsla(0., 0., 0.1, 1.),
            background: hsla(0., 0., 0.9, 1.),
            muted: hsla(0., 0., 0.5, 1.),
            red: hsla(0., 1., 0.5, 1.),
            green: hsla(0.3, 1., 0.5, 1.),
            yellow: hsla(0.15, 1., 0.5, 1.),
            blue: hsla(0.6, 1., 0.5, 1.),
            magenta: hsla(0.8, 1., 0.5, 1.),
            cyan: hsla(0.5, 1., 0.5, 1.),
        }
    }

    #[test]
    fn only_authentication_urls_are_auto_opened() {
        assert!(is_auth_url(
            "https://accounts.x.ai/oauth2/device?user_code=ABCD"
        ));
        assert!(is_auth_url("https://openrouter.ai/auth?callback=true"));
        assert!(!is_auth_url("https://example.com/a-link-from-model-output"));
        assert!(!is_auth_url("https://example.com/oauth/device"));
        assert!(!is_auth_url("javascript:alert(1)"));
    }

    #[test]
    fn dim_text_and_explicit_backgrounds_survive_rendering() {
        let mut cell = Cell::default();
        cell.flags.insert(Flags::DIM);
        let colors = Colors::default();
        let default_style = terminal_style(&cell, palette(), &colors);
        assert_eq!(default_style.fade_out, Some(0.45));
        assert_eq!(default_style.background_color, None);
        assert_eq!(terminal_background(&cell, palette(), &colors), None);

        cell.bg = Color::Spec(Rgb { r: 1, g: 2, b: 3 });
        assert!(terminal_background(&cell, palette(), &colors).is_some());
    }
}

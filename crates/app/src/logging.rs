//! The session log, the panic hook, and the debug-info dump behind Help ▸ Copy Debug Info.
//!
//! Everything here shares one thing — the path to the current session's log file — which is why
//! the logger, the panic hook, the log tail, and the dump that embeds it live in one module.
//!
//! The log rotates on startup rather than by size or date: `qrate.log` is the run you are in and
//! `qrate.old.log` is the one before it. That is the whole retention policy, and it is what makes
//! "reproduce it, then send me the log" work — the reproduction is always the current file.
//!
//! Nothing is uploaded. `qrate.dvnl.work` is a static site and cannot receive a report, so the
//! dump travels by clipboard or by a prefilled GitHub issue URL, both of which put the user in
//! front of the text before it leaves the machine.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::panic;
use std::path::{Path, PathBuf};

use gpui::App;
use log::{Level, Log, Metadata, Record};
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, TermLogger, TerminalMode, WriteLogger,
};

/// Where the current session writes. `None` only if the OS has no local data dir, in which case
/// there is nowhere to log and the terminal sink is all there is.
pub fn log_path() -> Option<PathBuf> {
    settings::data_dir().map(|dir| dir.join("logs").join("qrate.log"))
}

/// Start file + terminal logging and route panics into both.
///
/// Called before anything else in `main`, so a failure during startup — the exact case where no
/// window ever appears to report it — still lands in the file.
pub fn init() {
    let config = ConfigBuilder::new()
        .set_time_offset_to_local()
        .unwrap_or_else(|builder| builder)
        // Which module spoke — `Error` is simplelog for "every level", and without it a report
        // says "window not found" without saying whether that was us or gpui.
        .set_target_level(LevelFilter::Error)
        .build();

    let mut sinks: Vec<Box<dyn simplelog::SharedLogger>> = vec![TermLogger::new(
        LevelFilter::Info,
        config.clone(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )];

    if let Some(path) = log_path() {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
            let _ = fs::rename(&path, dir.join("qrate.old.log"));
        }
        if let Ok(file) = File::create(&path) {
            sinks.push(WriteLogger::new(LevelFilter::Debug, config, file));
        }
    }

    let _ = log::set_boxed_logger(Box::new(QuietGpuiNoise(CombinedLogger::new(sinks))));
    log::set_max_level(LevelFilter::Debug);

    // Chained rather than replaced: the default hook is what prints a panic to a developer's
    // terminal, and losing that to gain the file would be a bad trade.
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        log::error!(
            "panic: {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        previous(info);
    }));

    log::info!("qrate {} ({}) starting", version(), env!("QRATE_GIT_SHA"));
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Removes GPUI's unactionable focus-node notice and demotes window-teardown errors to debug.
///
/// GPUI reports the missing focus node at info level every time focus reaches an internal element
/// without an accessibility role, and narrates every accessibility tree update — one line per
/// frame, which was four fifths of a session log. Neither is actionable from a qrate log, so omit
/// those two notices while retaining other accessibility diagnostics.
///
/// Closing a window always produces them: a detached per-window callback outlives the window,
/// reads it, and `log_err`s the expected miss. Kept in the file for teardown debugging, but not at
/// a level that makes every pasted bug report open with three errors nobody can act on.
struct QuietGpuiNoise(Box<dyn Log>);

fn is_accessibility_focus_noise(record: &Record) -> bool {
    if !record.target().starts_with("gpui::window") {
        return false;
    }
    let message = record.args().to_string();
    message.starts_with("a11y: focused element") || message.starts_with("Sending a11y tree update")
}

fn is_window_teardown(record: &Record) -> bool {
    if record.level() != Level::Error || !record.target().starts_with("gpui") {
        return false;
    }
    let message = record.args().to_string();
    message == "window not found" || message.starts_with("Invalid window handle")
}

impl Log for QuietGpuiNoise {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.0.enabled(metadata)
    }

    fn flush(&self) {
        self.0.flush();
    }

    fn log(&self, record: &Record) {
        if is_accessibility_focus_noise(record) {
            return;
        }
        if is_window_teardown(record) {
            self.0.log(
                &Record::builder()
                    .level(Level::Debug)
                    .target(record.target())
                    .module_path(record.module_path())
                    .file(record.file())
                    .line(record.line())
                    .args(*record.args())
                    .build(),
            );
            return;
        }
        self.0.log(record);
    }
}

/// Everything a bug report needs about the machine and the session, redacted and ready to paste.
///
/// `log_lines` is how much of the tail to embed: a couple of hundred for the clipboard, far fewer
/// for the issue URL, which GitHub truncates somewhere past 8 KB.
pub fn debug_info(cx: &App, log_lines: usize) -> String {
    let info = os_info::get();
    let mut out = String::new();

    let _ = writeln!(out, "qrate {} ({})", version(), env!("QRATE_GIT_SHA"));
    let _ = writeln!(
        out,
        "OS: {} {} [{}]",
        // The edition is the marketing name ("Windows 11 Home"); the type alone is just "Windows".
        info.edition().unwrap_or(&info.os_type().to_string()),
        info.version(),
        info.bitness()
    );

    // Built here and not at startup: a `System` walks the machine, and nothing before this line
    // needs it. `RefreshKind::nothing()` keeps that walk off the process table.
    let system = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_cpu(sysinfo::CpuRefreshKind::nothing())
            .with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    if let Some(cpu) = system.cpus().first() {
        // Trimmed: x86 brand strings are a fixed-width field, padded with spaces on the right.
        let _ = writeln!(out, "CPU: {} x {}", cpu.brand().trim(), system.cpus().len());
    }
    let _ = writeln!(
        out,
        "Memory: {} / {}",
        gib(system.used_memory()),
        gib(system.total_memory())
    );

    match cx.try_global::<settings::project::CurrentProject>() {
        Some(project) => {
            let _ = writeln!(
                out,
                "\nProject: {} ({} rows x {} columns)",
                project.file.display(),
                project.data.rows.len(),
                project.data.headers.len()
            );
        }
        None => {
            let _ = writeln!(out, "\nProject: none open");
        }
    }

    let _ = writeln!(out, "\nPlugins:");
    let plugins = plugin_host::status(cx);
    if plugins.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for (name, error) in plugins {
        match error {
            Some(err) => {
                let _ = writeln!(out, "  {name}  ERROR  {err}");
            }
            None => {
                let _ = writeln!(out, "  {name}  ok");
            }
        }
    }

    let _ = writeln!(out, "\n--- log (last {log_lines} lines) ---");
    let _ = writeln!(out, "{}", log_tail(log_lines));

    redact(&out)
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

fn log_tail(lines: usize) -> String {
    let Some(text) = log_path().and_then(|path| fs::read_to_string(path).ok()) else {
        return "(no log file)".into();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Replace the user's home directory with `~` throughout.
///
/// The dump carries project paths and plugin file names, which on a real machine means a real
/// name — and the point of a paste-it-yourself report is that what the user sees is what they send.
fn redact(text: &str) -> String {
    let Some(home) = dirs::home_dir().map(|h| h.display().to_string()) else {
        return text.to_string();
    };
    // Windows paths reach the dump both ways depending on who formatted them.
    text.replace(&home, "~")
        .replace(&home.replace('\\', "/"), "~")
}

/// Percent-encode for the `?body=` of a GitHub issue URL.
pub fn urlencode(text: &str) -> String {
    text.bytes().fold(String::new(), |mut out, byte| {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
        out
    })
}

/// The path Help ▸ Open Logs Folder reveals, or the folder itself if this session never opened a
/// file to reveal.
pub fn reveal_target() -> Option<PathBuf> {
    let path = log_path()?;
    if path.exists() {
        return Some(path);
    }
    path.parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    #[test]
    fn redacts_the_home_directory() {
        let home = dirs::home_dir().unwrap().display().to_string();
        let redacted = super::redact(&format!("Project: {home}/Documents/a.qrate"));
        assert!(!redacted.contains(&home), "home dir survived: {redacted}");
        assert!(redacted.contains("~"), "no marker left behind: {redacted}");
    }

    #[test]
    fn urlencodes_what_a_url_cannot_carry() {
        assert_eq!(super::urlencode("a b&c"), "a%20b%26c");
        assert_eq!(super::urlencode("qrate-0.1_x.y~z"), "qrate-0.1_x.y~z");
    }

    /// The demotion has to be narrow: it must catch gpui's two teardown messages and nothing else,
    /// or a real failure quietly stops being an error.
    #[test]
    fn only_window_teardown_is_demoted() {
        let teardown = |target: &str, level: log::Level, message: &str| {
            super::is_window_teardown(
                &log::Record::builder()
                    .level(level)
                    .target(target)
                    .args(format_args!("{message}"))
                    .build(),
            )
        };
        let error = log::Level::Error;

        assert!(teardown("gpui::window", error, "window not found"));
        assert!(teardown(
            "gpui_windows::window",
            error,
            "Invalid window handle (0x80040102)"
        ));

        assert!(!teardown(
            "gpui_windows::platform",
            error,
            "DirectX device lost"
        ));
        assert!(!teardown("app::logging", error, "window not found"));
        assert!(!teardown(
            "gpui::window",
            log::Level::Warn,
            "window not found"
        ));
    }

    #[test]
    fn only_gpui_missing_focus_nodes_are_suppressed() {
        let noisy = |target: &str, message: &str| {
            super::is_accessibility_focus_noise(
                &log::Record::builder()
                    .level(log::Level::Info)
                    .target(target)
                    .args(format_args!("{message}"))
                    .build(),
            )
        };

        assert!(noisy(
            "gpui::window::a11y",
            "a11y: focused element (FocusId(6v1)) has no accessibility node"
        ));
        assert!(!noisy(
            "gpui::window::a11y",
            "a11y: accessibility tree failed to update"
        ));
        assert!(!noisy(
            "app::window::a11y",
            "a11y: focused element (FocusId(6v1)) has no accessibility node"
        ));
    }

    /// The dump is only useful if every section is actually in it — a report missing the version
    /// or the plugin list costs a round trip to ask for it.
    #[gpui::test]
    fn the_dump_carries_every_section(cx: &mut gpui::TestAppContext) {
        let dump = cx.update(|cx| super::debug_info(cx, 5));
        for expected in [
            env!("CARGO_PKG_VERSION"),
            env!("QRATE_GIT_SHA"),
            "OS:",
            "Memory:",
            "Project:",
            "Plugins:",
            "--- log (last 5 lines) ---",
        ] {
            assert!(dump.contains(expected), "no {expected:?} in:\n{dump}");
        }
    }
}

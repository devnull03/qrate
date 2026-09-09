//! The session log, panic hook, and reports behind Help ▸ Copy Debug Info and Send Feedback.
//!
//! Everything here shares one thing — the path to the current session's log file — which is why
//! the logger, the panic hook, the log tail, and the dump that embeds it live in one module.
//!
//! The log rotates on startup rather than by size or date: `qrate.log` is the run you are in and
//! `qrate.old.log` is the one before it. That is the whole retention policy, and it is what makes
//! "reproduce it, then send me the log" work — the reproduction is always the current file.
//!
//! Reports open on the qrate site with diagnostics and a short, redacted log tail in the fragment.
//! The fragment is not sent in the page request, and the user reviews what to include before the
//! site uploads anything.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::panic;
use std::path::{Path, PathBuf};

use gpui::App;
use log::{Level, Log, Metadata, Record};
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, TermLogger, TerminalMode, WriteLogger,
};

pub enum FeedbackKind {
    Bug,
    Feature,
    Ux,
}

#[cfg(debug_assertions)]
const FEEDBACK_URL: &str = "http://localhost:4321/feedback";
#[cfg(not(debug_assertions))]
const FEEDBACK_URL: &str = "https://qrate.dvnl.work/feedback";

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

/// Removes GPUI's unactionable focus notice and demotes expected platform errors to debug.
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

/// Whether `target` is one gpui logs under.
///
/// The empty string is one of them. gpui's error helper builds the target from the caller's file
/// path by splitting on a `crates/` segment — which exists in zed's own checkout and not in the
/// published `gpui-pre`, unpacked as `…/gpui-pre-0.3.3/src/…`, so every error it logs arrives
/// with no target at all. Our own modules always have one, so nothing of ours is caught here.
fn logged_by_gpui(target: &str) -> bool {
    target.starts_with("gpui") || target.is_empty()
}

fn is_accessibility_focus_noise(record: &Record) -> bool {
    if !logged_by_gpui(record.target()) {
        return false;
    }
    let message = record.args().to_string();
    message.starts_with("a11y: focused element") || message.starts_with("Sending a11y tree update")
}

fn is_window_teardown(record: &Record) -> bool {
    if record.level() != Level::Error || !logged_by_gpui(record.target()) {
        return false;
    }
    let message = record.args().to_string();
    message == "window not found" || message.starts_with("Invalid window handle")
}

fn is_missing_dxgi_debug_layer(record: &Record) -> bool {
    record.level() == Level::Error
        && logged_by_gpui(record.target())
        && record.args().to_string().starts_with(
            "The application requested an operation that depends on an SDK component that is missing or mismatched.",
        )
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
        if is_window_teardown(record) || is_missing_dxgi_debug_layer(record) {
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
                "\nProject: open ({} rows x {} columns)",
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

fn feedback_log_tail() -> String {
    let log = redact(&log_tail(30));
    if log.chars().count() <= 1_500 {
        return log;
    }
    let tail: String = log.chars().rev().take(1_500).collect();
    format!(
        "(earlier lines omitted)\n{}",
        tail.chars().rev().collect::<String>()
    )
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

/// Open the hosted form with diagnostics and a bounded log tail in the fragment.
///
/// The page removes the fragment after reading it and shows both values before submission.
pub fn feedback_url(cx: &App, kind: Option<FeedbackKind>) -> String {
    let kind = match kind {
        Some(FeedbackKind::Bug) => "?type=bug",
        Some(FeedbackKind::Feature) => "?type=feature",
        Some(FeedbackKind::Ux) => "?type=ui_ux",
        None => "",
    };
    let plugins = plugin_host::status(cx);
    let project = cx
        .try_global::<settings::project::CurrentProject>()
        .map(|project| {
            serde_json::json!({
                "rows": project.data.rows.len(),
                "columns": project.data.headers.len(),
            })
        });
    let diagnostics = serde_json::json!({
        "schema": 1,
        "version": version(),
        "commit": env!("QRATE_GIT_SHA"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "project": project,
        "plugins": plugins.len(),
        "plugin_failures": plugins.iter().filter(|(_, error)| error.is_some()).count(),
    });
    format!(
        "{FEEDBACK_URL}{kind}#diagnostics={}&logs={}",
        urlencode(&diagnostics.to_string()),
        urlencode(&feedback_log_tail())
    )
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

    #[gpui::test]
    fn feedback_links_select_the_form_and_keep_diagnostics_in_the_fragment(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bug, feature, ux) = cx.update(|cx| {
            (
                super::feedback_url(cx, Some(super::FeedbackKind::Bug)),
                super::feedback_url(cx, Some(super::FeedbackKind::Feature)),
                super::feedback_url(cx, Some(super::FeedbackKind::Ux)),
            )
        });

        assert!(bug.starts_with(&format!("{}?type=bug#diagnostics=", super::FEEDBACK_URL)));
        assert!(feature.contains("?type=feature#diagnostics="));
        assert!(ux.contains("?type=ui_ux#diagnostics="));
        assert!(bug.contains("%22schema%22%3A1"));
        assert!(bug.contains("&logs="));
        assert!(!bug.contains("CPU"));
        assert!(bug.len() < 7_000);
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
        // The published `gpui-pre` logs with no target at all: gpui's error helper derives one
        // from a `crates/` path segment that only exists in zed's own checkout. Missing this case
        // put 162 of these in one session's log the first time the app ran on 0.6.
        assert!(teardown("", error, "window not found"));

        assert!(!teardown(
            "gpui_windows::platform",
            error,
            "DirectX device lost"
        ));
        // An untargeted error still has to *say* it is a teardown to be demoted.
        assert!(!teardown("", error, "DirectX device lost"));
        assert!(!teardown("app::logging", error, "window not found"));
        assert!(!teardown(
            "gpui::window",
            log::Level::Warn,
            "window not found"
        ));
    }

    #[test]
    fn only_the_optional_dxgi_debug_layer_error_is_demoted() {
        let dxgi = |target: &str, level: log::Level, message: &str| {
            super::is_missing_dxgi_debug_layer(
                &log::Record::builder()
                    .level(level)
                    .target(target)
                    .args(format_args!("{message}"))
                    .build(),
            )
        };
        let error = log::Level::Error;

        assert!(dxgi(
            "",
            error,
            "The application requested an operation that depends on an SDK component that is missing or mismatched. (0x887A002D)"
        ));
        assert!(!dxgi(
            "gpui_windows::directx_devices",
            error,
            "Failed to create Direct3D device"
        ));
        assert!(!dxgi(
            "app::logging",
            error,
            "The application requested an operation that depends on an SDK component that is missing or mismatched. (0x887A002D)"
        ));
        assert!(!dxgi(
            "",
            log::Level::Warn,
            "The application requested an operation that depends on an SDK component that is missing or mismatched. (0x887A002D)"
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
        // Untargeted, for the same reason the teardown filter accepts it.
        assert!(noisy(
            "",
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

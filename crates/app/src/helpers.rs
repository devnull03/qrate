use std::path::Path;
use std::process::Command;

use gpui::*;
use gpui_component::input::InputState;

use workspace::Workspace;

pub fn get_file(
    window: &mut Window,
    cx: &mut Context<Workspace>,
    input: &Entity<InputState>,
    prompt: SharedString,
    is_folder: bool,
) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: !is_folder,
        directories: is_folder,
        multiple: false,
        prompt: Some(prompt),
    });

    let input = input.clone();
    cx.spawn_in(window, async move |_, cx| {
        if let Ok(Ok(Some(paths))) = receiver.await
            && let Some(path) = paths.first()
        {
            cx.update(|window, cx| {
                input.update(cx, |state, cx| {
                    state.set_value(path.to_string_lossy().to_string(), window, cx);
                });
            })
            .ok();
        }
    })
    .detach();
}

/// Opens a terminal at `cwd`. If `command` is non-empty it is executed after `cd`.
///
/// Windows: tries Windows Terminal (`wt`) first, falls back to `cmd.exe`.
/// macOS: checks `$TERM_PROGRAM` for iTerm2, falls back to Terminal.app.
pub fn spawn_terminal_at(cwd: &Path, command: &str) -> std::io::Result<()> {
    if !cwd.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("spawn_terminal_at expects a directory: {}", cwd.display()),
        ));
    }

    #[cfg(windows)]
    {
        let cwd_str = cwd.to_string_lossy();

        // Try Windows Terminal first — it accepts --startingDirectory natively.
        let mut wt = Command::new("wt");
        wt.arg("--startingDirectory").arg(cwd);
        if !command.trim().is_empty() {
            wt.args(["cmd", "/k", command]);
        }
        if wt.spawn().is_ok() {
            return Ok(());
        }

        // Fall back to cmd.exe. `start /d path` sets the new window's working directory.
        let mut args = vec!["/C", "start", "", "/d", &cwd_str, "cmd"];
        let script;
        if !command.trim().is_empty() {
            script = command.to_string();
            args.extend_from_slice(&["/k", &script]);
        }
        Command::new("cmd").args(&args).spawn()?;
    }

    #[cfg(target_os = "macos")]
    {
        let term_prog = std::env::var("TERM_PROGRAM").unwrap_or_default();

        if command.trim().is_empty() {
            if term_prog == "iTerm.app" {
                let escaped = escape_applescript(&cwd.to_string_lossy());
                let osa = format!(
                    r#"tell application "iTerm" to create window with default profile command "cd \"{}\"" "#,
                    escaped
                );
                Command::new("osascript").args(["-e", &osa]).spawn()?;
            } else {
                // Terminal.app accepts a directory argument directly.
                Command::new("open")
                    .args(["-a", "Terminal"])
                    .arg(cwd)
                    .spawn()?;
            }
        } else {
            let script = format!("cd \"{}\" && {}", cwd.display(), command);
            let escaped = escape_applescript(&script);
            let app = if term_prog == "iTerm.app" {
                "iTerm"
            } else {
                "Terminal"
            };
            let osa = format!(r#"tell application "{}" to do script "{}""#, app, escaped);
            Command::new("osascript").args(["-e", &osa]).spawn()?;
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "spawn_terminal_at is only supported on Windows and macOS",
        ));
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

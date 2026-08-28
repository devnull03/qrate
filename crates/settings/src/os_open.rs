//! Shelling out to the OS's own file handling — no gpui dependency, just `std::process::Command`.
//! Lives here (rather than `app`) so both `app` and `workspace` (which can't depend on `app`) can
//! call it — the Details panel's image overlay is the first `workspace`-side caller.

use std::path::Path;
use std::process::Command;

/// Opens `path` in the OS's default application for its file type.
pub fn open_in_default_app(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path does not exist: {}", path.display()),
        ));
    }

    #[cfg(windows)]
    {
        // `cmd /C start ""` — the empty title argument keeps `start` from treating a
        // quoted path as the window title instead of the file to open.
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).spawn()?;
    }

    Ok(())
}

/// Opens the file manager and selects `path`.
///
/// For a file, highlights that file in its parent folder. For a directory, shows that folder
/// selected in its parent (Windows) or opens it (macOS, Linux).
pub fn reveal_in_folder(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path does not exist: {}", path.display()),
        ));
    }

    #[cfg(windows)]
    {
        Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", &path.to_string_lossy()])
            .spawn()?;
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // The freedesktop way to *select* a file; Nautilus/Dolphin/Thunar/Nemo all serve it.
        let selected = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!(
                "array:string:file://{}",
                path.to_string_lossy().replace(' ', "%20")
            ))
            .arg("string:")
            .status()
            .is_ok_and(|s| s.success());
        if !selected {
            let dir = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(path)
            };
            Command::new("xdg-open").arg(dir).spawn()?;
        }
    }

    Ok(())
}

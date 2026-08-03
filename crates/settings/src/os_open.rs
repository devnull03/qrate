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
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "open_in_default_app is only supported on Windows and macOS",
        ));
    }

    Ok(())
}

/// Opens the file manager and selects `path`.
///
/// For a file, highlights that file in its parent folder. For a directory, shows that folder
/// selected in its parent (Windows) or opens it (macOS).
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
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "reveal_in_folder is only supported on Windows and macOS",
        ));
    }

    Ok(())
}

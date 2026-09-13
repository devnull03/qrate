use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitStatus, Stdio},
};

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "qrate",
    version,
    args_conflicts_with_subcommands = true,
    about = "Launch the qrate desktop application"
)]
struct Cli {
    /// Project file to open.
    project: Option<PathBuf>,
    /// Wait for the desktop process to exit and return its exit status.
    #[arg(long, global = true)]
    wait: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open a project, or show the project launcher.
    Open { project: Option<PathBuf> },
    /// Print the qrate CLI version.
    Version,
}

fn main() {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::Version)) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let project = match cli.command {
        Some(Command::Open { project }) => project,
        _ => cli.project,
    };
    let result = (|| {
        if let Some(project) = &project {
            validate_project(project)?;
        }
        let executable = std::env::current_exe()
            .map_err(|error| format!("cannot locate the qrate executable: {error}"))?;
        launch(&desktop_path(&executable), project.as_deref(), cli.wait)
    })();
    match result {
        Ok(Some(status)) => {
            if !status.success() {
                eprintln!("qrate: desktop exited with {status}; see the desktop log for details");
            }
            std::process::exit(exit_code(status));
        }
        Ok(None) => {}
        Err(error) => {
            // The public CLI has a console even when the desktop binary does not.
            eprintln!("qrate: {error}");
            std::process::exit(1);
        }
    }
}

fn validate_project(project: &Path) -> Result<(), String> {
    if project
        .to_str()
        .is_some_and(|path| path.to_ascii_lowercase().starts_with("qrate://"))
    {
        return Ok(());
    }
    if project.extension() != Some(OsStr::new("qrate")) {
        return Err(format!(
            "expected a .qrate project file: {}",
            project.display()
        ));
    }
    if !project.is_file() {
        return Err(format!(
            "project file does not exist or is not a file: {}",
            project.display()
        ));
    }
    Ok(())
}

fn desktop_path(executable: &Path) -> PathBuf {
    executable.with_file_name(format!("qrate-app{}", std::env::consts::EXE_SUFFIX))
}

fn launch(
    desktop: &Path,
    project: Option<&Path>,
    wait: bool,
) -> Result<Option<ExitStatus>, String> {
    let mut command = ProcessCommand::new(desktop);
    command.stdin(Stdio::null());
    if let Some(project) = project {
        command.arg("--").arg(project);
    }
    if !wait {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        format!(
            "cannot launch {}: {error}. Install qrate and qrate-app together",
            desktop.display()
        )
    })?;
    if wait {
        child
            .wait()
            .map(Some)
            .map_err(|error| format!("cannot wait for {}: {error}", desktop.display()))
    } else {
        Ok(None)
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{Cli, Command, desktop_path, launch, validate_project};
    use std::path::Path;

    #[test]
    fn parses_version_command() {
        let cli = Cli::try_parse_from(["qrate", "version"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Version)));
    }

    #[test]
    fn parses_launch_forms() {
        let cli = Cli::try_parse_from(["qrate"]).unwrap();
        assert!(cli.command.is_none() && cli.project.is_none() && !cli.wait);
        let cli = Cli::try_parse_from(["qrate", "my project.qrate", "--wait"]).unwrap();
        assert_eq!(cli.project.as_deref(), Some(Path::new("my project.qrate")));
        assert!(cli.wait);
        let cli = Cli::try_parse_from(["qrate", "open", "--wait", "my project.qrate"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Open { project: Some(_) })
        ));
        assert!(cli.wait);
        assert!(Cli::try_parse_from(["qrate", "open"]).is_ok());
        assert!(Cli::try_parse_from(["qrate", "--wait", "open"]).is_ok());
        assert!(Cli::try_parse_from(["qrate", "a.qrate", "open", "b.qrate"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "--unknown"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "a.qrate", "b.qrate"]).is_err());
    }

    #[test]
    fn resolves_only_a_sibling() {
        let path = desktop_path(Path::new("directory with spaces/qrate"));
        assert_eq!(path.parent(), Some(Path::new("directory with spaces")));
        assert_eq!(path.file_stem().unwrap(), "qrate-app");
    }

    #[test]
    fn accepts_links_and_rejects_missing_projects() {
        assert!(validate_project(Path::new("qrate://plugin/install/example")).is_ok());
        assert!(validate_project(Path::new("not-a-project.txt")).is_err());
        assert!(validate_project(Path::new("missing-project.qrate")).is_err());
    }

    #[test]
    fn reports_missing_desktop() {
        assert!(
            launch(Path::new("missing-desktop/qrate-app"), None, false)
                .unwrap_err()
                .contains("Install qrate and qrate-app together")
        );
    }

    #[cfg(unix)]
    #[test]
    fn waits_and_preserves_native_arguments() {
        use std::os::unix::ffi::OsStringExt;
        let path = std::ffi::OsString::from_vec(b"project with spaces \xff.qrate".to_vec());
        let cli = Cli::try_parse_from([std::ffi::OsString::from("qrate"), path.clone()]).unwrap();
        assert_eq!(cli.project.unwrap().as_os_str(), path);
        let status = launch(Path::new("/bin/false"), Some(Path::new(&path)), true)
            .unwrap()
            .unwrap();
        assert_eq!(super::exit_code(status), 1);
        assert!(
            launch(Path::new("/bin/true"), None, true)
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

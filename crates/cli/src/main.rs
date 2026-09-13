use std::{
    ffi::OsStr,
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitStatus, Stdio},
};

use clap::{Parser, Subcommand};
use serde::Deserialize;

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
    /// Inspect or launch the desktop application.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Inspect the active desktop project.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Print the qrate CLI version.
    Version,
}

#[derive(Debug, Subcommand)]
enum AppCommand {
    /// Show whether qrate is running and its active project.
    Status,
    /// Print the resolved desktop executable path.
    Path,
    /// Launch qrate without opening a project.
    Launch {
        /// Wait for the desktop process to exit.
        #[arg(long)]
        wait: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Show metadata for the active desktop project.
    Info,
}

fn main() {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::Version)) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => fail(format!("cannot locate the qrate executable: {error}")),
    };
    if let Some(Command::App { command }) = &cli.command {
        let result = match command {
            AppCommand::Status => app_status(),
            AppCommand::Path => {
                println!("{}", desktop_path(&executable).display());
                Ok(None)
            }
            AppCommand::Launch { wait } => launch(&desktop_path(&executable), None, *wait),
        };
        finish(result);
        return;
    }
    if let Some(Command::Project {
        command: ProjectCommand::Info,
    }) = &cli.command
    {
        finish(project_info());
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
        launch(&desktop_path(&executable), project.as_deref(), cli.wait)
    })();
    finish(result);
}

fn finish(result: Result<Option<ExitStatus>, String>) {
    match result {
        Ok(Some(status)) => {
            if !status.success() {
                eprintln!("qrate: desktop exited with {status}; see the desktop log for details");
            }
            std::process::exit(exit_code(status));
        }
        Ok(None) => {}
        Err(error) => {
            fail(error);
        }
    }
}

fn fail(error: String) -> ! {
    // The public CLI has a console even when the desktop binary does not.
    eprintln!("qrate: {error}");
    std::process::exit(1);
}

#[derive(Deserialize)]
struct AppControlDescriptor {
    app_control_protocol: u8,
    url: String,
    token: String,
}

#[derive(Deserialize)]
struct AppStatus {
    running: bool,
    project: Option<String>,
}

#[derive(Deserialize)]
struct ProjectInfoResponse {
    app_control_protocol: u8,
    project: ProjectInfo,
}

#[derive(Deserialize)]
struct ProjectInfo {
    name: String,
    path: String,
    source: Option<String>,
    created_at: Option<String>,
    link_method: Option<String>,
    files_folder: Option<String>,
    row_count: usize,
    column_count: usize,
}

fn project_info() -> Result<Option<ExitStatus>, String> {
    let body = app_request("/v1/project/info")?;
    let info: ProjectInfoResponse = serde_json::from_str(&body)
        .map_err(|error| format!("invalid qrate project info: {error}"))?;
    if info.app_control_protocol != 1 {
        return Err(format!(
            "unsupported project-info protocol {}",
            info.app_control_protocol
        ));
    }
    println!("name: {}", info.project.name);
    println!("path: {}", info.project.path);
    println!(
        "source: {}",
        info.project.source.as_deref().unwrap_or("none")
    );
    println!(
        "created: {}",
        info.project.created_at.as_deref().unwrap_or("unknown")
    );
    println!(
        "link method: {}",
        info.project.link_method.as_deref().unwrap_or("none")
    );
    println!(
        "files folder: {}",
        info.project.files_folder.as_deref().unwrap_or("none")
    );
    println!("rows: {}", info.project.row_count);
    println!("columns: {}", info.project.column_count);
    Ok(None)
}

fn app_request(route: &str) -> Result<String, String> {
    let path = dirs::data_local_dir()
        .map(|dir| dir.join("qrate/app-control.json"))
        .ok_or_else(|| "cannot locate qrate application data".to_string())?;
    let descriptor: AppControlDescriptor = fs::read(&path)
        .map_err(|_| "qrate is not running".to_string())
        .and_then(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|error| format!("invalid app-control descriptor: {error}"))
        })?;
    if descriptor.app_control_protocol != 1 {
        return Err(format!(
            "unsupported app-control protocol {}",
            descriptor.app_control_protocol
        ));
    }
    let address = descriptor
        .url
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| "invalid app-control endpoint".to_string())?;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{address}"))
        .map_err(|_| "qrate is not running".to_string())?;
    let timeout = Some(std::time::Duration::from_secs(1));
    stream
        .set_read_timeout(timeout)
        .and_then(|()| stream.set_write_timeout(timeout))
        .map_err(|error| format!("cannot configure qrate connection: {error}"))?;
    write!(
        stream,
        "GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        descriptor.token
    )
    .map_err(|error| format!("cannot query qrate: {error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("cannot read qrate response: {error}"))?;
    parse_app_response(&response)
}

fn parse_app_response(response: &str) -> Result<String, String> {
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid response from qrate".to_string())?;
    if head.starts_with("HTTP/1.1 200 ") {
        return Ok(body.to_string());
    }
    if head.starts_with("HTTP/1.1 409 ") && body.contains(r#""error":"no_active_project""#) {
        return Err("no project is active; run `qrate <PROJECT.qrate>`".to_string());
    }
    Err("qrate refused the request".to_string())
}

fn app_status() -> Result<Option<ExitStatus>, String> {
    let body = match app_request("/status") {
        Ok(body) => body,
        Err(error) if error == "qrate is not running" => {
            println!("stopped");
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let status: AppStatus =
        serde_json::from_str(&body).map_err(|error| format!("invalid qrate status: {error}"))?;
    println!("{}", if status.running { "running" } else { "stopped" });
    if let Some(project) = status.project {
        println!("project: {project}");
    } else {
        println!("project: none");
    }
    Ok(None)
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
    let directory = executable.parent().unwrap_or_else(|| Path::new(""));
    let install_directory = if directory.file_name() == Some(OsStr::new("bin")) {
        directory.parent().unwrap_or(directory)
    } else {
        directory
    };
    install_directory.join(format!("qrate{}", std::env::consts::EXE_SUFFIX))
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
            "cannot launch {}: {error}. Install qrate and qrate-cli together",
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

    use super::{
        AppCommand, Cli, Command, ProjectCommand, desktop_path, launch, parse_app_response,
        validate_project,
    };
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
        let cli = Cli::try_parse_from(["qrate", "app", "launch", "--wait"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::App {
                command: AppCommand::Launch { wait: true }
            })
        ));
        assert!(Cli::try_parse_from(["qrate", "app", "status"]).is_ok());
        assert!(Cli::try_parse_from(["qrate", "app", "path"]).is_ok());
        let cli = Cli::try_parse_from(["qrate", "project", "info"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Project {
                command: ProjectCommand::Info
            })
        ));
        assert!(Cli::try_parse_from(["qrate", "project"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "project", "verify"]).is_err());
    }

    #[test]
    fn parses_project_info_protocol_responses() {
        assert_eq!(
            parse_app_response("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap(),
            "{}"
        );
        let error = parse_app_response(
            "HTTP/1.1 409 Conflict\r\nContent-Length: 54\r\n\r\n{\"app_control_protocol\":1,\"error\":\"no_active_project\"}",
        )
        .unwrap_err();
        assert!(error.contains("no project is active"));
        assert!(error.contains("qrate <PROJECT.qrate>"));
        assert!(parse_app_response("HTTP/1.1 401 Unauthorized\r\n\r\n{}").is_err());
    }

    #[test]
    fn resolves_only_a_sibling() {
        let path = desktop_path(Path::new("directory with spaces/qrate-cli"));
        assert_eq!(path.parent(), Some(Path::new("directory with spaces")));
        assert_eq!(path.file_stem().unwrap(), "qrate");
        let shim = desktop_path(Path::new("directory with spaces/bin/qrate"));
        assert_eq!(
            shim,
            Path::new("directory with spaces")
                .join(format!("qrate{}", std::env::consts::EXE_SUFFIX))
        );
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
            launch(Path::new("missing-desktop/qrate"), None, false)
                .unwrap_err()
                .contains("Install qrate and qrate-cli together")
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

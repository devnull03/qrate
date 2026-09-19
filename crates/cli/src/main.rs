use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitStatus, Stdio},
};

use clap::{CommandFactory as _, Parser, Subcommand};
use serde::Deserialize;
use serde_json::Value;

const REFUSED: i32 = 1;
const USAGE: i32 = 2;
const UNAVAILABLE: i32 = 3;

#[derive(Debug, Parser)]
#[command(
    name = "qrate",
    version,
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
    /// Read the live project as an agent. Parameters are JSON on stdin; the answer is JSON on stdout.
    Agent {
        /// Name shown in qrate's Agent panel. Defaults to $QRATE_AGENT. A label, not proof.
        #[arg(long, global = true)]
        agent: Option<String>,
        #[command(subcommand)]
        command: AgentCommand,
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
    Launch,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Show metadata for the active desktop project.
    Info,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum AgentCommand {
    /// Project, columns, selection, diagnostic counts and revision. Takes no input.
    Overview,
    /// Bounded rows or diagnostics.
    Query,
    /// Validate and activate a confined Luau program without running it.
    ProgramSave,
    /// Run the saved program once at an exact revision.
    ProgramRun,
    /// Up to four 512-pixel PNG thumbnails.
    Thumbnails,
    /// Publish a complete batch of draft findings. Never changes a cell.
    StageFindings,
}

impl AgentCommand {
    fn method(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Query => "query",
            Self::ProgramSave => "program_save",
            Self::ProgramRun => "program_run",
            Self::Thumbnails => "thumbnails",
            Self::StageFindings => "stage_findings",
        }
    }
}

/// Why a command failed, which decides its exit code.
#[derive(Debug, PartialEq)]
enum Failure {
    /// qrate answered with a refusal; the JSON goes to stdout for the caller to read.
    Refused(String),
    Usage(String),
    Unavailable(String),
    Other(String),
}

/// `args_conflicts_with_subcommands` would do this, but it also stops a root `--wait` reaching one.
fn parse_args(
    args: impl IntoIterator<Item = impl Into<std::ffi::OsString> + Clone>,
) -> Result<Cli, clap::Error> {
    let cli = Cli::try_parse_from(args)?;
    if cli.project.is_some() && cli.command.is_some() {
        return Err(Cli::command().error(
            clap::error::ErrorKind::ArgumentConflict,
            "a project path cannot be combined with a subcommand",
        ));
    }
    Ok(cli)
}

fn main() {
    let cli = parse_args(std::env::args_os()).unwrap_or_else(|error| error.exit());
    let result = match cli.command {
        Some(Command::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(None)
        }
        Some(Command::App {
            command: AppCommand::Status,
        }) => app_status(),
        Some(Command::App {
            command: AppCommand::Path,
        }) => executable().map(|executable| {
            println!("{}", desktop_path(&executable).display());
            None
        }),
        Some(Command::App {
            command: AppCommand::Launch,
        }) => {
            executable().and_then(|executable| launch(&desktop_path(&executable), None, cli.wait))
        }
        Some(Command::Project {
            command: ProjectCommand::Info,
        }) => project_info(),
        Some(Command::Agent { agent, command }) => agent_call(
            command,
            agent.or_else(|| std::env::var("QRATE_AGENT").ok()),
            &mut std::io::stdin(),
        ),
        Some(Command::Open { project }) => open(project, cli.wait),
        None => open(cli.project, cli.wait),
    };
    match result {
        Ok(Some(status)) => {
            if !status.success() {
                eprintln!("qrate: desktop exited with {status}; see the desktop log for details");
            }
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(None) => {}
        Err(Failure::Refused(body)) => {
            println!("{body}");
            std::process::exit(REFUSED);
        }
        // The public CLI has a console even when the desktop binary does not.
        Err(Failure::Usage(error)) => fail(error, USAGE),
        Err(Failure::Unavailable(error)) => fail(error, UNAVAILABLE),
        Err(Failure::Other(error)) => fail(error, 1),
    }
}

fn fail(error: String, code: i32) -> ! {
    eprintln!("qrate: {error}");
    std::process::exit(code);
}

/// Canonical, so a symlink such as `~/.local/bin/qrate` still finds the install it points into.
fn executable() -> Result<PathBuf, Failure> {
    std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| Failure::Other(format!("cannot locate the qrate executable: {error}")))
}

fn open(project: Option<PathBuf>, wait: bool) -> Result<Option<ExitStatus>, Failure> {
    if let Some(project) = &project {
        validate_project(project).map_err(Failure::Usage)?;
    }
    launch(&desktop_path(&executable()?), project.as_deref(), wait)
}

#[derive(Deserialize)]
struct AppControlDescriptor {
    app_control_protocol: u8,
    url: String,
    token: String,
}

#[derive(Deserialize)]
struct ProjectInfoResponse {
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

fn project_info() -> Result<Option<ExitStatus>, Failure> {
    let body = match app_request("GET", "/v1/project/info", None, None)? {
        (409, _) => {
            return Err(Failure::Other(
                "no project is active; run `qrate <PROJECT.qrate>`".into(),
            ));
        }
        response => accept(response)?,
    };
    let info: ProjectInfoResponse = serde_json::from_str(&body)
        .map_err(|error| Failure::Other(format!("invalid qrate project info: {error}")))?;
    let project = info.project;
    let or = |value: Option<String>, missing: &str| value.unwrap_or_else(|| missing.to_owned());
    println!("name: {}", project.name);
    println!("path: {}", project.path);
    println!("source: {}", or(project.source, "none"));
    println!("created: {}", or(project.created_at, "unknown"));
    println!("link method: {}", or(project.link_method, "none"));
    println!("files folder: {}", or(project.files_folder, "none"));
    println!("rows: {}", project.row_count);
    println!("columns: {}", project.column_count);
    Ok(None)
}

fn app_status() -> Result<Option<ExitStatus>, Failure> {
    let body = match app_request("GET", "/v1/status", None, None) {
        Err(Failure::Unavailable(_)) => {
            println!("stopped");
            return Ok(None);
        }
        response => accept(response?)?,
    };
    let status: Value = serde_json::from_str(&body)
        .map_err(|error| Failure::Other(format!("invalid qrate status: {error}")))?;
    println!("running");
    println!("project: {}", status["project"].as_str().unwrap_or("none"));
    Ok(None)
}

fn agent_call(
    command: AgentCommand,
    agent: Option<String>,
    stdin: &mut impl Read,
) -> Result<Option<ExitStatus>, Failure> {
    let request = agent_request(command, stdin)?;
    let body = accept(app_request(
        "POST",
        "/v1/agent",
        agent.as_deref(),
        Some(&request),
    )?)?;
    println!("{body}");
    Ok(None)
}

fn agent_request(command: AgentCommand, stdin: &mut impl Read) -> Result<String, Failure> {
    let mut request = serde_json::json!({ "method": command.method() });
    if !matches!(command, AgentCommand::Overview) {
        let mut input = String::new();
        stdin.read_to_string(&mut input).map_err(|error| {
            Failure::Usage(format!("cannot read parameters from stdin: {error}"))
        })?;
        let params: Value = serde_json::from_str(&input)
            .map_err(|error| Failure::Usage(format!("stdin is not a JSON object: {error}")))?;
        if !params.is_object() {
            return Err(Failure::Usage("stdin is not a JSON object".into()));
        }
        request["params"] = params;
    }
    Ok(request.to_string())
}

/// A 200 body, or the reason there is none.
fn accept((code, body): (u16, String)) -> Result<String, Failure> {
    match code {
        200 => Ok(body),
        401 => Err(Failure::Unavailable(
            "qrate rejected this CLI's credentials; restart qrate".into(),
        )),
        400 | 403 | 409 | 413 => Err(Failure::Refused(body)),
        503 => Err(Failure::Unavailable("qrate is closing".into())),
        _ => Err(Failure::Other(format!("qrate answered with HTTP {code}"))),
    }
}

fn app_request(
    method: &str,
    route: &str,
    agent: Option<&str>,
    body: Option<&str>,
) -> Result<(u16, String), Failure> {
    let not_running = || Failure::Unavailable("qrate is not running".into());
    let path = dirs::data_local_dir()
        .map(|dir| dir.join("qrate/app-control.json"))
        .ok_or_else(|| Failure::Other("cannot locate qrate application data".into()))?;
    let descriptor: AppControlDescriptor =
        serde_json::from_slice(&fs::read(&path).map_err(|_| not_running())?)
            .map_err(|error| Failure::Other(format!("invalid app-control descriptor: {error}")))?;
    if descriptor.app_control_protocol != 1 {
        return Err(Failure::Unavailable(format!(
            "qrate speaks app-control protocol {}; install qrate and qrate-cli together",
            descriptor.app_control_protocol
        )));
    }
    let address = descriptor
        .url
        .strip_prefix("http://")
        .filter(|address| address.starts_with("127.0.0.1:"))
        .ok_or_else(|| Failure::Other("invalid app-control endpoint".into()))?;
    let mut stream = TcpStream::connect(address).map_err(|_| not_running())?;
    let timeout = Some(std::time::Duration::from_secs(30));
    let _ = stream
        .set_read_timeout(timeout)
        .and_then(|()| stream.set_write_timeout(timeout));
    let body = body.unwrap_or_default();
    let agent = agent
        .map(|name| format!("X-Agent: {}\r\n", name.replace(['\r', '\n'], " ")))
        .unwrap_or_default();
    write!(
        stream,
        "{method} {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\n{agent}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        descriptor.token,
        body.len()
    )
    .map_err(|error| Failure::Unavailable(format!("cannot query qrate: {error}")))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| Failure::Unavailable(format!("cannot read qrate's answer: {error}")))?;
    parse_response(&response)
}

fn parse_response(response: &str) -> Result<(u16, String), Failure> {
    let invalid = || Failure::Other("invalid response from qrate".into());
    let (head, body) = response.split_once("\r\n\r\n").ok_or_else(invalid)?;
    let code = head
        .strip_prefix("HTTP/1.1 ")
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse().ok())
        .ok_or_else(invalid)?;
    Ok((code, body.to_owned()))
}

fn validate_project(project: &Path) -> Result<(), String> {
    if project
        .to_str()
        .is_some_and(|path| path.to_ascii_lowercase().starts_with("qrate://"))
    {
        return Ok(());
    }
    if !project
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("qrate"))
    {
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

/// The GUI beside this program. A copy named `qrate` (Windows `bin\qrate.exe`) is its own sibling,
/// so that one looks one directory up.
fn desktop_path(executable: &Path) -> PathBuf {
    let name = format!("qrate{}", std::env::consts::EXE_SUFFIX);
    let directory = executable.parent().unwrap_or_else(|| Path::new(""));
    let sibling = directory.join(&name);
    if sibling != executable {
        return sibling;
    }
    directory
        .parent()
        .map_or(sibling, |parent| parent.join(&name))
}

fn launch(
    desktop: &Path,
    project: Option<&Path>,
    wait: bool,
) -> Result<Option<ExitStatus>, Failure> {
    let mut command = ProcessCommand::new(desktop);
    command.stdin(Stdio::null());
    if let Some(project) = project {
        command.arg("--").arg(project);
    }
    if !wait {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        Failure::Other(format!(
            "cannot launch {}: {error}. Install qrate and qrate-cli together",
            desktop.display()
        ))
    })?;
    if !wait {
        return Ok(None);
    }
    child
        .wait()
        .map(Some)
        .map_err(|error| Failure::Other(format!("cannot wait for {}: {error}", desktop.display())))
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{
        AgentCommand, Cli, Command, Failure, ProjectCommand, accept, agent_request, desktop_path,
        launch, parse_args, parse_response, validate_project,
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
        let cli = Cli::try_parse_from(["qrate", "--wait", "open"]).unwrap();
        assert!(cli.wait && matches!(cli.command, Some(Command::Open { project: None })));
        assert!(parse_args(["qrate", "a.qrate", "open", "b.qrate"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "--unknown"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "a.qrate", "b.qrate"]).is_err());
        let cli = Cli::try_parse_from(["qrate", "app", "launch", "--wait"]).unwrap();
        assert!(cli.wait);
        let cli = Cli::try_parse_from(["qrate", "--wait", "app", "launch"]).unwrap();
        assert!(cli.wait);
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
    fn parses_agent_commands() {
        let cli =
            Cli::try_parse_from(["qrate", "agent", "stage-findings", "--agent", "pi"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Agent { agent: Some(ref name), command: AgentCommand::StageFindings }) if name == "pi"
        ));
        assert!(Cli::try_parse_from(["qrate", "agent", "program-run"]).is_ok());
        assert!(Cli::try_parse_from(["qrate", "agent"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "agent", "stage_findings"]).is_err());
    }

    #[test]
    fn frames_agent_requests_from_stdin() {
        assert_eq!(
            agent_request(AgentCommand::Overview, &mut "ignored".as_bytes()).unwrap(),
            r#"{"method":"overview"}"#
        );
        assert_eq!(
            agent_request(
                AgentCommand::ProgramRun,
                &mut r#"{"revision":3}"#.as_bytes()
            )
            .unwrap(),
            r#"{"method":"program_run","params":{"revision":3}}"#
        );
        assert!(matches!(
            agent_request(AgentCommand::Query, &mut "[1]".as_bytes()),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            agent_request(AgentCommand::Query, &mut "".as_bytes()),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn maps_answers_to_exit_reasons() {
        let parsed =
            parse_response("HTTP/1.1 400 Bad Request\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(parsed, (400, "{}".to_owned()));
        assert!(parse_response("garbage").is_err());
        assert_eq!(accept((200, "{}".into())), Ok("{}".into()));
        assert_eq!(
            accept((400, "{}".into())),
            Err(Failure::Refused("{}".into()))
        );
        assert!(matches!(
            accept((401, String::new())),
            Err(Failure::Unavailable(_))
        ));
        assert!(matches!(
            accept((500, String::new())),
            Err(Failure::Other(_))
        ));
    }

    #[test]
    fn resolves_the_desktop_beside_the_cli() {
        let exe = |name: &str| format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let install = Path::new("directory with spaces");
        assert_eq!(
            desktop_path(&install.join(exe("qrate-cli"))),
            install.join(exe("qrate"))
        );
        assert_eq!(
            desktop_path(&install.join("bin").join(exe("qrate"))),
            install.join(exe("qrate"))
        );
        let cargo_bin = Path::new("home/.cargo/bin");
        assert_eq!(
            desktop_path(&cargo_bin.join(exe("qrate-cli"))),
            cargo_bin.join(exe("qrate"))
        );
    }

    #[test]
    fn accepts_links_and_rejects_missing_projects() {
        assert!(validate_project(Path::new("qrate://plugin/install/example")).is_ok());
        assert!(validate_project(Path::new("not-a-project.txt")).is_err());
        assert!(validate_project(Path::new("missing-project.QRATE")).is_err());
    }

    #[test]
    fn reports_missing_desktop() {
        assert!(matches!(
            launch(Path::new("missing-desktop/qrate"), None, false),
            Err(Failure::Other(message)) if message.contains("Install qrate and qrate-cli together")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn waits_and_preserves_native_arguments() {
        use std::os::unix::ffi::OsStringExt;
        let path = std::ffi::OsString::from_vec(b"project with spaces \xff.qrate".to_vec());
        let cli = Cli::try_parse_from([std::ffi::OsString::from("qrate"), path.clone()]).unwrap();
        assert_eq!(cli.project.unwrap().as_os_str(), path);
        let status = launch(Path::new("false"), Some(Path::new(&path)), true)
            .unwrap()
            .unwrap();
        assert_eq!(status.code(), Some(1));
        assert!(
            launch(Path::new("true"), None, true)
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

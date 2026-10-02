use std::{
    fs,
    io::{IsTerminal as _, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitStatus, Stdio},
};

use clap::{CommandFactory as _, Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use serde_json::{Value, json};

const REFUSED: i32 = 1;
const USAGE: i32 = 2;
const UNAVAILABLE: i32 = 3;

const AFTER_HELP: &str = "\
Exit codes:
  0  The command completed.
  1  qrate refused the request, or the command failed. A refusal is JSON on stdout.
  2  The command line or its input was not valid.
  3  qrate is not running or could not be reached.

Examples:
  qrate catalog.qrate                     Open a project.
  qrate app status                        Is qrate running, and with which project?
  qrate project info --format json        The open project, as JSON.
  echo '{\"limit\":5}' | qrate agent query   Five rows of the open project.

Documentation: https://qrate.dvnl.work/docs/cli";

const WAIT_HELP: &str = "Wait for the desktop application to exit and return its exit status";
const FORMAT_HELP: &str = "Defaults to `human` on a terminal and `json` in a pipe";

const AGENT_AFTER_HELP: &str = "\
Every command but `overview` reads one JSON object on stdin and prints one JSON object on stdout.
No command changes a cell: `stage-findings` publishes drafts that only the archivist can apply.

Examples:
  qrate agent overview --agent codex
  echo '{\"source\":{\"kind\":\"selected_rows\"},\"select\":[\"Title\"]}' | qrate agent query
  qrate agent stage-findings --agent codex < findings.json

The full contract: https://github.com/devnull03/qrate/blob/main/AGENTS.md";

/// Open qrate and read the project that is open in it.
///
/// With no command, `qrate` opens the desktop application, and `qrate <PROJECT>` opens a project
/// in it. The other commands talk to the qrate that is already running: they read the project on
/// screen, unsaved edits included, and none of them changes a cell.
#[derive(Debug, Parser)]
#[command(name = "qrate", bin_name = "qrate", version, after_help = AFTER_HELP)]
struct Cli {
    /// A .qrate project file or a qrate:// link to open.
    project: Option<PathBuf>,
    #[arg(long, help = WAIT_HELP)]
    wait: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
enum Format {
    Human,
    Json,
}

impl Format {
    fn json(format: Option<Self>) -> bool {
        format.map_or_else(
            || !std::io::stdout().is_terminal(),
            |format| format == Self::Json,
        )
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open a project, or show the project launcher.
    Open {
        /// A .qrate project file or a qrate:// link to open.
        project: Option<PathBuf>,
        #[arg(long, help = WAIT_HELP)]
        wait: bool,
    },
    /// Inspect or launch the desktop application.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Inspect the project open in qrate.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Read the open project as an agent: JSON in on stdin, JSON out on stdout.
    #[command(after_help = AGENT_AFTER_HELP)]
    Agent {
        /// The name shown in qrate's Agent panel. Defaults to $QRATE_AGENT. A label, not proof.
        #[arg(long, global = true)]
        agent: Option<String>,
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Print a completion script for a shell.
    ///
    /// Load the script from your shell's startup file, for example
    /// `qrate completion bash > ~/.local/share/bash-completion/completions/qrate`.
    Completion { shell: clap_complete::Shell },
    /// Write the manual pages into a directory.
    ///
    /// One page for `qrate` and one for each command, such as `qrate-agent-query.1`. Put the
    /// directory's parent on MANPATH, or write straight into a `man1` directory man already reads.
    Man { directory: PathBuf },
    /// Print the qrate CLI version.
    Version,
}

#[derive(Debug, Subcommand)]
enum AppCommand {
    /// Show whether qrate is running and which project is open.
    ///
    /// Exits 0 either way; read the answer, not the exit code.
    Status {
        #[arg(long, value_enum, help = FORMAT_HELP)]
        format: Option<Format>,
    },
    /// Print the path of the desktop executable this command launches.
    Path,
    /// Launch qrate without opening a project.
    Launch {
        #[arg(long, help = WAIT_HELP)]
        wait: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Show the open project's name, path, source, files folder, and row and column counts.
    ///
    /// Exits 1 when qrate has no project open.
    Info {
        #[arg(long, value_enum, help = FORMAT_HELP)]
        format: Option<Format>,
    },
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum AgentCommand {
    /// Project, columns, selection, diagnostic counts and revision. Takes no input.
    ///
    /// Start here, and keep `revision`: `program-run` and `stage-findings` need that exact number.
    Overview,
    /// Bounded rows or diagnostics.
    ///
    /// stdin: {"source": {"kind": "all_rows" | "selected_rows" | "rows" | "search" |
    /// "diagnostics"}, "select": [...], "where": [...], "distinct": ..., "group_by": [...],
    /// "order_by": {...}, "limit": 1-50, "cursor": ...}. Every field is optional; `{}` is the
    /// first 20 rows.
    Query,
    /// Validate and activate a confined Luau program without running it.
    ///
    /// stdin: {"source": "<Luau>"}. The program has no network, filesystem, process, or clock.
    ProgramSave,
    /// Run the saved program once at an exact revision.
    ///
    /// stdin: {"revision": <from overview>, "args": <any JSON>}.
    ProgramRun,
    /// Up to four 512-pixel PNG thumbnails.
    ///
    /// stdin: {"items": [{"row": <source row>, "page": <page, default 0>}]}.
    Thumbnails,
    /// Publish a complete batch of draft findings. Never changes a cell.
    ///
    /// stdin: {"revision": <from overview>, "findings": [{"row", "column", "severity",
    /// "message", "expected", "replacement"?}]}. The batch replaces this agent's earlier drafts.
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
            print(env!("CARGO_PKG_VERSION"));
            Ok(None)
        }
        Some(Command::Completion { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "qrate", &mut std::io::stdout());
            Ok(None)
        }
        Some(Command::Man { directory }) => fs::create_dir_all(&directory)
            .and_then(|()| clap_mangen::generate_to(Cli::command(), &directory))
            .map(|()| None)
            .map_err(|error| {
                Failure::Other(format!(
                    "cannot write manual pages to {}: {error}",
                    directory.display()
                ))
            }),
        Some(Command::App {
            command: AppCommand::Status { format },
        }) => app_status(Format::json(format)),
        Some(Command::App {
            command: AppCommand::Path,
        }) => executable().map(|executable| {
            print(&display_path(&desktop_path(&executable)));
            None
        }),
        Some(Command::App {
            command: AppCommand::Launch { wait },
        }) => executable()
            .and_then(|executable| launch(&desktop_path(&executable), None, cli.wait || wait)),
        Some(Command::Project {
            command: ProjectCommand::Info { format },
        }) => project_info(Format::json(format)),
        Some(Command::Agent { command, .. })
            if !matches!(command, AgentCommand::Overview) && std::io::stdin().is_terminal() =>
        {
            Err(Failure::Usage(
                "this command reads a JSON object on stdin; pipe one in, or see `qrate help agent`"
                    .into(),
            ))
        }
        Some(Command::Agent { agent, command }) => agent_call(
            command,
            agent.or_else(|| std::env::var("QRATE_AGENT").ok()),
            &mut std::io::stdin(),
        ),
        Some(Command::Open { project, wait }) => open(project, cli.wait || wait),
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
            print(&body);
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

/// `println!` panics when the reader has gone, which is ordinary for `qrate … | head`.
fn print(text: &str) {
    if writeln!(std::io::stdout(), "{text}").is_err() {
        std::process::exit(1);
    }
}

/// Windows canonical paths carry a `\\?\` prefix that cmd.exe and most tools refuse.
fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(plain) if !plain.starts_with(r"UNC\") => plain.to_owned(),
        _ => text.into_owned(),
    }
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

fn project_info(json: bool) -> Result<Option<ExitStatus>, Failure> {
    let body = match app_request("GET", "/v1/project/info", None, None)? {
        (409, _) if !json => {
            return Err(Failure::Other(
                "no project is active; run `qrate <PROJECT.qrate>`".into(),
            ));
        }
        response => accept(response)?,
    };
    let info: Value = serde_json::from_str(&body)
        .map_err(|error| Failure::Other(format!("invalid qrate project info: {error}")))?;
    print(&project_report(&info["project"], json));
    Ok(None)
}

fn project_report(project: &Value, json: bool) -> String {
    if json {
        return project.to_string();
    }
    let text = |key: &str, missing: &str| project[key].as_str().unwrap_or(missing).to_owned();
    [
        format!("name: {}", text("name", "unknown")),
        format!("path: {}", text("path", "unknown")),
        format!("source: {}", text("source", "none")),
        format!("created: {}", text("created_at", "unknown")),
        format!("link method: {}", text("link_method", "none")),
        format!("files folder: {}", text("files_folder", "none")),
        format!("rows: {}", project["row_count"]),
        format!("columns: {}", project["column_count"]),
    ]
    .join("\n")
}

fn app_status(json: bool) -> Result<Option<ExitStatus>, Failure> {
    let project = match app_request("GET", "/v1/status", None, None) {
        Err(Failure::Unavailable(_)) => None,
        response => {
            let status: Value = serde_json::from_str(&accept(response?)?)
                .map_err(|error| Failure::Other(format!("invalid qrate status: {error}")))?;
            Some(status["project"].clone())
        }
    };
    print(&status_report(project, json));
    Ok(None)
}

/// `project` is `None` when qrate is not running, and JSON null when it has no project open.
fn status_report(project: Option<Value>, json: bool) -> String {
    match (project, json) {
        (project, true) => {
            json!({ "running": project.is_some(), "project": project.unwrap_or_default() })
                .to_string()
        }
        (None, false) => "stopped".into(),
        (Some(project), false) => {
            format!("running\nproject: {}", project.as_str().unwrap_or("none"))
        }
    }
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
    print(&body);
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
    use clap::{CommandFactory as _, Parser as _};
    use serde_json::json;

    use super::{
        AgentCommand, AppCommand, Cli, Command, Failure, Format, ProjectCommand, accept,
        agent_request, desktop_path, display_path, launch, parse_args, parse_response,
        project_report, status_report, validate_project,
    };
    use std::path::Path;

    #[test]
    fn the_command_tree_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn shows_a_path_other_tools_accept() {
        let shown = |path: &str| display_path(Path::new(path));
        assert_eq!(shown(r"\\?\C:\qrate\qrate.exe"), r"C:\qrate\qrate.exe");
        assert_eq!(shown(r"\\?\UNC\host\share\q"), r"\\?\UNC\host\share\q");
        assert_eq!(shown("/opt/qrate/qrate"), "/opt/qrate/qrate");
    }

    #[test]
    fn reports_status_and_project_in_both_formats() {
        assert_eq!(status_report(None, false), "stopped");
        assert_eq!(
            status_report(None, true),
            r#"{"running":false,"project":null}"#
        );
        assert_eq!(
            status_report(Some(json!(null)), false),
            "running\nproject: none"
        );
        assert_eq!(
            status_report(Some(json!("a.qrate")), true),
            r#"{"running":true,"project":"a.qrate"}"#
        );
        let project = json!({ "name": "a", "path": "a.qrate", "row_count": 3, "column_count": 2 });
        assert_eq!(project_report(&project, true), project.to_string());
        assert_eq!(
            project_report(&project, false).lines().collect::<Vec<_>>(),
            [
                "name: a",
                "path: a.qrate",
                "source: none",
                "created: unknown",
                "link method: none",
                "files folder: none",
                "rows: 3",
                "columns: 2"
            ]
        );
        let cli = Cli::try_parse_from(["qrate", "project", "info", "--format", "json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Project {
                command: ProjectCommand::Info {
                    format: Some(Format::Json)
                }
            })
        ));
        assert!(Format::json(Some(Format::Json)) && !Format::json(Some(Format::Human)));
        assert!(Cli::try_parse_from(["qrate", "app", "status", "--format", "xml"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "agent", "overview", "--format", "json"]).is_err());
    }

    #[test]
    fn writes_completions_and_a_manual_page_per_command() {
        let mut script = Vec::new();
        clap_complete::generate(
            clap_complete::Shell::Bash,
            &mut Cli::command(),
            "qrate",
            &mut script,
        );
        assert!(
            String::from_utf8(script)
                .unwrap()
                .contains("stage-findings")
        );
        assert!(Cli::try_parse_from(["qrate", "completion", "powershell"]).is_ok());
        assert!(Cli::try_parse_from(["qrate", "completion", "cmd"]).is_err());

        let directory = std::env::temp_dir().join(format!("qrate man test {}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        clap_mangen::generate_to(Cli::command(), &directory).unwrap();
        let page = std::fs::read_to_string(directory.join("qrate.1")).unwrap();
        assert!(page.contains("Exit codes"));
        assert!(directory.join("qrate-agent-query.1").is_file());
        std::fs::remove_dir_all(&directory).unwrap();
    }

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
            Some(Command::Open {
                project: Some(_),
                wait: true
            })
        ));
        assert!(Cli::try_parse_from(["qrate", "open"]).is_ok());
        let cli = Cli::try_parse_from(["qrate", "--wait", "open"]).unwrap();
        assert!(
            cli.wait
                && matches!(
                    cli.command,
                    Some(Command::Open {
                        project: None,
                        wait: false
                    })
                )
        );
        assert!(parse_args(["qrate", "a.qrate", "open", "b.qrate"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "--unknown"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "a.qrate", "b.qrate"]).is_err());
        let cli = Cli::try_parse_from(["qrate", "app", "launch", "--wait"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::App {
                command: AppCommand::Launch { wait: true }
            })
        ));
        let cli = Cli::try_parse_from(["qrate", "--wait", "app", "launch"]).unwrap();
        assert!(cli.wait);
        assert!(Cli::try_parse_from(["qrate", "agent", "overview", "--wait"]).is_err());
        assert!(Cli::try_parse_from(["qrate", "app", "status"]).is_ok());
        assert!(Cli::try_parse_from(["qrate", "app", "path"]).is_ok());
        let cli = Cli::try_parse_from(["qrate", "project", "info"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Project {
                command: ProjectCommand::Info { format: None }
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

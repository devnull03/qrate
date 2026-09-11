use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "qrate",
    version,
    about = "Inspect and automate qrate projects",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the qrate CLI version.
    Version,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Version => println!("{}", env!("CARGO_PKG_VERSION")),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{Cli, Command};

    #[test]
    fn parses_version_command() {
        let cli = Cli::try_parse_from(["qrate", "version"]).unwrap();
        assert!(matches!(cli.command, Command::Version));
    }

    #[test]
    fn requires_a_command() {
        let error = Cli::try_parse_from(["qrate"]).unwrap_err();
        assert_eq!(error.exit_code(), 2);
    }
}

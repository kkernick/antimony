/// Antimony's CLI.
pub mod config;
pub mod edit;
pub mod export;
pub mod import;
pub mod info;
pub mod integrate;
pub mod refresh;
pub mod remove;
pub mod run;
pub mod seccomp;

use anyhow::Result;
use clap::{Parser, Subcommand};
use enum_dispatch::enum_dispatch;

/// Create run arguments from subcommand passthrough.
pub fn run_vec(profile: &str, mut passthrough: Vec<String>) -> run::Args {
    let mut command: Vec<String> = vec!["antimony", "run", profile]
        .into_iter()
        .map(String::from)
        .collect();

    command.append(&mut passthrough);
    let cli = Cli::parse_from(command);
    match cli.command {
        Command::Run(args) => args,
        _ => unreachable!(),
    }
}

#[derive(Parser, Debug, Default)]
#[command(name = "Antimony")]
#[command(version)]
#[command(about = "Sandbox Applications")]
#[command(before_help = r#"⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⠟⠹⣧⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⣷⣦⣄⣠⣿⠃⢠⣄⠈⢻⣆⣠⣴⡞⡆⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⢀⣀⣀⣿⠀⠈⢻⣇⢀⣾⢟⡄⣸⡿⠋⠀⡇⣇⣀⣀⠀⠀⠀⠀⠀
⠀⣤⣤⣤⣀⣱⢻⠚⠻⣧⣀⠀⢹⡿⠃⠈⢻⣟⠀⢀⣤⠧⠓⣹⣟⣀⣤⣤⣤⡀
⠀⠈⠻⣧⠉⠛⣽⠀⠀⠀⠙⣷⡿⠁⠀⠀⠀⢻⣶⠛⠁⠀⠀⡟⠟⠉⣵⡟⠁⠀
⠀⠀⠀⠹⣧⡀⠏⡇⠀⠀⠀⣿⠁⠀⠀⠀⠀⠀⣿⡄⠀⠀⢠⢷⠀⣼⡟⠀⠀⠀
⠀⠀⠀⠀⠙⣟⢼⡹⡄⠀⠀⣿⡄⠀⠀⠀⠀⢀⣿⡇⠀⢀⣞⣦⢾⠟⠀⠀⠀⠀
⠀⠠⢶⣿⣛⠛⢒⣭⢻⣶⣤⣹⣿⣤⣀⣀⣠⣾⣟⣠⣔⡛⢫⣐⠛⢛⣻⣶⠆⠀
⠀⠀⠀⠉⣻⡽⠛⠉⠁⠀⠉⢙⣿⠖⠒⠛⠻⣿⡋⠉⠁⠈⠉⠙⢿⣿⠉⠀⠀⠀
⠀⠀⠀⠸⠿⠷⠒⣦⣤⣴⣶⢿⣿⡀⠀⠀⠀⣽⡿⢷⣦⠤⢤⡖⠶⠿⠧⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠛⢿⣦⣴⡾⠟⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠙⠟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"#)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
#[enum_dispatch(Run)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Run a profile
    Run(run::Args),

    /// Edit an existing profile
    Edit(edit::Args),

    /// Refresh caches
    Refresh(refresh::Args),

    /// Integrate a profile into the user environment.
    Integrate(integrate::Args),

    /// Reset a profile back to the system-defined profile.
    Remove(remove::Args),

    /// List installed profiles and features
    Info(info::Args),

    /// Perform operations on the SECCOMP Database.
    Seccomp(seccomp::Args),

    /// Export user profiles.
    Export(export::Args),

    /// Import user profiles.
    Import(import::Args),

    /// Import user profiles.
    Config(config::Args),
}
impl Default for Command {
    fn default() -> Self {
        Self::Run(run::Args::default())
    }
}

#[enum_dispatch]
pub trait Run {
    fn run(self) -> Result<()>;
}

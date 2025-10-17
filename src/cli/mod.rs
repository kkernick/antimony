/// Antimony's CLI.
pub mod create;
pub mod debug_shell;
pub mod default;
pub mod edit;
pub mod feature;
pub mod info;
pub mod integrate;
pub mod refresh;
pub mod reset;
pub mod run;
pub mod seccomp;
pub mod stat;
pub mod trace;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
pub enum Command {
    /// Run a profile
    Run(Box<run::Args>),

    /// Create a new profile
    Create(create::Args),

    /// Edit an existing profile
    Edit(edit::Args),

    /// Edit the default profile
    Default(default::Args),

    /// Modify the system features.
    Feature(feature::Args),

    /// Refresh caches
    Refresh(refresh::Args),

    /// Integrate a profile into the user environment.
    Integrate(integrate::Args),

    /// Reset a profile back to the system-defined profile.
    Reset(reset::Args),

    /// Trace a profile for missing syscalls or files.
    Trace(trace::Args),

    /// Collect stats about a profile's sandbox
    Stat(stat::Args),

    /// List installed profiles and features
    Info(info::Args),

    /// Drop into a debugging shell within a profile's sandbox
    DebugShell(debug_shell::Args),

    /// Perform operations on the SECCOMP Database.
    Seccomp(seccomp::Args),
}
impl Run for Command {
    fn run(self) -> Result<()> {
        match self {
            Command::Run(args) => args.run(),
            Command::Create(args) => args.run(),
            Command::Edit(args) => args.run(),
            Command::Default(args) => args.run(),
            Command::Feature(args) => args.run(),
            Command::Refresh(args) => args.run(),
            Command::Integrate(args) => args.run(),
            Command::Reset(args) => args.run(),
            Command::Trace(args) => args.run(),
            Command::Stat(args) => args.run(),
            Command::Info(args) => args.run(),
            Command::DebugShell(args) => args.run(),
            Command::Seccomp(args) => args.run(),
        }
    }
}
impl Default for Command {
    fn default() -> Self {
        Self::Run(Box::default())
    }
}

pub trait Run {
    fn run(self) -> Result<()>;
}

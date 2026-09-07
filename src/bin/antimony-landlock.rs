//! This application runs a program underneath Landlock.
#![allow(unused_crate_dependencies)]

use std::fs;

use antimony::shared::landlock::LandlockPolicy;
use anyhow::Result;
use clap::{Parser, ValueHint};
use spawn::Spawner;

#[derive(Parser, Default)]
#[command(name = "Antimony-Landlock")]
#[command(version)]
#[command(about = "Run programs under landlock")]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// The command to run.
    #[arg(value_hint = ValueHint::CommandName)]
    pub command: String,

    /// The path to the policy document.
    #[arg(short, long, value_hint = ValueHint::FilePath, env = "LANDLOCK_POLICY")]
    pub policy: String,

    /// Passthrough arguments.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}

fn main() -> Result<()> {
    // Try and init the logger
    let _ = notify::init();
    let cli = Cli::parse();

    let policy: LandlockPolicy = toml::from_str(&fs::read_to_string(cli.policy)?)?;
    let ruleset = policy.construct_ruleset()?;

    let handle = Spawner::new(cli.command)?;
    if let Some(args) = cli.passthrough {
        handle.args_i(args);
    }
    handle
        .landlock(ruleset)
        .preserve_env(true)
        .spawn()?
        .wait()?;

    Ok(())
}

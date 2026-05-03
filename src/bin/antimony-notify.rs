#![allow(unused_crate_dependencies)]
//! This file compiles into an interface to the Notification Portal.
//! Effectively, it's notify-send, but in Rust.

use clap::Parser;
use notify::Error;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "Antimony Notify")]
#[command(version)]
#[command(about = "Notify user-space via the Notification Portal")]
pub struct Cli {
    /// The title of the notification
    #[arg(long)]
    title: String,

    /// The message body
    #[arg(long)]
    body: String,

    /// A timeout, in milliseconds
    #[arg(long)]
    timeout: Option<u64>,

    /// The urgency of the notification
    #[arg(long)]
    urgency: Option<notify::Urgency>,

    /// A set of actions to prompt the user with,
    /// in the format Key=Value. If no actions,
    /// this program returns nothing. If
    /// actions are defined, returns the Key
    /// of the selected action.
    #[arg(long)]
    action: Vec<String>,
}

fn main() -> Result<(), Error> {
    let cli = Cli::parse();
    let timeout = cli.timeout.map(Duration::from_millis);

    if cli.action.is_empty() {
        notify::notify(cli.title, cli.body, timeout, cli.urgency)
    } else {
        let r: Vec<(String, String)> = cli
            .action
            .iter()
            .map(|pair| {
                if let Some((key, value)) = pair.split_once('=') {
                    (key, value)
                } else {
                    (pair.as_str(), pair.as_str())
                }
            })
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();

        let action = notify::action(cli.title, cli.body, timeout, cli.urgency, r)?;
        println!("{action}");
        Ok(())
    }
}

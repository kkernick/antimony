//! Drop into a debugging shell within the sandbox.
use crate::{cli::run::wait_for_doc, setup::setup};
use anyhow::{Result, anyhow};
use log::debug;
use nix::sys::signal::Signal::SIGTERM;
use std::borrow::Cow;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of the profile
    pub profile: String,

    /// Use a configuration within the profile.
    #[arg(short, long)]
    pub config: Option<String>,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let mut args = super::run::Args {
            binaries: Some(
                vec!["sh", "cat", "ls"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
            config: self.config,
            ..Default::default()
        };

        match setup(Cow::Borrowed(&self.profile), &mut args) {
            Ok(info) => debug_shell(info),
            Err(e) => Err(anyhow!("Failed to run profile: {e}")),
        }
    }
}

fn debug_shell(info: crate::setup::Info) -> Result<()> {
    info.handle.arg_i("sh")?;

    debug!("Waiting for document portal");
    wait_for_doc();

    debug!("Spawning");
    info.handle.spawn()?.wait_for_signal(SIGTERM)?;
    crate::setup::cleanup(info.instance)
}

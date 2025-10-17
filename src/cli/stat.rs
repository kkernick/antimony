//! Get statistics about the sandbox.
use crate::setup::setup;
use anyhow::{Result, anyhow};
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
                vec!["find", "ls", "wc"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
            config: self.config,
            ..Default::default()
        };

        match setup(Cow::Borrowed(&self.profile), &mut args) {
            Ok(info) => stat(info),
            Err(e) => Err(anyhow!("Failed to run profile: {e}")),
        }
    }
}

fn stat(info: crate::setup::Info) -> Result<()> {
    info.handle.args_i([
            "sh",
            "-c",
            "
                echo \"/etc => $(find /etc -type f | wc -l)\";
                for dir in $(ls /etc); do echo \"   $dir => $(find /etc/$dir -type f | wc -l)\"; done;
                for dir in /usr/bin /usr/lib /usr/share; do echo \"$dir => $(find $dir -type f | wc -l)\"; done;
                for dir in $(ls /usr/share); do echo \"    $dir => $(find /usr/share/$dir -type f | wc -l)\"; done;
            ",
        ])?;
    info.handle.spawn()?.wait()?;
    crate::setup::cleanup(info.instance)
}

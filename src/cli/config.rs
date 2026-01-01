//! Edit the default profile
use crate::shared::{config::ConfigFile, env::AT_HOME, privileged};
use anyhow::Result;
use log::{error, trace};
use std::fs;
use user::as_effective;

#[derive(clap::Args, Debug, Default)]
pub struct Args {}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        if !privileged()? {
            Err(anyhow::anyhow!(
                "Modifying the configuration file is a privileged operation."
            ))
        } else {
            let path = {
                let path = AT_HOME.join("config.toml");
                if !path.exists() {
                    as_effective!({
                        fs::copy(AT_HOME.join("config").join("config.toml"), &path)
                    })??;
                }
                path
            };

            trace!("Editing");
            if let Err(e) = ConfigFile::edit(&path) {
                error!("Failed to edit config: {e}");
                as_effective!(fs::remove_file(&path))??;
                return Err(e.into());
            }
            Ok(())
        }
    }
}

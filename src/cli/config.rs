//! Edit the configuration file.

use crate::shared::{
    config::{CONFIG_FILE, ConfigFile},
    env::AT_HOME,
    privileged,
};
use anyhow::Result;
use log::trace;
use std::{fs, ops::Deref};

#[derive(clap::Args, Default)]
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
                    fs::copy(AT_HOME.join("config").join("config.toml"), &path)?;
                }
                path
            };

            trace!("Editing");
            if let Some(out) = ConfigFile::edit(&toml::to_string(CONFIG_FILE.deref())?)? {
                fs::write(path, out)?;
            }
            Ok(())
        }
    }
}

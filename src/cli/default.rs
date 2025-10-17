//! Edit the default profile
use crate::aux::{env::AT_HOME, profile::Profile};
use anyhow::Result;
use std::fs::File;

#[derive(clap::Args, Debug, Default)]
pub struct Args {}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        let path = {
            let path = Profile::default_profile();
            if !path.exists() {
                std::fs::copy(AT_HOME.join("config").join("default.toml"), &path)?;
            }
            path
        };

        if !path.exists() {
            File::create(&path)?;
        }

        if Profile::edit(&path).is_err() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

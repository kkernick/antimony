//! Edit the default profile
use crate::shared::{
    env::AT_HOME,
    profile::{self, Profile},
};
use anyhow::Result;
use log::{error, trace};
use std::fs::{self, File};

#[derive(clap::Args, Debug, Default)]
pub struct Args {}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        user::set(user::Mode::Effective)?;
        let path = {
            let path = Profile::default_profile();
            if let Some(parent) = path.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }

            if !path.exists() {
                fs::copy(AT_HOME.join("config").join("default.toml"), &path)?;
            }
            path
        };

        if !path.exists() {
            File::create(&path)?;
        }

        trace!("Editing");
        if let Err(e) = Profile::edit(&path) {
            fs::remove_file(&path)?;
            error!("Failed to edit default: {e}");
            return Err(e.into());
        } else {
            fs::remove_dir_all(profile::CACHE_DIR.as_path())?;
        }
        Ok(())
    }
}

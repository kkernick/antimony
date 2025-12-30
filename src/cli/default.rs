//! Edit the default profile
use crate::shared::{
    env::AT_HOME,
    profile::{self, Profile},
};
use anyhow::Result;
use log::{error, trace};
use std::fs;
use user::as_effective;

#[derive(clap::Args, Debug, Default)]
pub struct Args {}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        let path = {
            let path = Profile::default_profile();
            if let Some(parent) = path.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }

            if !path.exists() {
                as_effective!({ fs::copy(AT_HOME.join("config").join("default.toml"), &path) })??;
            }
            path
        };

        trace!("Editing");
        if let Err(e) = Profile::edit(&path) {
            error!("Failed to edit default: {e}");
            as_effective!(fs::remove_file(&path))??;
            return Err(e.into());
        } else {
            as_effective!({ fs::remove_dir_all(profile::CACHE_DIR.as_path()) })??;
        }
        Ok(())
    }
}

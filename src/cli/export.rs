//! Export user-profiles
use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow};

use crate::aux::env::{AT_HOME, PWD, USER_NAME};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of a profile to export. If absent, export all user-profiles.
    profile: Option<String>,

    /// Where to export to. Defaults to current directory
    dest: Option<String>,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        user::drop(user::Mode::Real)?;

        let profiles = AT_HOME
            .join("config")
            .join(USER_NAME.as_str())
            .join("profiles");

        let dest = match self.dest {
            Some(path) => PathBuf::from(path),
            None => PWD.clone(),
        };

        if !dest.exists() {
            Err(anyhow!("Destination does not exist"))
        } else if let Some(profile) = self.profile {
            let source = profiles.join(&profile).with_extension("toml");
            if !source.exists() {
                Err(anyhow!("No such profile"))
            } else {
                fs::copy(source, dest.join(profile).with_extension("toml"))?;
                Ok(())
            }
        } else {
            let dest = dest.join("profiles");
            if !dest.exists() {
                fs::create_dir_all(&dest)?;
            }
            for profile in profiles.read_dir()?.filter_map(|e| e.ok()) {
                fs::copy(profile.path(), dest.join(profile.file_name()))?;
            }
            Ok(())
        }
    }
}

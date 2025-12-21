//! Import user-profiles
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use dialoguer::Confirm;
use log::warn;

use crate::shared::{
    env::{AT_HOME, USER_NAME},
    profile::Profile,
};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The path of the profile. Can also be a directory, which will import all files within.
    profile: String,

    /// Overwrite existing entries
    #[arg(short, long, default_value_t = false)]
    overwrite: bool,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        user::set(user::Mode::Effective)?;

        let profile = PathBuf::from(self.profile);
        let dest = AT_HOME
            .join("config")
            .join(USER_NAME.as_str())
            .join("profiles");

        if !dest.exists() {
            fs::create_dir_all(&dest)?;
        }

        let import = |src: &Path, dst: &Path| -> Result<()> {
            if Profile::new(&src.to_string_lossy()).is_ok() {
                let dest = dst.join(src.file_name().unwrap());
                if dest.exists()
                    && !self.overwrite
                    && !Confirm::new()
                        .with_prompt(format!("Profile {dest:?} already exists. Overwrite?"))
                        .interact()?
                {
                    return Ok(());
                }
                fs::copy(src, dst.join(src.file_name().unwrap()))?;
            } else {
                warn!("Invalid profile: {profile:?}");
            }
            Ok(())
        };

        if profile.is_dir() {
            for profile in profile.read_dir()?.filter_map(|e| e.ok()) {
                import(&profile.path(), &dest)?;
            }
            Ok(())
        } else if profile.is_file() {
            import(&profile, &dest)
        } else {
            Err(anyhow!("No such profile!"))
        }
    }
}

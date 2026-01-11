//! Import user-profiles

use crate::shared::profile::Profile;
use anyhow::{Result, anyhow};
use log::warn;
use std::{fs, path::Path};

#[derive(clap::Args)]
pub struct Args {
    /// The path of the profile. Can also be a directory, which will import all files within.
    profile: String,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let import = |src: &Path| -> Result<()> {
            let name = src.to_string_lossy();
            if let Ok(profile) = Profile::load(&name) {
                let user_profile =
                    Profile::user_profile(src.file_name().unwrap().to_str().unwrap());
                if let Some(parent) = user_profile.parent()
                    && !parent.exists()
                {
                    fs::create_dir_all(parent)?;
                }

                fs::write(user_profile, toml::to_string(&profile)?)?;
            } else {
                warn!("Invalid profile: {}", src.display());
            }
            Ok(())
        };

        let profile = Path::new(&self.profile);
        if profile.is_dir() {
            for profile in profile.read_dir()?.filter_map(|e| e.ok()) {
                import(&profile.path())?;
            }
            Ok(())
        } else if profile.is_file() {
            import(profile)
        } else {
            Err(anyhow!("No such profile!"))
        }
    }
}

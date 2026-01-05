//! Import user-profiles

use crate::shared::{
    db::{self, Database, Table},
    profile::Profile,
};
use anyhow::{Result, anyhow};
use log::warn;
use std::path::Path;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The path of the profile. Can also be a directory, which will import all files within.
    profile: String,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let import = |src: &Path| -> Result<()> {
            if let Ok(profile) = Profile::load(&src.to_string_lossy()) {
                db::save(
                    &src.file_stem().unwrap().to_string_lossy(),
                    &profile,
                    Database::User,
                    Table::Profiles,
                )?
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

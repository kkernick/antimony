//! Import user-profiles

use crate::shared::{
    feature::{self, Feature},
    profile::{self, Profile},
    store::{self, Object},
};
use anyhow::{Result, anyhow};
use log::warn;
use std::path::Path;

#[derive(clap::Args)]
pub struct Args {
    /// The path of the object. Can also be a directory, which will import all files within.
    name: String,

    /// Target the feature set rather than the profile set.
    #[arg(long, default_value_t = false)]
    feature: bool,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let import = |src: &Path| -> Result<()> {
            let name = src.to_string_lossy();
            let content = if self.feature {
                toml::to_string(&store::load::<Feature, feature::Error>(&name, table, true)?)
            } else {
                toml::to_string(&store::load::<Profile, profile::Error>(&name, table, true)?)
            };

            if let Ok(content) = content {
                store::USER_STORE.borrow().store(
                    src.file_name().unwrap().to_str().unwrap(),
                    table,
                    &content,
                )?;
            } else {
                warn!("Invalid {kind}: {}", src.display());
            }
            Ok(())
        };

        let profile = Path::new(&self.name);
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

//! Edit an existing profile.
use crate::shared::{env::AT_HOME, feature::Feature, privileged, profile};
use anyhow::{Result, anyhow};
use dialoguer::Confirm;
use std::fs;

#[derive(clap::Args, Debug, Default)]
pub struct Args {
    /// The name of the feature
    pub feature: String,

    /// Delete a feature. It cannot be recovered. DANGEROUS!
    #[arg(short, long, default_value_t = false)]
    pub delete: bool,
}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        if !privileged()? {
            return Err(anyhow!(
                "Modifying the system feature set is a privileged operation"
            ));
        } else {
            user::set(user::Mode::Effective)?;

            // Edit the feature
            let feature = AT_HOME
                .join("features")
                .join(&self.feature)
                .with_extension("toml");

            let new = !feature.exists();
            if new {
                if self.delete {
                    return Err(anyhow!("Requested feature does not exist!"));
                }
                if let Some(parent) = feature.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(AT_HOME.join("config").join("feature.toml"), &feature)?;
            } else if self.delete {
                let confirm = Confirm::new()
                    .with_prompt(format!("Are you sure you want to delete {}?", self.feature))
                    .interact()?;
                if confirm {
                    println!("Deleting {}", feature.display());
                    fs::remove_file(&feature)?;
                }
                return Ok(());
            }

            // Edit it.
            if Feature::edit(&feature)?.is_none() && new {
                // If there was no modifications, delete the empty feature
                fs::remove_file(feature)?;
            } else {
                fs::remove_dir_all(profile::CACHE_DIR.as_path())?;
            }
        }
        Ok(())
    }
}

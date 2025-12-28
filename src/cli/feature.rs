//! Edit an existing profile.
use std::fs;

use anyhow::{Result, anyhow};
use dialoguer::Confirm;
use nix::unistd::getpid;
use spawn::Spawner;
use user::try_run_as;

use crate::shared::{env::AT_HOME, feature::Feature, profile};

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
        let result = try_run_as!(user::Mode::Real, Result<i32>, {
            Ok(Spawner::abs("/usr/bin/pkcheck")
                .args([
                    "--action-id",
                    "org.freedesktop.policykit.exec",
                    "--allow-user-interaction",
                    "--process",
                    &format!("{}", getpid().as_raw()),
                ])?
                .spawn()?
                .wait()?)
        })?;

        if result != 0 {
            return Err(anyhow!(
                "Administrative privilege and Polkit is required to modify the system feature set!"
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
                    println!("Deleting {feature:?}");
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

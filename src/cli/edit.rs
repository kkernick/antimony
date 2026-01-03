//! Edit profiles/features, Create New Ones, and Modify the Default.

use crate::{
    cli::{self},
    shared::{
        env::AT_HOME,
        feature::Feature,
        privileged,
        profile::{self, Profile},
    },
};
use std::fs;

#[derive(clap::Args, Debug, Default)]
pub struct Args {
    /// The object to edit.
    name: String,

    /// Target a feature, rather than a profile. Requires privilege.
    #[arg(long)]
    pub feature: bool,
}
impl cli::Run for Args {
    fn run(self) -> anyhow::Result<()> {
        user::set(user::Mode::Effective)?;

        if self.feature {
            if !privileged()? {
                Err(anyhow::anyhow!(
                    "Modifying the system feature set is a privileged operation"
                ))
            } else {
                // Edit the feature
                let feature = AT_HOME
                    .join("features")
                    .join(&self.name)
                    .with_extension("toml");

                let new = !feature.exists();
                if new {
                    if let Some(parent) = feature.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(AT_HOME.join("config").join("feature.toml"), &feature)?;
                }

                // Edit it.
                if Feature::edit(&feature)?.is_none() && new {
                    // If there was no modifications, delete the empty feature
                    fs::remove_file(feature)?;
                } else {
                    fs::remove_dir_all(profile::CACHE_DIR.as_path())?;
                }

                Ok(())
            }
        } else if self.name == "default" {
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

            if let Err(e) = Profile::edit(&path) {
                fs::remove_file(&path)?;
                return Err(e.into());
            } else {
                fs::remove_dir_all(profile::CACHE_DIR.as_path())?;
            }
            Ok(())
        } else {
            user::set(user::Mode::Effective)?;

            // Edit the profile
            let user = Profile::user_profile(&self.name);
            let new = !user.exists();
            if new {
                if let Some(parent) = user.parent()
                    && !parent.exists()
                {
                    fs::create_dir_all(parent)?;
                }

                match Profile::path(&self.name) {
                    Ok(source) => {
                        if source.exists() {
                            fs::copy(source, &user)?;
                        }
                    }
                    Err(profile::Error::NotFound(_, _)) => {
                        fs::copy(AT_HOME.join("config").join("new.toml"), &user)?;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to get profile information: {e}"));
                    }
                }
            }

            // Edit it.
            if Profile::edit(&user)?.is_none() && new {
                // If there was no modifications, delete the profile
                // since it's identical to the system one.
                fs::remove_file(user)?;
            }
            Ok(())
        }
    }
}

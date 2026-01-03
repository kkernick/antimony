//! Reset a user profile back to the system default.

use crate::{
    cli,
    shared::{
        env::{AT_HOME, USER_NAME},
        privileged,
        profile::Profile,
    },
};
use anyhow::{Result, anyhow};
use dialoguer::Confirm;
use std::fs;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of the profile. If absent, resets profiles that are identical to the system.
    pub name: Option<String>,

    /// Target a feature, rather than a profile. Requires privilege.
    #[arg(long)]
    pub feature: bool,
}
impl cli::Run for Args {
    fn run(self) -> Result<()> {
        if self.feature {
            if !privileged()? {
                Err(anyhow::anyhow!(
                    "Modifying the system feature set is a privileged operation"
                ))
            } else if let Some(name) = &self.name {
                user::set(user::Mode::Effective)?;

                // Edit the feature
                let feature = AT_HOME.join("features").join(name).with_extension("toml");

                let new = !feature.exists();
                if new {
                    if let Some(parent) = feature.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(AT_HOME.join("config").join("feature.toml"), &feature)?;
                }

                let confirm = Confirm::new()
                    .with_prompt(format!("Are you sure you want to delete {}?", name))
                    .interact()?;
                if confirm {
                    println!("Deleting {}", feature.display());
                    fs::remove_file(&feature)?;
                }

                Ok(())
            } else {
                Err(anyhow::anyhow!("Specify a feature!"))
            }
        } else if let Some(name) = self.name {
            let dest = Profile::user_profile(&name);
            if dest.exists() {
                let system = Profile::system_profile(&name);
                if !system.exists() {
                    let confirm = Confirm::new()
                        .with_prompt(format!(
                            "{name} is a user-created profile. There is \
                        no system default to reset to. Are you sure you want to remove it?`",
                        ))
                        .interact()?;

                    if confirm {
                        fs::remove_file(dest)?;
                    }
                } else {
                    fs::remove_file(dest)?;
                }
                Ok(())
            } else {
                Err(anyhow!("{name} does not exist"))
            }
        } else {
            let profiles = AT_HOME
                .join("config")
                .join(USER_NAME.as_str())
                .join("profiles");
            let system = AT_HOME.join("profiles");
            if profiles.exists() {
                for profile in profiles.read_dir()?.filter_map(|e| e.ok()) {
                    let s_profile = system.join(profile.file_name());
                    if s_profile.exists()
                        && fs::read_to_string(profile.path())? == fs::read_to_string(s_profile)?
                    {
                        println!(
                            "Removing identical user profile {}",
                            profile.path().display()
                        );
                        fs::remove_file(profile.path())?;
                    }
                }
            }
            Ok(())
        }
    }
}

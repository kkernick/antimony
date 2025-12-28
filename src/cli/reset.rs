//! Reset a user profile back to the system default.
use crate::shared::{
    env::{AT_HOME, USER_NAME},
    profile::Profile,
};
use anyhow::{Result, anyhow};
use dialoguer::Confirm;
use std::fs;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of the profile. If absent, resets profiles that are identical to the system.
    pub profile: Option<String>,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        if let Some(name) = self.profile {
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
                        println!("Removing identical user profile {:?}", profile.path());
                        fs::remove_file(profile.path())?;
                    }
                }
            }
            Ok(())
        }
    }
}

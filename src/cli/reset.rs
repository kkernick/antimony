//! Reset a user profile back to the system default.
use crate::aux::profile::Profile;
use anyhow::{Result, anyhow};
use dialoguer::Confirm;
use std::fs;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of the profile
    pub profile: String,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let name = &self.profile;
        let dest = Profile::user_profile(name);
        if dest.exists() {
            let system = Profile::system_profile(name);
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
    }
}

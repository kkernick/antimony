//! Edit an existing profile.
use crate::aux::profile::Profile;
use anyhow::Result;
use std::fs;

#[derive(clap::Args, Debug, Default)]
pub struct Args {
    /// The name of the profile
    pub profile: String,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        // Edit the profile
        let user = Profile::user_profile(&self.profile);
        let new = !user.exists();
        if new {
            let source = Profile::path(&self.profile)?;
            if let Some(parent) = user.parent() {
                user::set(user::Mode::Effective)?;
                fs::create_dir_all(parent)?;
                user::revert()?;
            }
            fs::copy(source, &user)?;
        }

        // Edit it.
        if Profile::edit(&user)?.is_none() && new {
            // If there was no modifications, delete the profile
            // since it's identical to the system one.
            user::set(user::Mode::Effective)?;
            fs::remove_file(user)?;
        }
        Ok(())
    }
}

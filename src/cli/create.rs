//! Create a new profile.
use crate::{
    cli::default,
    shared::{env::AT_HOME, profile::Profile},
};
use anyhow::Result;
use std::fs::{self, File};

#[derive(clap::Args, Debug, Default)]
pub struct Args {
    /// The name of the profile.
    pub profile: String,

    /// Provide an empty file, rather than a documented one.
    #[arg(short, long, default_value_t = false)]
    pub blank: bool,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        if self.profile == "default" {
            let args = default::Args::default();
            args.run()
        } else {
            user::set(user::Mode::Effective)?;

            let path = {
                let path = Profile::user_profile(&self.profile);
                if let Some(parent) = path.parent()
                    && !parent.exists()
                {
                    fs::create_dir_all(parent)?;
                }

                if !path.exists() && !self.blank {
                    fs::copy(AT_HOME.join("config").join("new.toml"), &path)?;
                }
                path
            };

            if !path.exists() {
                File::create(&path)?;
            }

            if Profile::edit(&path).is_err() {
                fs::remove_file(&path)?;
            }
            Ok(())
        }
    }
}

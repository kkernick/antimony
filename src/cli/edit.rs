//! Edit profiles/features, Create New Ones, and Modify the Default.

use std::fs;

use user::as_effective;

use crate::{
    cli,
    shared::{
        env::{AT_CONFIG, USER_NAME},
        feature::Feature,
        profile::Profile,
    },
};

#[derive(clap::Args, Default)]
pub struct Args {
    /// The object to edit.
    name: String,

    /// Target the feature set rather than the profile set.
    #[arg(long)]
    pub feature: bool,
}
impl cli::Run for Args {
    fn run(self) -> anyhow::Result<()> {
        let (table, kind) = if self.feature {
            ("features", "feature")
        } else {
            ("profiles", "profile")
        };

        let user = AT_CONFIG
            .join(USER_NAME.as_str())
            .join(table)
            .join(&self.name)
            .with_extension("toml");
        let system = AT_CONFIG
            .join(table)
            .join(&self.name)
            .with_extension("toml");

        let (src, path) = if user.exists() {
            (None, user)
        } else if system.exists() {
            fs::copy(&system, &user)?;
            (Some(system), user)
        } else {
            as_effective!(anyhow::Result<()>, {
                if let Some(parent) = user.parent() {
                    fs::create_dir_all(parent)?;
                }

                fs::copy(AT_CONFIG.join(kind).with_extension("toml"), &user)?;
                Ok(())
            })??;
            (Some(AT_CONFIG.join(kind).with_extension("toml")), user)
        };

        if self.feature {
            Feature::edit(&path)?
        } else {
            Profile::edit(&path)?
        };

        if let Some(src) = src
            && fs::read_to_string(src)? == fs::read_to_string(&path)?
        {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

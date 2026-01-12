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

        let path = if user.exists() {
            user
        } else if system.exists() {
            system
        } else {
            as_effective!(anyhow::Result<()>, {
                if let Some(parent) = user.parent() {
                    fs::create_dir_all(parent)?;
                }

                fs::copy(AT_CONFIG.join(kind).with_extension("toml"), &user)?;
                Ok(())
            })??;
            user
        };

        if self.feature {
            Feature::edit(&path)?
        } else {
            Profile::edit(&path)?
        };
        Ok(())
    }
}

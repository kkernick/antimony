//! Export user-profiles

use crate::shared::{
    env::PWD,
    store::{Object, SYSTEM_STORE, USER_STORE},
};
use anyhow::Result;
use clap::ValueHint;
use std::{fs, path::PathBuf};

#[derive(clap::Args)]
pub struct Args {
    /// The name of the profile/feature to export. If absent, export all user-profiles/features.
    #[arg(short, long, value_hint = ValueHint::CommandName)]
    name: Option<String>,

    /// Where to export to. Defaults to current directory
    #[arg(short, long, value_hint = ValueHint::DirPath)]
    dest: Option<String>,

    /// Target the feature set rather than the profile set.
    #[arg(short, long)]
    feature: bool,

    /// Target the system set rather than the user set.
    #[arg(short, long)]
    pub system: bool,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let dest = self.dest.map_or_else(|| PWD.clone(), PathBuf::from);
        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let export = |name: &str| -> Result<()> {
            let content = if !self.system
                && let Ok(user) = USER_STORE.borrow().fetch(name, table)
            {
                user
            } else if let Ok(system) = SYSTEM_STORE.borrow().fetch(name, table) {
                system
            } else {
                return Err(anyhow::anyhow!("No such {kind}: {name}"));
            };

            fs::write(dest.join(name).with_extension("toml"), content)?;
            Ok(())
        };

        if !dest.exists() {
            fs::create_dir_all(&dest)?;
        }

        if let Some(object) = self.name {
            export(&object)
        } else {
            for object in USER_STORE.borrow().get(table)? {
                export(&object)?;
            }
            Ok(())
        }
    }
}

//! Export user-profiles

use crate::shared::{
    env::PWD,
    store::{Object, SYSTEM_STORE, USER_STORE},
};
use anyhow::Result;
use std::{fs, path::PathBuf};
use user::as_real;

#[derive(clap::Args)]
pub struct Args {
    /// The name of the profile/feature to export. If absent, export all user-profiles/features.
    #[arg(long)]
    name: Option<String>,

    /// Where to export to. Defaults to current directory
    #[arg(long)]
    dest: Option<String>,

    /// Target the feature set rather than the profile set.
    #[arg(long, default_value_t = false)]
    feature: bool,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let dest = match self.dest {
            Some(path) => PathBuf::from(path),
            None => PWD.clone(),
        };

        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let export = |name: &str| -> Result<()> {
            let content = if let Ok(user) = USER_STORE.borrow().fetch(name, table) {
                user
            } else if let Ok(system) = SYSTEM_STORE.borrow().fetch(name, table) {
                system
            } else {
                return Err(anyhow::anyhow!("No such {kind}: {name}"));
            };

            as_real!(Result<()>, {
                fs::write(dest.join(name).with_extension("toml"), content)?;
                Ok(())
            })?
        };

        if !dest.exists() {
            as_real!(fs::create_dir_all(&dest))??;
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

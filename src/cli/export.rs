//! Export user-profiles

use crate::shared::{
    db::{self, Database, Table},
    env::PWD,
};
use anyhow::{Result, anyhow};
use std::{fs, path::PathBuf};
use user::as_real;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of the profile/feature to export. If absent, export all user-profiles/features.
    name: Option<String>,

    /// Where to export to. Defaults to current directory
    dest: Option<String>,

    /// Target the feature set instead of the profile set.
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
            (Table::Features, "feature")
        } else {
            (Table::Profiles, "profile")
        };

        let export = |name: &str| -> Result<()> {
            if let Some(content) = db::dump::<String>(name, Database::User, table)? {
                as_real!(Result<()>, {
                    fs::write(dest.join(name).with_extension("toml"), content)?;
                    Ok(())
                })??;
            } else {
                println!("No such {kind}: {name}");
            }
            Ok(())
        };

        if !dest.exists() {
            Err(anyhow!("Destination does not exist"))
        } else if let Some(object) = self.name {
            export(&object)
        } else {
            for object in db::all(Database::User, table)? {
                export(&object)?;
            }
            Ok(())
        }
    }
}

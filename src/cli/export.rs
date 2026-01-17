//! Export user-profiles

use crate::shared::env::{AT_CONFIG, PWD, USER_NAME};
use anyhow::{Result, anyhow};
use std::{fs, path::PathBuf};
use user::as_real;

#[derive(clap::Args)]
pub struct Args {
    /// The name of the profile/feature to export. If absent, export all user-profiles/features.
    name: Option<String>,

    /// Where to export to. Defaults to current directory
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
            ("features", "feature")
        } else {
            ("profiles", "profile")
        };

        let export = |name: &str| -> Result<()> {
            let path = AT_CONFIG
                .join(USER_NAME.as_str())
                .join(table)
                .join(name)
                .with_extension("toml");
            if let Ok(content) = fs::read_to_string(path) {
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
            for object in fs::read_dir(AT_CONFIG.join(USER_NAME.as_str()).join(table))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
            {
                export(&object.into_string().unwrap())?;
            }
            Ok(())
        }
    }
}

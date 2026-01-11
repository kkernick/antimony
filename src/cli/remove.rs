//! Reset a user profile back to the system default.

use crate::{
    cli,
    shared::{
        Set,
        env::{AT_CONFIG, USER_NAME},
        privileged,
    },
};
use anyhow::Result;
use dialoguer::Confirm;
use std::fs;
use user::as_effective;

#[derive(clap::Args)]
pub struct Args {
    /// The name of the profile. If absent, resets profiles that are identical to the system.
    pub name: Option<String>,

    /// Target a feature, rather than a profile. Requires privilege.
    #[arg(long)]
    pub feature: bool,
}
impl cli::Run for Args {
    fn run(self) -> Result<()> {
        let (table, kind) = if self.feature {
            ("features", "feature")
        } else {
            ("profiles", "profile")
        };

        if let Some(name) = self.name {
            let user = AT_CONFIG
                .join(USER_NAME.as_str())
                .join(table)
                .join(&name)
                .with_extension("toml");

            let system = AT_CONFIG.join(table).join(&name).with_extension("toml");

            if user.exists() && system.exists() {
                println!("Resetting to system {kind}");
                as_effective!(fs::remove_file(user))??;
            } else if user.exists() {
                if Confirm::new()
                    .with_prompt(format!(
                        "{name} is a user-created {kind}. There is no system default to reset to. Are you sure you want to remove it?",
                    ))
                    .interact()?
                {
                    println!("Goodbye, {name}!");
                    as_effective!(fs::remove_file(user))??;
                }
            } else if system.exists() && name != "default" {
                if privileged()?
                    && Confirm::new()
                        .with_prompt(format!(
                            "{name} is a system {kind}. Are you sure you want to remove it?",
                        ))
                        .interact()?
                {
                    println!("Deleting system {kind}");
                    as_effective!(fs::remove_file(system))??;
                }
            } else {
                println!("No such {kind}")
            }
            Ok(())
        } else {
            let user: Set<_> = fs::read_dir(AT_CONFIG.join(USER_NAME.as_str()).join(table))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect();
            let system: Set<_> = fs::read_dir(AT_CONFIG.join(table))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect();

            for thing in user.intersection(&system) {
                let user = AT_CONFIG.join(USER_NAME.as_str()).join(table).join(thing);
                let system = AT_CONFIG.join(table).join(thing);

                if fs::read_to_string(&user)? == fs::read_to_string(&system)? {
                    println!("Removing redundant {kind}: {}", thing.display());
                    as_effective!(fs::remove_file(user))??;
                }
            }
            Ok(())
        }
    }
}

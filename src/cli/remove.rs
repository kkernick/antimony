//! Reset a user profile back to the system default.

use crate::{
    cli,
    shared::{
        Set,
        feature::Feature,
        privileged,
        profile::Profile,
        store::{BackingStore, Object, SYSTEM_STORE, USER_STORE},
    },
};
use anyhow::Result;
use dialoguer::Confirm;
use log::debug;

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
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        if let Some(name) = self.name {
            let user = USER_STORE.with_borrow(|s| s.exists(&name, table));
            let system = SYSTEM_STORE.with_borrow(|s| s.exists(&name, table));

            if user && system {
                println!("Resetting to system {kind}");
                USER_STORE.with_borrow(|s| s.remove(&name, table))?;
            } else if user {
                if Confirm::new()
                    .with_prompt(format!(
                        "{name} is a user-created {kind}. There is no system default to reset to. Are you sure you want to remove it?",
                    ))
                    .interact()?
                {
                    println!("Goodbye, {name}!");
                    USER_STORE.with_borrow(|s| s.remove(&name, table))?;
                }
            } else if system && name != "default" {
                if privileged()?
                    && Confirm::new()
                        .with_prompt(format!(
                            "{name} is a system {kind}. Are you sure you want to remove it?",
                        ))
                        .interact()?
                {
                    println!("Deleting system {kind}");
                    SYSTEM_STORE.with_borrow(|s| s.remove(&name, table))?;
                }
            } else {
                println!("No such {kind}")
            }
            Ok(())
        } else {
            let user: Set<_> = USER_STORE
                .with_borrow(|s| s.get(table))?
                .into_iter()
                .collect();
            let system: Set<_> = SYSTEM_STORE
                .with_borrow(|s| s.get(table))?
                .into_iter()
                .collect();

            for thing in user.intersection(&system) {
                debug!("Custom User {kind} for: {thing}");
                let user = USER_STORE.with_borrow(|s| s.fetch(thing, table))?;
                let system = SYSTEM_STORE.with_borrow(|s| s.fetch(thing, table))?;

                let identical = if self.feature {
                    toml::from_str::<Feature>(&user)? == toml::from_str::<Feature>(&system)?
                } else {
                    toml::from_str::<Profile>(&user)? == toml::from_str::<Profile>(&system)?
                };

                if identical {
                    println!("Removing redundant {kind}: {thing}");
                    USER_STORE.with_borrow(|s| s.remove(thing, table))?;
                }
            }
            Ok(())
        }
    }
}

//! Reset a user profile back to the system default.

use crate::{
    cli,
    shared::{
        Set,
        feature::Feature,
        privileged,
        profile::Profile,
        store::{Object, SYSTEM_STORE, USER_STORE},
    },
};
use anyhow::Result;
use clap::ValueHint;
use dialoguer::Confirm;
use log::debug;

#[derive(clap::Args)]
pub struct Args {
    /// The name of the profile. If absent, resets profiles that are identical to the system.
    #[arg(value_hint = ValueHint::CommandName)]
    pub name: Option<String>,

    /// Target a feature, rather than a profile.
    #[arg(long, default_value_t = false)]
    pub feature: bool,

    /// Do not ask for confirmation.
    #[arg(long, default_value_t = false)]
    pub yes: bool,
}
impl cli::Run for Args {
    fn run(self) -> Result<()> {
        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        if let Some(name) = self.name {
            let user = USER_STORE.borrow().exists(&name, table);
            let system = SYSTEM_STORE.borrow().exists(&name, table);

            if user && system {
                println!("Resetting to system {kind}");
                USER_STORE.borrow().remove(&name, table)?;
            } else if user {
                if self.yes || Confirm::new()
                    .with_prompt(format!(
                        "{name} is a user-created {kind}. There is no system default to reset to. Are you sure you want to remove it?",
                    ))
                    .interact()?
                {
                    println!("Goodbye, {name}!");
                    USER_STORE.borrow().remove(&name, table)?;
                }
            } else if system && name != "default" {
                if privileged()?
                    && (self.yes
                        || Confirm::new()
                            .with_prompt(format!(
                                "{name} is a system {kind}. Are you sure you want to remove it?",
                            ))
                            .interact()?)
                {
                    println!("Deleting system {kind}");
                    SYSTEM_STORE.borrow().remove(&name, table)?;
                }
            } else {
                return Err(anyhow::anyhow!("No such {kind}"));
            }
        } else {
            let user: Set<_> = USER_STORE.borrow().get(table)?.into_iter().collect();
            let system: Set<_> = SYSTEM_STORE.borrow().get(table)?.into_iter().collect();

            for thing in user.intersection(&system) {
                debug!("Custom User {kind} for: {thing}");
                let user = USER_STORE.borrow().fetch(thing, table)?;
                let system = SYSTEM_STORE.borrow().fetch(thing, table)?;

                let identical = if self.feature {
                    toml::from_str::<Feature>(&user)? == toml::from_str::<Feature>(&system)?
                } else {
                    toml::from_str::<Profile>(&user)? == toml::from_str::<Profile>(&system)?
                };

                if identical {
                    println!("Removing redundant {kind}: {thing}");
                    USER_STORE.borrow().remove(thing, table)?;
                }
            }
        }
        Ok(())
    }
}

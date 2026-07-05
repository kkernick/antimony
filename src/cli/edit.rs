//! Edit profiles/features, Create New Ones, and Modify the Default.

use crate::{
    cli,
    shared::{
        env::AT_CONFIG,
        feature::Feature,
        privileged,
        profile::Profile,
        store::{Object, SYSTEM_STORE, USER_STORE},
    },
};
use anyhow::anyhow;
use clap::ValueHint;
use dialoguer::console::style;
use std::fs;

#[derive(clap::Args, Default)]
pub struct Args {
    /// The object to edit.
    #[arg(value_hint = ValueHint::CommandName)]
    name: String,

    /// Target the feature set rather than the profile set.
    #[arg(short, long)]
    pub feature: bool,

    /// Target the system set rather than the user set.
    #[arg(short, long)]
    pub system: bool,
}
impl cli::Run for Args {
    fn run(self) -> anyhow::Result<()> {
        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let user = USER_STORE.borrow().fetch(&self.name, table);
        let system = SYSTEM_STORE.borrow().fetch(&self.name, table);

        let (buffer, new) = if self.system {
            if privileged()? {
                if let Ok(str) = system {
                    (str, false)
                } else {
                    let str = fs::read_to_string(AT_CONFIG.join(kind).with_extension("toml"))?;
                    SYSTEM_STORE.borrow().store(&self.name, table, &str)?;
                    (str, true)
                }
            } else {
                return Err(anyhow!("Not allowed to modify system store"));
            }
        } else if let Ok(str) = user {
            (str, false)
        } else if let Ok(str) = system {
            USER_STORE.borrow().store(&self.name, table, &str)?;
            (str, true)
        } else {
            (
                {
                    let str = fs::read_to_string(AT_CONFIG.join(kind).with_extension("toml"))?;
                    USER_STORE.borrow().store(&self.name, table, &str)?;
                    str
                },
                true,
            )
        };

        let commit = if self.feature {
            Feature::edit(&buffer)?
        } else {
            Profile::edit(&buffer)?
        };

        if let Some(out) = commit {
            USER_STORE.borrow().store(&self.name, table, &out)?;
            if self.name == "default" || self.feature {
                eprintln!(
                    "{}",
                    style("Note: Profiles will not use your changes until you refresh them.")
                        .yellow()
                );
            }
        } else if new {
            USER_STORE.borrow().remove(&self.name, table)?;
        }

        Ok(())
    }
}

//! Edit profiles/features, Create New Ones, and Modify the Default.

use std::fs;

use user::as_effective;

use crate::{
    cli,
    shared::{
        env::AT_CONFIG,
        feature::Feature,
        profile::Profile,
        store::{Object, SYSTEM_STORE, USER_STORE},
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
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let user = USER_STORE.with_borrow(|s| s.fetch(&self.name, table));
        let system = SYSTEM_STORE.with_borrow(|s| s.fetch(&self.name, table));

        let (buffer, new) = if let Ok(str) = user {
            (str, false)
        } else if let Ok(str) = system {
            USER_STORE.with_borrow(|s| s.store(&self.name, table, &str))?;
            (str, true)
        } else {
            (
                as_effective!(anyhow::Result<String>, {
                    let str = fs::read_to_string(AT_CONFIG.join(kind).with_extension("toml"))?;
                    USER_STORE.with_borrow(|s| s.store(&self.name, table, &str))?;
                    Ok(str)
                })??,
                true,
            )
        };

        let commit = if self.feature {
            Feature::edit(&buffer)?
        } else {
            Profile::edit(&buffer)?
        };

        if let Some(out) = commit {
            USER_STORE.with_borrow(|s| s.store(&self.name, table, &out))?;
        } else if new {
            USER_STORE.with_borrow(|s| s.remove(&self.name, table))?;
        }

        Ok(())
    }
}

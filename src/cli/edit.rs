//! Edit profiles/features, Create New Ones, and Modify the Default.

use std::{
    fs,
    io::{Read, stdin},
};

use dialoguer::console::style;

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

    /// Read from stdin, rather than using an interactive editor.
    #[arg(long)]
    pub stdin: bool,
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

        let (buffer, new) = if let Ok(str) = user {
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

        let commit = if self.stdin {
            let mut buffer = String::new();
            stdin().read_to_string(&mut buffer)?;
            if (self.feature && toml::from_str::<Feature>(&buffer).is_ok())
                || (!self.feature && toml::from_str::<Profile>(&buffer).is_ok())
            {
                Some(buffer)
            } else {
                if new {
                    USER_STORE.borrow().remove(&self.name, table)?;
                }
                return Err(anyhow::anyhow!("Invalid {kind} on stdin."));
            }
        } else if self.feature {
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

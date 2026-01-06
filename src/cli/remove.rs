//! Reset a user profile back to the system default.

use crate::{
    cli,
    shared::{
        db::{self, Database, Table},
        privileged,
    },
};
use anyhow::Result;
use dialoguer::Confirm;

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
            (Table::Features, "feature")
        } else {
            (Table::Profiles, "profile")
        };

        if let Some(name) = self.name {
            let user = db::exists(&name, Database::User, table)?;
            let system = db::exists(&name, Database::System, table)?;

            if user && system {
                println!("Resetting to system {kind}");
                db::delete(&name, Database::User, table)?;
            } else if user {
                if Confirm::new()
                    .with_prompt(format!(
                        "{name} is a user-created {kind}. There is no system default to reset to. Are you sure you want to remove it?",
                    ))
                    .interact()?
                {
                    println!("Goodbye, {name}!");
                    db::delete(&name, Database::User, table)?;
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
                    db::delete(&name, Database::System, table)?;
                }
            } else {
                println!("No such {kind}")
            }
            Ok(())
        } else {
            let user = db::all(Database::User, table)?;
            let system = db::all(Database::System, table)?;

            for thing in user.intersection(&system) {
                if db::dump::<String>(thing, Database::User, table)?
                    == db::dump::<String>(thing, Database::System, table)?
                {
                    println!("Removing redundant {kind}: {thing}");
                    db::delete(thing, Database::User, table)?;
                }
            }
            Ok(())
        }
    }
}

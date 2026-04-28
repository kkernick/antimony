//! Edit profiles/features, Create New Ones, and Modify the Default.

use dialoguer::console::style;
use similar::{Algorithm, TextDiff};

use crate::{
    cli,
    shared::store::{Object, SYSTEM_STORE, USER_STORE},
};

#[derive(clap::Args, Default)]
pub struct Args {
    /// The object to edit.
    name: Option<String>,

    /// Target the feature set rather than the profile set.
    #[arg(long)]
    pub feature: bool,

    /// Display the difference between system and user profiles.
    #[arg(long)]
    pub diff: bool,
}

impl cli::Run for Args {
    fn run(self) -> anyhow::Result<()> {
        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let info_dump = |name: &str| -> String {
            let user = USER_STORE.borrow().fetch(name, table);
            let system = SYSTEM_STORE.borrow().fetch(name, table);

            if let Ok(user) = user {
                if self.diff
                    && name != "default"
                    && let Ok(system) = system
                {
                    TextDiff::configure()
                        .algorithm(Algorithm::Patience)
                        .diff_lines(&system, &user)
                        .unified_diff()
                        .to_string()
                } else if system.is_err() {
                    format!("USER\n{user}")
                } else {
                    user
                }
            } else if let Ok(system) = system {
                system
            } else {
                format!("No such {kind}: name")
            }
        };

        let print = |name: &str, out: String| {
            if out.starts_with("USER") {
                println!(
                    "\n{}",
                    style(format!("=== {name} (User) ===")).bold().italic()
                );
            } else {
                println!("\n{}", style(format!("=== {name} ===")).bold());
            };
            out.lines().for_each(|line| {
                if line.starts_with("+") {
                    println!("{}", style(line).green())
                } else if line.starts_with("-") {
                    println!("{}", style(line).red())
                } else if !line.starts_with("@") && line != "USER" {
                    println!("{line}")
                }
            })
        };

        if let Some(name) = self.name {
            print(&name, info_dump(&name))
        } else if let Ok(user) = USER_STORE.borrow().get(table) {
            SYSTEM_STORE
                .borrow()
                .get(table)?
                .union(&user)
                .for_each(|name| print(name, info_dump(name)));
        } else {
            SYSTEM_STORE
                .borrow()
                .get(table)?
                .iter()
                .for_each(|name| print(name, info_dump(name)));
        }

        Ok(())
    }
}

//! Edit profiles/features, Create New Ones, and Modify the Default.

use crate::{
    cli,
    shared::{
        Set,
        store::{Object, SYSTEM_STORE, USER_STORE},
        syscalls,
    },
};
use anyhow::Result;
use clap::ValueHint;
use dialoguer::console::style;
use seccomp::syscall::Syscall;
use similar::{Algorithm, TextDiff};

#[derive(clap::Args, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Args {
    /// The object to display information on. Default to everything.
    #[arg(value_hint = ValueHint::CommandName)]
    name: Option<String>,

    /// Target the feature set rather than the profile set.
    #[arg(short, long)]
    pub feature: bool,

    /// Display the difference between system and user profiles.
    #[arg(long, long)]
    pub diff: bool,

    /// Target the system set rather than the user set.
    #[arg(short, long)]
    pub system: bool,

    /// Display SECCOMP data. Overrides --feature, --system, and --diff.

    #[arg(long)]
    pub seccomp: bool,
}

impl cli::Run for Args {
    #[allow(clippy::too_many_lines)]
    fn run(self) -> Result<()> {
        if self.seccomp {
            match self.name {
                // Get Profile/Binary information depending on a path.
                Some(name) => {
                    let calls: Set<i32> = if name.contains('/') {
                        syscalls::CONNECTION.with_borrow_mut(|conn| -> Result<_> {
                            let tx = conn.transaction()?;
                            let calls = syscalls::get_binary_syscalls(&tx, &name)?;
                            tx.commit()?;
                            Ok(calls)
                        })?
                    } else {
                        let (syscalls, _) = syscalls::get_calls(&name, &Set::default());
                        syscalls.into_iter().collect()
                    };

                    let mut syscalls = calls
                        .into_iter()
                        .filter_map(|e| Syscall::get_name(e).ok())
                        .collect::<Vec<_>>();

                    syscalls.sort();
                    println!(
                        "{} => {}",
                        style(name).italic().magenta(),
                        style(syscalls.join(" ")).bold()
                    );
                }

                // Get information on everything in the database.
                None => {
                    syscalls::CONNECTION.with_borrow_mut(|conn| -> Result<()> {
                        let tx = conn.transaction()?;

                        // Profile info.
                        println!("{}", style("\n=== Profiles ===").bold());
                        let mut stmt = tx.prepare("SELECT name FROM profiles")?;
                        for profile in stmt.query_map([], |row| row.get::<_, String>(0))?.flatten()
                        {
                            let mut stmt = tx.prepare(
                                "SELECT b.path
                              FROM profiles p
                              JOIN profile_binaries pb ON pb.profile_id = p.id
                              JOIN binaries b ON b.id = pb.binary_id
                              WHERE p.name = ?1",
                            )?;

                            let binaries: Vec<String> = stmt
                                .query_map([&profile], |row| row.get::<_, String>(0))?
                                .flatten()
                                .collect();

                            println!("{}:", style(profile).italic().magenta());
                            for binary in binaries {
                                println!("\t - {}", style(binary).italic());
                            }
                        }
                        println!("{}", style("================").bold());

                        // Binary information.
                        println!("{}", style("\n=== Binaries ===").bold());
                        let mut stmt = tx.prepare("SELECT path FROM binaries")?;
                        for path in stmt.query_map([], |row| row.get::<_, String>(0))?.flatten() {
                            if let Ok(syscalls) = syscalls::get_binary_syscalls(&tx, &path) {
                                let mut syscalls: Vec<_> =
                                    syscalls::get_names(syscalls).into_iter().collect();
                                syscalls.sort();
                                println!(
                                    "{} => {}",
                                    style(path).italic().magenta(),
                                    style(syscalls.join(" ")).bold()
                                );
                                println!();
                            }
                        }
                        println!("{}", style("================").bold());
                        Ok(())
                    })?;
                }
            }
            return Ok(());
        }

        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let info_dump = |name: &str| -> String {
            let user = USER_STORE.borrow().fetch(name, table);
            let system = SYSTEM_STORE.borrow().fetch(name, table);

            if let Ok(user) = user
                && !self.system
            {
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
            }

            out.lines().for_each(|line| {
                if line.starts_with('+') {
                    println!("{}", style(line).green());
                } else if line.starts_with('-') {
                    println!("{}", style(line).red());
                } else if !line.starts_with('@') && line != "USER" {
                    println!("{line}");
                }
            });
        };

        if let Some(name) = self.name {
            print(&name, info_dump(&name));
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

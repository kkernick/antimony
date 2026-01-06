//! Get info about the installed configuration.

use crate::shared::{
    Set,
    db::{self, Database, Table},
    feature::Feature,
    profile::Profile,
    syscalls,
};
use anyhow::Result;
use clap::ValueEnum;
use seccomp::syscall::Syscall;

pub trait Info {
    fn info(&self, name: &str, verbosity: u8);
}

/// What to get information on.
#[derive(ValueEnum, Clone, Debug)]
pub enum Target {
    /// A Profile
    Profile,

    /// A Feature
    Feature,

    /// Query the SECCOMP database.
    Seccomp,
}

#[derive(clap::Args, Debug)]
pub struct Args {
    /// What to get info on.
    pub target: Target,

    /// Profile/Feature/Binary name.
    pub name: Option<String>,

    /// The verbosity of information.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbosity: u8,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        match self.target {
            Target::Profile => match self.name {
                Some(profile) => Profile::new(&profile, None)?.info(&profile, self.verbosity + 1),
                None => {
                    for profile in db::all(Database::System, Table::Profiles)? {
                        Profile::new(&profile, None)?.info(&profile, self.verbosity + 1);
                    }
                }
            },

            // Feature information.
            Target::Feature => match self.name {
                Some(feature) => Feature::new(&feature)?.info(&feature, self.verbosity + 1),
                None => {
                    for feature in db::all(Database::System, Table::Features)? {
                        Feature::new(&feature)?.info(&feature, self.verbosity + 1);
                    }
                }
            },

            // SECCOMP Info.
            Target::Seccomp => match self.name {
                // Get Profile/Binary information depending on a path.
                Some(name) => {
                    print!("{name}: ");
                    let calls: Set<i32> = if name.contains('/') {
                        syscalls::CONNECTION.with_borrow_mut(|conn| -> Result<Set<i32>> {
                            let tx = conn.transaction()?;
                            let calls = syscalls::get_binary_syscalls(&tx, &name)?;
                            tx.commit()?;
                            Ok(calls)
                        })?
                    } else {
                        let (syscalls, _) = syscalls::get_calls(&name, &Set::default());
                        syscalls.into_iter().collect()
                    };

                    if self.verbosity > 0 {
                        let mut syscalls = calls
                            .into_iter()
                            .filter_map(|e| Syscall::get_name(e).ok())
                            .collect::<Vec<_>>();

                        syscalls.sort();
                        for call in syscalls {
                            print!("{call} ");
                        }
                    } else {
                        print!("{} syscalls", calls.len());
                    }
                    println!();
                }

                // Get information on everything in the database.
                None => {
                    syscalls::CONNECTION.with_borrow_mut(|conn| -> Result<()> {
                        let tx = conn.transaction()?;

                        // Profile info.
                        println!("\n=== Profiles ===");
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

                            println!("{profile} => {}", binaries.join(" "))
                        }
                        println!("================");

                        // Binary information.
                        println!("\n=== Binaries ===");
                        let mut stmt = tx.prepare("SELECT path FROM binaries")?;
                        for path in stmt.query_map([], |row| row.get::<_, String>(0))?.flatten() {
                            if let Ok(syscalls) = syscalls::get_binary_syscalls(&tx, &path) {
                                let syscalls = syscalls::get_names(syscalls);
                                if self.verbosity > 0 {
                                    println!("{path} => {}", syscalls.join(" "))
                                } else {
                                    println!("{path} => {} syscalls", syscalls.len())
                                }
                            }
                        }
                        println!("================");
                        Ok(())
                    })?;
                }
            },
        };
        Ok(())
    }
}

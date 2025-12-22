//! Get info about the installed configuration.
use crate::shared::{
    env::AT_HOME,
    feature::Feature,
    profile::{self, Profile},
    syscalls,
};
use anyhow::Result;
use clap::ValueEnum;
use console::style;
use log::error;
use seccomp::syscall::Syscall;
use std::{collections::HashSet, fs, path::Path};

/// What to get information on.
#[derive(ValueEnum, Clone, Debug)]
pub enum What {
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
    pub what: What,

    /// Profile/Feature/Binary name.
    pub name: Option<String>,

    /// The verbosity of information.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbosity: u8,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        match self.what {
            What::Profile => {
                // Print information on a profile.
                let print = |path: &str, verbosity: u8| -> Result<()> {
                    let name = if let Some(i) = path.rfind('/') {
                        &path[i + 1..]
                    } else {
                        path
                    };

                    match Profile::new(path, None) {
                        Ok(profile) => {
                            profile.info(name, verbosity);
                        }
                        Err(profile::Error::Path(_)) => {
                            let name = if let Some(i) = path.rfind('/') {
                                &path[i + 1..]
                            } else {
                                path
                            };

                            println!(
                                "{} => {}",
                                style(name).bold(),
                                style("Application not installed").red()
                            );
                        }
                        Err(e) => error!("{e}"),
                    }
                    Ok(())
                };

                // Either get information on a single profile, or all of them.
                match self.name {
                    Some(profile) => print(&profile, self.verbosity + 1)?,
                    None => {
                        let profiles = Path::new(AT_HOME.as_path()).join("profiles");
                        for path in fs::read_dir(profiles)?.filter_map(|e| e.ok()) {
                            let path = path.path().to_string_lossy().into_owned();
                            print(&path, self.verbosity)?;
                        }
                    }
                }
            }

            // Feature information.
            What::Feature => match self.name {
                Some(profile) => Feature::new(&profile)?.info(self.verbosity + 1),
                None => {
                    let features = Path::new(AT_HOME.as_path()).join("features");
                    for path in fs::read_dir(features)?.filter_map(|e| e.ok()) {
                        let feature: Feature = toml::from_str(&fs::read_to_string(path.path())?)?;
                        feature.info(self.verbosity);
                    }
                }
            },

            // SECCOMP Info.
            What::Seccomp => match self.name {
                // Get Profile/Binary information depending on a path.
                Some(name) => {
                    print!("{name}: ");
                    let calls: HashSet<i32> = if name.contains('/') {
                        let mut conn = syscalls::DB_POOL.get()?;
                        let tx = conn.transaction()?;
                        let calls = syscalls::get_binary_syscalls(&tx, &name)?;
                        tx.commit()?;
                        calls
                    } else {
                        let (syscalls, _) = syscalls::get_calls(&name, &None, false)?;
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
                    let mut conn = syscalls::DB_POOL.get()?;
                    let tx = conn.transaction()?;

                    // Profile info.
                    println!("\n=== Profiles ===");
                    let mut stmt = tx.prepare("SELECT name FROM profiles")?;
                    for profile in stmt.query_map([], |row| row.get::<_, String>(0))?.flatten() {
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
                }
            },
        }
        Ok(())
    }
}

//! Modify the SECCOMP Database.
use crate::shared::{
    env::{AT_HOME, DATA_HOME},
    syscalls::{self, DB_POOL},
};
use anyhow::{Result, anyhow};
use clap::ValueEnum;
use dialoguer::Confirm;
use nix::unistd::{getcwd, getpid};
use rusqlite::params;
use spawn::Spawner;
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use user::try_run_as;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The operation to perform.
    pub operation: Operation,

    /// An optional path, used by Export/Merge.
    pub path: Option<String>,
}

/// The Operation to perform.
#[derive(ValueEnum, Copy, Clone, Debug)]
pub enum Operation {
    /// Optimize the database.
    Optimize,

    /// Remove the database completely.
    Remove,

    /// Export the database to a path.
    Export,

    /// Merge another database into the system database.
    Merge,

    /// Remove binaries that no longer exist from the database.
    Clean,
}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        let result = try_run_as!(user::Mode::Real, Result<i32>, {
            Ok(Spawner::new("pkcheck")
                .args([
                    "--action-id",
                    "org.freedesktop.policykit.exec",
                    "--allow-user-interaction",
                    "--process",
                    &format!("{}", getpid().as_raw()),
                ])?
                .mode(user::Mode::Real)
                .preserve_env(true)
                .spawn()?
                .wait()?)
        })?;

        if result != 0 {
            Err(anyhow!(
                "Administrative privilege and Polkit is required to modify the SECCOMP database!"
            ))
        } else {
            match self.operation {
                Operation::Optimize => {
                    let conn = DB_POOL.get()?;
                    conn.execute("VACUUM;", [])?;
                    conn.execute("ANALYZE;", [])?;
                    println!("Optimized!");
                    Ok(())
                }
                Operation::Remove => {
                    let confirm = Confirm::new()
                        .with_prompt("Are you sure you want to delete the SECCOMP Database?")
                        .interact()?;

                    if confirm {
                        fs::remove_dir_all(AT_HOME.join("seccomp"))?;
                        println!("Deleted");
                    }
                    Ok(())
                }
                Operation::Export => {
                    try_run_as!(user::Mode::Real, Result<()>, {
                        let db = AT_HOME.join("seccomp").join("syscalls.db");
                        if !db.exists() {
                            return Err(anyhow!("No database exists!"));
                        } else {
                            let dest = match self.path {
                                Some(path) => PathBuf::from(path),
                                None => getcwd()?.join("syscalls.db"),
                            };

                            io::copy(&mut File::open(db)?, &mut File::create(&dest)?)?;
                            println!("Exported to {dest:?}");
                        }
                        Ok(())
                    })
                }

                Operation::Merge => {
                    let db = match self.path {
                        Some(path) => PathBuf::from(path),
                        None => getcwd()?.join("syscalls.db"),
                    };

                    let temp = NamedTempFile::new_in(AT_HOME.join("seccomp"))?;
                    fs::copy(&db, temp.path())?;

                    let mut conn = DB_POOL.get()?;
                    let tx = conn.transaction()?;
                    tx.execute(
                        &format!("ATTACH DATABASE '{}' AS other", temp.path().display()),
                        [],
                    )?;
                    tx.execute_batch(
                        "
                     INSERT OR IGNORE INTO binaries (path)
                     SELECT path FROM other.binaries;

                     INSERT OR IGNORE INTO syscalls (name)
                     SELECT name FROM other.syscalls;

                     INSERT OR IGNORE INTO profiles (name)
                     SELECT name FROM other.profiles;

                     INSERT OR IGNORE INTO binary_syscalls (binary_id, syscall_id)
                     SELECT b1.id, s1.id
                     FROM other.binary_syscalls bs
                     JOIN other.binaries b2 ON bs.binary_id = b2.id
                     JOIN binaries b1 ON b1.path = b2.path
                     JOIN other.syscalls s2 ON bs.syscall_id = s2.id
                     JOIN syscalls s1 ON s1.name = s2.name;

                     INSERT OR IGNORE INTO profile_binaries (profile_id, binary_id)
                     SELECT p1.id, b1.id
                     FROM other.profile_binaries pb
                     JOIN other.profiles p2 ON pb.profile_id = p2.id
                     JOIN profiles p1 ON p1.name = p2.name
                     JOIN other.binaries b2 ON pb.binary_id = b2.id
                     JOIN binaries b1 ON b1.path = b2.path;
                     ",
                    )?;
                    tx.commit()?;
                    Ok(())
                }

                Operation::Clean => {
                    let mut conn = syscalls::DB_POOL.get()?;
                    let tx = conn.transaction()?;

                    || -> Result<()> {
                        let mut stmt = tx.prepare("SELECT id, name FROM profiles")?;
                        let profiles = stmt.query_map([], |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                        })?;

                        for profile in profiles {
                            let (id, name) = profile?;
                            if name == "xdg-dbus-proxy" {
                                continue;
                            }

                            if !AT_HOME
                                .join("profiles")
                                .join(format!("{name}.toml"))
                                .exists()
                            {
                                println!("Removing missing profile: {name}");
                                tx.execute("DELETE FROM profiles WHERE id = ?", params![id])?;
                            }
                        }
                        Ok(())
                    }()?;

                    || -> Result<()> {
                        tx.execute("DELETE FROM profile_binaries WHERE profile_id NOT IN (SELECT id FROM profiles);", [])?;
                        Ok(())
                    }()?;

                    // Remove missing binaries
                    || -> Result<()> {
                        let mut stmt = tx.prepare("SELECT id, path FROM binaries")?;
                        let binaries = stmt.query_map([], |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                        })?;

                        for binary in binaries {
                            let (id, path) = binary?;
                            let remove = if path.starts_with("/home/antimony") {
                                // If any profile's home has the binary in it, we preserve.
                                let wild = path.replace("/home/antimony", "*");
                                Spawner::new("/usr/bin/find")
                                    .arg(DATA_HOME.join("antimony").to_string_lossy())?
                                    .args(["-wholename", &wild])?
                                    .mode(user::Mode::Real)
                                    .output(true)
                                    .spawn()?
                                    .output_all()?
                                    .is_empty()
                            } else if path.ends_with("flatpak-spawn") {
                                false
                            } else {
                                !Path::new(&path).exists()
                            };

                            if remove {
                                println!("Removing missing binary: {path}");
                                tx.execute("DELETE FROM binaries WHERE id = ?", params![id])?;
                            }
                        }
                        Ok(())
                    }()?;

                    // Remove Orphans
                    || -> Result<()> {
                        tx.execute("DELETE FROM binaries WHERE id NOT IN (SELECT DISTINCT binary_id FROM profile_binaries);", [])?;
                        Ok(())
                    }()?;

                    tx.commit()?;
                    Ok(())
                }
            }
        }
    }
}

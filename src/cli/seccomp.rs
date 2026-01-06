//! Modify the SECCOMP Database.

use crate::shared::{
    env::{AT_HOME, DATA_HOME},
    privileged, syscalls,
};
use anyhow::{Result, anyhow};
use clap::ValueEnum;
use nix::unistd::getcwd;
use rusqlite::params;
use spawn::{Spawner, StreamMode};
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};
use user::as_real;

#[derive(clap::Args)]
pub struct Args {
    /// The operation to perform.
    pub operation: Operation,

    /// An optional path, used by Export/Merge.
    pub path: Option<String>,
}

/// The Operation to perform.
#[derive(ValueEnum, Copy, Clone)]
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
        if !privileged()? {
            Err(anyhow!(
                "Modifying the SECCOMP database is a privileged operation"
            ))
        } else {
            user::set(user::Mode::Effective)?;

            match self.operation {
                Operation::Optimize => syscalls::CONNECTION.with_borrow_mut(|conn| {
                    conn.execute("VACUUM;", [])?;
                    conn.execute("ANALYZE;", [])?;
                    println!("Optimized!");
                    Ok(())
                }),
                Operation::Remove => {
                    fs::remove_dir_all(AT_HOME.join("seccomp"))?;
                    println!("Deleted");

                    Ok(())
                }
                Operation::Export => {
                    let db = AT_HOME.join("seccomp").join("syscalls.db");
                    if !db.exists() {
                        return Err(anyhow!("No database exists!"));
                    } else {
                        let dest = match self.path {
                            Some(path) => PathBuf::from(path),
                            None => getcwd()?.join("syscalls.db"),
                        };

                        as_real!({ io::copy(&mut File::open(db)?, &mut File::create(&dest)?) })??;
                        println!("Exported to {}", dest.display());
                    }
                    Ok(())
                }

                Operation::Merge => {
                    let db = match self.path {
                        Some(path) => PathBuf::from(path),
                        None => getcwd()?.join("syscalls.db"),
                    };

                    let temp = temp::Builder::new()
                        .within(AT_HOME.join("seccomp"))
                        .create::<temp::File>()?;
                    fs::copy(&db, temp.path())?;

                    syscalls::CONNECTION.with_borrow_mut(|conn| {
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
                    })
                }

                Operation::Clean => {
                    syscalls::CONNECTION.with_borrow_mut(|conn| {
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
                                Spawner::abs("/usr/bin/find")
                                    .arg(DATA_HOME.join("antimony").to_string_lossy())?
                                    .args(["-wholename", &wild])?
                                    .mode(user::Mode::Real)
                                    .output(StreamMode::Pipe)
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
                    })
                }
            }
        }
    }
}

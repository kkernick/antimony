//! Modify the SECCOMP Database.
use crate::aux::{
    env::{AT_HOME, DATA_HOME},
    syscalls::{self, DB_POOL},
};
use anyhow::{Result, anyhow};
use clap::ValueEnum;
use dialoguer::Confirm;
use nix::unistd::getcwd;
use rusqlite::params;
use spawn::Spawner;
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

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
                    std::fs::remove_dir_all(AT_HOME.join("seccomp"))?;
                    println!("Deleted");
                }
                Ok(())
            }
            Operation::Export => {
                user::set(user::Mode::Real)?;
                let db = AT_HOME.join("seccomp").join("syscalls.db");
                if !db.exists() {
                    return Err(anyhow!("No database exists!"));
                } else {
                    let dest = match self.path {
                        Some(path) => PathBuf::from(path),
                        None => getcwd()?.join("syscalls.db"),
                    };

                    std::io::copy(&mut File::open(db)?, &mut File::create(&dest)?)?;
                    println!("Exported to {dest:?}")
                }
                Ok(())
            }

            Operation::Merge => {
                user::set(user::Mode::Effective)?;
                let db = match self.path {
                    Some(path) => PathBuf::from(path),
                    None => getcwd()?.join("syscalls.db"),
                };

                let temp = NamedTempFile::new_in(AT_HOME.join("seccomp"))?;
                std::fs::copy(&db, temp.path())?;

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

                tx.commit()?;
                Ok(())
            }
        }
    }
}

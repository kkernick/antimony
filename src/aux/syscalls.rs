use crate::aux::env::AT_HOME;
use crate::aux::path::which_exclude;
use crate::aux::{path::user_dir, profile::SeccompPolicy};
use log::{info, trace, warn};
use nix::sys::socket::{self, ControlMessage, MsgFlags};
use once_cell::sync::Lazy;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Transaction;
use seccomp::{self, action::Action, attribute::Attribute, filter::Filter, syscall::Syscall};
use std::borrow::Cow;
use std::collections::{BTreeSet, HashSet};
use std::io::IoSlice;
use std::os::fd::{AsRawFd, IntoRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

/// Connection to the Database
pub static DB_POOL: Lazy<Pool<SqliteConnectionManager>> = Lazy::new(|| {
    let saved = user::save().expect("Failed to save user");
    user::set(user::Mode::Effective).expect("Failed to set user");
    let dir = AT_HOME.join("seccomp");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).expect("Failed to create SECCOMP directory");
    }
    let manager = SqliteConnectionManager::file(dir.join("syscalls.db"));
    let pool = Pool::new(manager).expect("Failed to create pool");

    let conn = pool.get().expect("Failed to get connection");
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("Failed to set mode");

    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        CREATE TABLE IF NOT EXISTS binaries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS syscalls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name INTEGER NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS binary_syscalls (
            binary_id INTEGER NOT NULL,
            syscall_id INTEGER NOT NULL,
            PRIMARY KEY (binary_id, syscall_id),
            FOREIGN KEY (binary_id) REFERENCES binaries(id) ON DELETE CASCADE,
            FOREIGN KEY (syscall_id) REFERENCES syscalls(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL
            );

        CREATE TABLE IF NOT EXISTS profile_binaries (
            profile_id INTEGER NOT NULL,
            binary_id INTEGER NOT NULL,
            PRIMARY KEY (profile_id, binary_id),
            FOREIGN KEY (profile_id) REFERENCES profiles(id) ON DELETE CASCADE,
            FOREIGN KEY (binary_id) REFERENCES binaries(id) ON DELETE CASCADE
        );
        ",
    )
    .expect("Failed to initialize schema");
    user::restore(saved).expect("Failed to restore user");
    pool
});

/// Errors relating to SECCOMP policy generation.
#[derive(Debug)]
pub enum Error {
    /// Errors from the `seccomp` crate.
    Seccomp(seccomp::Error),

    /// Errors interfacing with the database
    Database(rusqlite::Error),

    /// Errors connecting to the database.
    Connection(r2d2::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Seccomp(e) => write!(f, "SECCOMP Error: {e}"),
            Self::Database(e) => write!(f, "Database Error: {e}"),
            Self::Connection(e) => write!(f, "Connection Error: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Seccomp(e) => Some(e),
            Self::Database(e) => Some(e),
            Self::Connection(e) => Some(e),
        }
    }
}
impl From<seccomp::Error> for Error {
    fn from(value: seccomp::Error) -> Self {
        Error::Seccomp(value)
    }
}
impl From<seccomp::filter::Error> for Error {
    fn from(value: seccomp::filter::Error) -> Self {
        Error::Seccomp(value.into())
    }
}
impl From<seccomp::syscall::Error> for Error {
    fn from(value: seccomp::syscall::Error) -> Self {
        Error::Seccomp(value.into())
    }
}
impl From<seccomp::notify::Error> for Error {
    fn from(value: seccomp::notify::Error) -> Self {
        Error::Seccomp(value.into())
    }
}
impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Error::Database(value)
    }
}
impl From<r2d2::Error> for Error {
    fn from(value: r2d2::Error) -> Self {
        Error::Connection(value)
    }
}

/// The Antimony Monitor Notifier Implementation.
struct Notifier {
    /// The path to the socket.
    path: PathBuf,

    /// The name of the profile to pass to the monitor
    name: String,

    /// The stream to the monitor, established in `prepare()`
    stream: Option<UnixStream>,
}
impl Notifier {
    /// Construct a new Notifier from the path the monitor should listen,
    /// and the name of the process.
    pub fn new(path: PathBuf, name: String) -> Self {
        Self {
            path,
            name,
            stream: None,
        }
    }
}
impl seccomp::filter::Notifier for Notifier {
    /// The Notifier needs sendmsg.
    fn exempt(&self) -> Vec<(Action, Syscall)> {
        vec![(
            Action::Log,
            Syscall::from_name("sendmsg").expect("Failed to get syscall"),
        )]
    }

    /// Setup the UnixStream. We wait for the Monitor to setup the socket.
    fn prepare(&mut self) {
        while !self.path.exists() {
            sleep(Duration::from_millis(100));
        }
        self.stream = Some(UnixStream::connect(&self.path).expect("Failed to connect"));
    }

    /// Send the FD to the Monitor.
    fn handle(&mut self, fd: OwnedFd) {
        let stream = self.stream.take().unwrap();
        let raw_fd = stream.as_raw_fd();
        let name_bytes = self.name.as_bytes();
        let io = [IoSlice::new(name_bytes)];
        let fds = [fd.into_raw_fd()];
        let msgs = [ControlMessage::ScmRights(&fds)];

        socket::sendmsg::<()>(raw_fd, &io, &msgs, MsgFlags::empty(), None)
            .expect("Failed to send the FD");
    }
}

/// Get the internal ID of a profile
pub fn profile_id(tx: &Transaction, name: &str) -> Result<i64, Error> {
    let id: i64 = tx.query_row("SELECT id FROM profiles WHERE name = ?1", [name], |row| {
        row.get(0)
    })?;
    Ok(id)
}

/// Add a profile to the database.
pub fn insert_profile(tx: &Transaction, name: &str) -> Result<i64, Error> {
    if let Ok(id) = profile_id(tx, name) {
        Ok(id)
    } else {
        tx.execute("INSERT OR IGNORE INTO profiles (name) VALUES (?1)", [name])?;
        profile_id(tx, name)
    }
}

/// Get the internal ID of a binary.
pub fn binary_id(tx: &Transaction, path: &str) -> Result<i64, Error> {
    let id = tx.query_row("SELECT id FROM binaries WHERE path = ?1", [path], |row| {
        row.get(0)
    })?;
    Ok(id)
}

/// Add a binary to the database.
pub fn insert_binary(tx: &Transaction, path: &str) -> Result<i64, Error> {
    if let Ok(id) = binary_id(tx, path) {
        Ok(id)
    } else {
        tx.execute("INSERT INTO binaries (path) VALUES (?1)", [path])?;
        binary_id(tx, path)
    }
}

/// Map syscall names.
pub fn get_names(syscalls: HashSet<i32>) -> Vec<String> {
    syscalls
        .into_iter()
        .filter_map(|i| Syscall::get_name(i).ok())
        .collect()
}

/// Get the syscalls used by a binary.
pub fn get_binary_syscalls(tx: &Transaction, binary: &str) -> Result<HashSet<i32>, Error> {
    let mut syscalls = HashSet::new();
    match binary_id(tx, binary) {
        Ok(binary_id) => {
            let mut stmt = tx
            .prepare("SELECT s.name FROM syscalls s JOIN binary_syscalls bs ON s.id = bs.syscall_id WHERE bs.binary_id = ?1")?;

            info!("Adding syscalls from {binary}");
            let rows = stmt.query_map([binary_id], |row| row.get::<_, i32>(0))?;
            for row in rows.flatten() {
                syscalls.insert(row);
            }
        }
        Err(e) => warn!("{binary} not found in binaries table: {e}"),
    }
    Ok(syscalls)
}

/// Add the syscalls from a binary to the working set.
fn extend(binary: &str, syscalls: &mut HashSet<i32>) -> Result<(), Error> {
    let mut conn = DB_POOL.get()?;
    let tx = conn.transaction()?;

    let binary = match binary.split_once('=') {
        Some((_, dest)) => dest,
        None => binary,
    };

    let resolved = match which_exclude(binary) {
        Ok(resolved) => Cow::Owned(resolved),
        Err(_) => Cow::Borrowed(binary),
    };

    for syscall in get_binary_syscalls(&tx, &resolved)? {
        syscalls.insert(syscall);
    }
    Ok(())
}

/// Get all syscalls for the profile.
pub fn get_calls(name: &str, p_binaries: &Option<BTreeSet<String>>) -> Result<HashSet<i32>, Error> {
    let mut conn = DB_POOL.get()?;
    let binaries = || -> Result<HashSet<String>, Error> {
        let tx = conn.transaction()?;

        // Get profile_id, insert profile if missing
        let profile_id = profile_id(&tx, name)?;

        let mut stmt = tx.prepare(
            "SELECT b.path
            FROM binaries b
            JOIN profile_binaries pb ON b.id = pb.binary_id
            WHERE pb.profile_id = ?1",
        )?;

        let mut binaries = HashSet::new();
        if let Ok(binaries_iter) = stmt.query_map([profile_id], |row| row.get::<_, String>(0)) {
            for bin_res in binaries_iter {
                binaries.insert(bin_res?);
            }
        }

        // Add extra binaries if passed
        if let Some(extra_bins) = p_binaries {
            for b in extra_bins {
                binaries.insert(b.clone());
            }
        }
        Ok(binaries)
    }()?;

    let mut syscalls = HashSet::new();
    binaries
        .iter()
        .try_for_each(|bin| extend(bin, &mut syscalls))?;
    Ok(syscalls)
}

/// Return a new Policy
pub fn new(
    name: &str,
    instance: &str,
    policy: SeccompPolicy,
    binaries: &Option<BTreeSet<String>>,
) -> Result<Filter, Error> {
    let syscalls = get_calls(name, binaries).unwrap_or_default();

    let mut filter = if policy == SeccompPolicy::Permissive {
        let mut filter = Filter::new(Action::Notify)?;
        filter.set_notifier(Notifier::new(
            user_dir(instance).join("monitor"),
            name.to_string(),
        ));

        filter
    } else {
        Filter::new(Action::KillProcess)?
    };

    filter.set_attribute(Attribute::NoNewPrivileges(true))?;
    filter.set_attribute(Attribute::ThreadSync(true))?;
    filter.set_attribute(Attribute::BadArchAction(Action::KillProcess))?;

    trace!("Allowing syscalls: {syscalls:?}");
    for syscall in syscalls {
        filter.add_rule(Action::Allow, Syscall::from_number(syscall))?;
    }
    Ok(filter)
}

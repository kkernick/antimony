use crate::{
    shared::{Set, env::AT_HOME, path::user_dir, profile::SeccompPolicy},
    timer,
};
use ahash::HashSetExt;
use common::cache::{self, CacheStatic};
use dashmap::DashMap;
use inotify::{Inotify, WatchMask};
use log::{debug, info, warn};
use nix::{
    errno,
    sys::socket::{self, ControlMessage, MsgFlags},
};
use parking_lot::{Mutex, MutexGuard};
use rusqlite::{Connection, Transaction};
use seccomp::{self, action::Action, attribute::Attribute, filter::Filter, syscall::Syscall};
use std::{
    borrow::Cow,
    error, fmt,
    fs::{self, File},
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, IoSlice},
    os::{
        fd::{AsRawFd, IntoRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    sync::LazyLock,
    thread::{ThreadId, sleep},
    time::Duration,
};
use user::{as_effective, as_real};

fn new_connection() -> Result<Connection, Error> {
    as_effective!({
        let dir = AT_HOME.join("seccomp");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        let conn = Connection::open(dir.join("syscalls.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Ok(conn)
    })?
}

pub static POOL: LazyLock<anyhow::Result<()>> = LazyLock::new(|| {
    as_effective!(anyhow::Result<()>, {
        let conn = new_connection()?;
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
        )?;
        Ok(())
    })?
});

static CONNECTION_CACHE: CacheStatic<ThreadId, Mutex<Connection>> = LazyLock::new(DashMap::default);
pub static CONNECTIONS: LazyLock<cache::Cache<ThreadId, Mutex<Connection>>> =
    LazyLock::new(|| cache::Cache::new(&CONNECTION_CACHE));

pub fn get_connection() -> Result<MutexGuard<'static, Connection>, Error> {
    if POOL.is_err() {
        Err(Error::Pool)
    } else {
        let id = std::thread::current().id();
        match CONNECTIONS.get(&id) {
            Some(connection) => Ok(connection.lock()),
            None => {
                CONNECTIONS.insert(id, Mutex::new(new_connection()?));
                Ok(CONNECTIONS.get(&id).unwrap().lock())
            }
        }
    }
}

static CACHE: LazyLock<DashMap<String, i32, ahash::RandomState>> = LazyLock::new(DashMap::default);

pub fn get_name(name: &str) -> i32 {
    // Insert if missing
    if !CACHE.contains_key(name) {
        CACHE.insert(
            name.to_string(),
            Syscall::from_name(name).unwrap().get_number(),
        );
    }
    *CACHE.get(name).unwrap().value()
}

/// Errors relating to SECCOMP policy generation.
#[derive(Debug)]
pub enum Error {
    /// Errors from the `seccomp` crate.
    Seccomp(seccomp::Error),

    /// Errors interfacing with the database
    Database(rusqlite::Error),

    /// Errors for IO.
    Io(io::Error),

    /// Misc Errnos.
    Errno(errno::Errno),

    /// Error when the pool failed to initialize
    Pool,

    /// Errors with multithreading.
    Sync,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Seccomp(e) => write!(f, "SECCOMP Error: {e}"),
            Self::Database(e) => write!(f, "Database Error: {e}"),
            Self::Io(e) => write!(f, "Io Error: {e}"),
            Self::Errno(e) => write!(f, "Error {e}"),
            Self::Pool => write!(f, "Database pool could not be initialized"),
            Self::Sync => write!(f, "Multiple references exist to Connection!"),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Seccomp(e) => Some(e),
            Self::Database(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Errno(e) => Some(e),
            _ => None,
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
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}
impl From<errno::Errno> for Error {
    fn from(value: errno::Errno) -> Self {
        Error::Errno(value)
    }
}

/// The Antimony Monitor Notifier Implementation.
pub struct Notifier {
    /// The path to the socket.
    path: PathBuf,

    /// The name of the profile to pass to the monitor
    name: String,

    /// The stream to the monitor, established in `prepare()`
    stream: Option<UnixStream>,

    /// An optional inotify instance to use instead of busy-waiting for the monitor.
    notify: Option<Inotify>,
}
impl Notifier {
    /// Construct a new Notifier from the path the monitor should listen,
    /// and the name of the process.
    pub fn new(path: PathBuf, name: String) -> Self {
        let notify = if let Ok(notify) = Inotify::init()
            && let Some(parent) = path.parent()
            && notify.watches().add(parent, WatchMask::ALL_EVENTS).is_ok()
        {
            Some(notify)
        } else {
            None
        };

        Self {
            path,
            name,
            stream: None,
            notify,
        }
    }
}
impl seccomp::notify::Notifier for Notifier {
    /// The Notifier needs sendmsg.
    fn exempt(&self) -> Vec<(Action, Syscall)> {
        vec![(
            Action::Log,
            Syscall::from_name("sendmsg").expect("Failed to get syscall"),
        )]
    }

    /// Setup the UnixStream. We wait for the Monitor to setup the socket.
    fn prepare(&mut self) -> Result<(), String> {
        if let Err(e) = as_real!({
            if !self.path.exists() {
                if let Some(mut notify) = self.notify.take() {
                    let mut buffer = [0; 1024];
                    let _ = notify.read_events_blocking(&mut buffer);
                } else {
                    while !self.path.exists() {
                        sleep(Duration::from_millis(10));
                    }
                }
            }
            match UnixStream::connect(&self.path) {
                Ok(stream) => {
                    self.stream = Some(stream);
                    Ok(())
                }
                Err(e) => Err(format!("Failed to connect to stream: {e}")),
            }
        }) {
            Err(format!("Failed to get monitor socket: {e}"))
        } else {
            Ok(())
        }
    }

    /// Send the FD to the Monitor.
    fn handle(&mut self, fd: OwnedFd) {
        let stream = self.stream.take().expect("Failed to get stream");
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
        tx.execute("INSERT INTO profiles (name) VALUES (?1)", [name])?;
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
pub fn get_names(syscalls: Set<i32>) -> Vec<String> {
    syscalls
        .into_iter()
        .filter_map(|i| Syscall::get_name(i).ok())
        .collect()
}

pub fn id_syscalls(
    tx: &Transaction,
    binary: &str,
    id: i64,
    syscalls: &mut Set<i32>,
) -> Result<(), Error> {
    let mut stmt = tx
    .prepare("SELECT s.name FROM syscalls s JOIN binary_syscalls bs ON s.id = bs.syscall_id WHERE bs.binary_id = ?1")?;

    info!("Adding syscalls from {binary}");
    let rows = stmt.query_map([id], |row| row.get::<_, i32>(0))?;
    for row in rows.flatten() {
        syscalls.insert(row);
    }
    Ok(())
}

/// Get the syscalls used by a binary.
pub fn get_binary_syscalls(tx: &Transaction, binary: &str) -> Result<Set<i32>, Error> {
    let mut syscalls = Set::new();
    if let Ok(id) = binary_id(tx, binary) {
        id_syscalls(tx, binary, id, &mut syscalls)?;
    } else if let Ok(link) = fs::read_link(PathBuf::from(binary)) {
        if let Ok(id) = binary_id(tx, &link.to_string_lossy()) {
            id_syscalls(tx, binary, id, &mut syscalls)?;
        }
    } else {
        warn!("{binary} not found in binaries table")
    }
    Ok(syscalls)
}

/// Add the syscalls from a binary to the working set.
fn extend(tx: &Transaction, binary: &str, syscalls: &mut Set<i32>) -> Result<(), Error> {
    let binary = match binary.split_once('=') {
        Some((_, dest)) => dest,
        None => binary,
    };

    let resolved = match which::which(binary) {
        Ok(resolved) => Cow::Borrowed(resolved),
        Err(_) => Cow::Borrowed(binary),
    };

    for syscall in get_binary_syscalls(tx, &resolved)? {
        syscalls.insert(syscall);
    }
    Ok(())
}

type PolicyPair = (Set<i32>, Set<i32>);

/// Get all syscalls for the profile.
pub fn get_calls(name: &str, p_binaries: &Set<String>) -> PolicyPair {
    let mut syscalls = Set::new();
    let mut bwrap = Set::new();

    if let Ok(mut conn) = get_connection() {
        let query_result = || -> Result<(), Error> {
            let tx = conn.transaction()?;
            let profile_id = insert_profile(&tx, name)?;

            let mut stmt = tx.prepare(
                "SELECT b.path
            FROM binaries b
            JOIN profile_binaries pb ON b.id = pb.binary_id
            WHERE pb.profile_id = ?1",
            )?;

            let mut binaries = Set::new();
            if let Ok(binaries_iter) = stmt.query_map([profile_id], |row| row.get::<_, String>(0)) {
                for bin_res in binaries_iter {
                    binaries.insert(bin_res?);
                }
            }

            // Add extra binaries if passed
            for b in p_binaries {
                binaries.insert(b.clone());
            }
            binaries.iter().for_each(|bin| {
                if let Err(e) = extend(
                    &tx,
                    bin,
                    if bin.ends_with("bwrap") {
                        &mut bwrap
                    } else {
                        &mut syscalls
                    },
                ) {
                    warn!("Failed to extend syscalls for binary {bin}: {e}");
                }
            });
            Ok(())
        }();

        if let Err(e) = query_result {
            warn!("Could not query the SECCOMP Database: {e}. Filter is incomplete");
        }

        bwrap = bwrap.difference(&syscalls).cloned().collect();
    }
    (syscalls, bwrap)
}

/// Return a new Policy
pub fn new(
    name: &str,
    instance: &str,
    policy: SeccompPolicy,
    binaries: &Set<String>,
) -> Result<Option<(Filter, Option<OwnedFd>, bool)>, Error> {
    let (mut syscalls, bwrap) = timer!("::get_calls", get_calls(name, binaries));
    let mut filter = if policy == SeccompPolicy::Permissive || policy == SeccompPolicy::Notifying {
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

    for required in ["execve", "wait4", "exit"] {
        syscalls.insert(Syscall::from_name(required)?.get_number());
    }

    let audit = !syscalls.contains(&get_name("sendmsg"));

    let syscalls = syscalls.into_iter().collect::<Vec<_>>();
    timer!("::add_rules", {
        for syscall in &syscalls {
            filter.add_rule(Action::Allow, Syscall::from_number(*syscall))?;
        }
    });

    let fd = if policy == SeccompPolicy::Enforcing {
        let mut s = DefaultHasher::new();
        syscalls.hash(&mut s);
        let hash = format!("{}", s.finish());

        debug!("Enforcing BPF");
        let bpf = AT_HOME
            .join("cache")
            .join(".seccomp")
            .join(format!("{hash}.bpf"));

        if let Some(parent) = bpf.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        Some(if !bpf.exists() {
            debug!("Writing filter...");
            filter.write(&bpf)?
        } else {
            File::open(&bpf)?.into()
        })
    } else {
        None
    };

    for syscall in bwrap {
        filter.add_rule(Action::Allow, Syscall::from_number(syscall))?;
    }
    Ok(Some((filter, fd, audit)))
}

//! This is a relatively simple implementation of a SECCOMP Notify Monitor.
//! It doesn't do anything more than log syscalls, permitting them all.
//!
//! This utility is used while a profile is in *Permissive* mode. When in
//! *Enforcing*, the logged syscalls are the only ones permitted, and the
//! `Filter` is loaded immediately.

use antimony::shared::{
    env::{DATA_HOME, RUNTIME_DIR},
    profile::SeccompPolicy,
    syscalls,
};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use dashmap::DashMap;
use inflector::Inflector;
use log::{debug, error, info, trace, warn};
use nix::{
    errno::Errno,
    libc::{EPERM, PR_SET_SECCOMP},
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::{
        signal::{
            Signal::{SIGKILL, SIGTERM},
            kill,
        },
        socket::{
            AddressFamily, ControlMessageOwned, MsgFlags, NetlinkAddr, SockFlag, SockProtocol,
            SockType, bind, recv, recvmsg, socket,
        },
    },
    unistd::Pid,
};
use once_cell::sync::Lazy;
use rusqlite::Transaction;
use seccomp::{notify::Pair, syscall::Syscall};
use spawn::Spawner;
use std::{
    collections::HashSet,
    fmt::Display,
    fs,
    io::{self, IoSliceMut},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use user::Mode;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Errno(nix::errno::Errno),
}
impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O Error: {e}"),
            Self::Errno(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Errno(e) => Some(e),
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<nix::errno::Errno> for Error {
    fn from(value: nix::errno::Errno) -> Self {
        Self::Errno(value)
    }
}

#[derive(Parser, Debug, Default)]
#[command(name = "Antimony-Monitor")]
#[command(version)]
#[command(about = "A SECCOMP-Notify application")]
pub struct Cli {
    #[arg(short, long)]
    pub instance: String,

    #[arg(short, long)]
    pub profile: String,

    #[arg(short, long)]
    pub mode: SeccompPolicy,
}

fn update_binary<'a, T: Iterator<Item = &'a i32>>(
    tx: &Transaction,
    binary: &str,
    syscalls: T,
) -> Result<()> {
    let binary_id = syscalls::insert_binary(tx, binary)?;

    let mut insert_syscall = tx.prepare("INSERT OR IGNORE INTO syscalls (name) VALUES (?1)")?;
    let mut get_syscall_id = tx.prepare("SELECT id FROM syscalls WHERE name = ?1")?;
    let mut insert_link = tx
        .prepare("INSERT OR IGNORE INTO binary_syscalls (binary_id, syscall_id) VALUES (?1, ?2)")?;

    for syscall in syscalls {
        insert_syscall.execute([syscall])?;
        let syscall_id: i64 = get_syscall_id.query_row([syscall], |row| row.get(0))?;
        insert_link.execute([&binary_id, &syscall_id])?;
    }

    Ok(())
}

fn update_profile(tx: &Transaction, profile: &str, binaries: &HashSet<String>) -> Result<()> {
    let profile_id = syscalls::insert_profile(tx, profile)?;
    for binary_name in binaries {
        let binary_id = syscalls::binary_id(tx, binary_name)?;
        tx.execute(
            "INSERT OR IGNORE INTO profile_binaries (profile_id, binary_id) VALUES (?1, ?2)",
            (profile_id, binary_id),
        )?;
    }
    Ok(())
}

/// Read from the Audit log
///
/// This function requires CAP_AUDIT_READ.
///
/// Because we cannot Notify for the syscalls used to send the SECCOMP FD to
/// the monitor, we LOG them instead.
pub fn audit_reader(term: Arc<AtomicBool>, log: Arc<DashMap<String, HashSet<i32>>>) -> Result<()> {
    const BUFFER_SIZE: usize = 4096;

    // Open netlink socket for audit
    let sock_fd = socket(
        AddressFamily::Netlink,
        SockType::Datagram,
        SockFlag::empty(),
        SockProtocol::NetlinkAudit,
    )?;

    let addr = NetlinkAddr::new(0, 1);
    bind(sock_fd.as_raw_fd(), &addr)?;

    debug!("Listening to Audit.");
    let mut buf = vec![0u8; BUFFER_SIZE];

    while !term.load(Ordering::Relaxed) {
        let size = recv(sock_fd.as_raw_fd(), &mut buf, MsgFlags::empty())?;
        if size == 0 {
            continue;
        }
        let msg = String::from_utf8_lossy(&buf[..size]);
        if msg.contains("syscall=") {
            // Grab the executable name
            let exe =
                if let Some(exe_match) = msg.split_whitespace().find(|s| s.starts_with("exe=")) {
                    exe_match
                        .trim_start_matches("exe=")
                        .trim_matches('"')
                        .to_string()
                } else {
                    continue;
                };

            // Grab the syscall name.
            let syscall = if let Some(syscall_match) =
                msg.split_whitespace().find(|s| s.starts_with("syscall="))
            {
                syscall_match
                    .trim_start_matches("syscall=")
                    .to_string()
                    .parse::<i32>()
            } else {
                continue;
            };

            // If everything is valid, log it.
            if let Ok(syscall) = syscall {
                let mut entry = log.entry(exe.clone()).or_default();
                if !entry.contains(&syscall) {
                    trace!("Audited Syscall: {exe} => {syscall}");
                    entry.insert(syscall);
                }
            }
        }
    }
    Ok(())
}

static SECCOMP: Lazy<i32> = Lazy::new(|| {
    Syscall::from_name("seccomp")
        .expect("Failed to code seccomp code")
        .get_number()
});

static PRCTL: Lazy<i32> = Lazy::new(|| {
    Syscall::from_name("prctl")
        .expect("Failed to code seccomp code")
        .get_number()
});

static FCHMOD: Lazy<i32> = Lazy::new(|| {
    Syscall::from_name("fchmod")
        .expect("Failed to code seccomp code")
        .get_number()
});

pub fn notify(profile: &str, call: i32, path: &Path) -> Result<String> {
    let name = Syscall::get_name(call)?;
    let mut handle = Spawner::new("notify-send")
        .args([
            &format!("Syscall Request: {} => {}", profile.to_title_case(), name.to_title_case()),
            &format!("The program <i>{}</i> attempted to use the syscall <b>{name}</b> within profile {profile}, which is not registered in its policy. What would you like to do?", path.to_string_lossy()),
            "-a", "Antimony",
            "-t", "30000",
            "-A", "All=Save All",
            "-A", "Save=Save",
            "-A", "Allow=Allow",
            "-A", "Deny=Deny",
            "-A", "Kill=Kill",
        ])?
        .preserve_env(true)
        .mode(Mode::Real)
        .output(true)
        .spawn()?;

    if handle.wait()? != 0 {
        return Err(anyhow!("Failed to get input!"));
    }
    let output = handle.output_all()?;
    Ok(output)
}

pub fn notify_reader(
    term: Arc<AtomicBool>,
    stats: Arc<DashMap<String, HashSet<i32>>>,
    fd: OwnedFd,
    name: String,
    ask: AtomicBool,
) -> Result<()> {
    let deny = Arc::new(DashMap::<String, HashSet<i32>>::new());
    let allow = Arc::new(DashMap::<String, HashSet<i32>>::new());
    let ask = Arc::new(ask);

    while !term.load(Ordering::Relaxed) {
        // New pair for each loop, since we don't want to mediate access to one.
        let pair = Pair::new()?;
        let stats_clone = Arc::clone(&stats);

        match pair.recv(fd.as_raw_fd()) {
            Ok(Some(_)) => {
                let log = Arc::clone(&stats_clone);

                let deny_clone = Arc::clone(&deny);
                let allow_clone = Arc::clone(&allow);
                let ask_clone = Arc::clone(&ask);

                let raw = fd.as_raw_fd();
                let profile_name = name.clone();

                // Spawn a handler.
                rayon::spawn(move || {
                    // Reply to the thread. Our handler just gets the name of the executable,
                    // resolves the syscall name, and permits the request.
                    if let Err(e) = pair.reply(raw, |req, resp| {
                        // Get the binary name
                        let pid = req.pid;
                        let exe_path = match fs::read_link(format!("/proc/{pid}/exe")) {
                            Ok(path) => Some(path),
                            Err(_) => {
                                warn!("Invalid exe at PID {pid}");
                                None
                            }
                        };

                        if let Some(exe_path) = exe_path {
                            let path = exe_path.to_string_lossy().into_owned();

                            // Get the syscall name
                            let call = req.data.nr;
                            let mut entry = log.entry(path.clone()).or_default();

                            if let Some(value) = deny_clone.get(&path)
                                && value.contains(&call)
                            {
                                resp.error = -EPERM;
                                resp.flags = 0;
                                return;
                            }

                            if let Some(value) = allow_clone.get(&path)
                                && value.contains(&call)
                            {
                                resp.val = 0;
                                resp.error = 0;
                                resp.flags = 1;
                                return;
                            }

                            // Add new values.
                            if !entry.contains(&call) {
                                let commit = if ask_clone.load(Ordering::Relaxed) {
                                    let mut commit = false;
                                    match notify(&profile_name, call, &exe_path) {
                                        Ok(result) => {
                                            resp.val = 0;
                                            resp.error = 0;
                                            resp.flags = 1;

                                            if !result.is_empty() {
                                                match &result[..result.len() - 1] {
                                                    "All" => {
                                                        commit = true;
                                                        ask_clone.store(false, Ordering::Relaxed);
                                                    }
                                                    "Save" => {
                                                        commit = true;
                                                    }
                                                    "Allow" => {
                                                        allow_clone
                                                            .entry(path.to_string())
                                                            .or_default()
                                                            .insert(call);
                                                    }
                                                    "Deny" => {
                                                        resp.error = -EPERM;
                                                        resp.flags = 0;
                                                        deny_clone
                                                            .entry(path.clone())
                                                            .or_default()
                                                            .insert(call);
                                                    }
                                                    "Kill" => {
                                                        // Kill the offending process with recourse.
                                                        let _ = kill(
                                                            Pid::from_raw(pid as i32),
                                                            SIGKILL,
                                                        );

                                                        // Let the others clean up.
                                                        if let Err(e) =
                                                            kill(Pid::from_raw(0), SIGTERM)
                                                        {
                                                            error!("Failed to kill child: {e}");
                                                        }
                                                    }
                                                    e => {
                                                        warn!("Unrecognized option: {e}");
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to ask user: {e}");
                                        }
                                    }
                                    commit
                                } else {
                                    true
                                };

                                if commit {
                                    user::sync::run_as!(user::Mode::Effective, {
                                        if let Ok(mut conn) = syscalls::DB_POOL.get()
                                            && let Ok(tx) = conn.transaction()
                                            && update_binary(&tx, &path, [call].iter()).is_ok()
                                            && tx.commit().is_ok()
                                        {
                                            info!(
                                                "{path} => {}",
                                                Syscall::get_name(call)
                                                    .unwrap_or(format!("{call}"))
                                            );
                                        } else {
                                            warn!("Pending commit");
                                            entry.insert(call);
                                        }
                                    });
                                    allow_clone.entry(path).or_default().insert(call);
                                }
                            }
                        }

                        let call = req.data.nr;
                        let args = req.data.args;

                        resp.val = 0;
                        resp.error = 0;
                        resp.flags = 1;

                        // If a SECCOMP Policy is installed with a higher precedence than
                        // ours (NOTIFY is pretty low), it will replace the filter, and deny
                        // us access to the syscalls.
                        //
                        // So, we lie and pretend the filter was applied, without actually doing
                        // anything. Chromium/Electron, for some reason, do not use seccomp_api_get
                        // to determine features, but instead send null pointers to test capabilities.
                        // We handle both cases, and only ignore filters that would have actually worked.
                        if ((call == *PRCTL && args[0] == PR_SET_SECCOMP as u64)
                            || call == *SECCOMP)
                            && args[2] != 0
                        {
                            trace!("Ignoring SECCOMP request");
                            resp.flags = 0;

                        // Chromium/Electron use this to test that SECCOMP works.
                        } else if call == *FCHMOD && args[0] as i32 == -1 && args[1] == 0o7777 {
                            trace!("Injected fchmod => EPERM");
                            resp.error = -EPERM;
                            resp.flags = 0;
                        }
                    }) {
                        warn!("Failed to reply: {e}");
                    }
                });
            }
            Ok(None) => continue,
            Err(e) => {
                error!("Fatal error: {e}");
                break;
            }
        }
    }
    Ok(())
}

/// Poll on Accept, Timing out after timeout.
fn accept_with_timeout(
    listener: &UnixListener,
    timeout: PollTimeout,
) -> Result<Option<UnixStream>, Error> {
    listener.set_nonblocking(true)?;

    let fd = listener.as_fd();
    let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];

    let res = poll(&mut fds, timeout)?;

    if res == 0 {
        // Timed out
        Ok(None)
    } else {
        // Ready to accept
        match listener.accept() {
            Ok((stream, _addr)) => Ok(Some(stream)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Receive a file descriptor from a Unix socket as an `OwnedFd`.
pub fn receive_fd(listener: &UnixListener) -> Result<Option<(OwnedFd, String)>, Error> {
    let stream = accept_with_timeout(listener, PollTimeout::from(100u16))?;
    if let Some(stream) = stream {
        let mut buf = [0u8; 256];
        let pair = || -> Result<Option<(OwnedFd, usize)>, Errno> {
            let raw_fd = stream.as_raw_fd();

            let mut io = [IoSliceMut::new(&mut buf)];
            let mut msg_space = nix::cmsg_space!([RawFd; 1]);

            let msg = recvmsg::<()>(raw_fd, &mut io, Some(&mut msg_space), MsgFlags::empty())?;

            for cmsg in msg.cmsgs()? {
                if let ControlMessageOwned::ScmRights(fds) = cmsg
                    && let Some(fd) = fds.first()
                {
                    let owned_fd = unsafe { OwnedFd::from_raw_fd(*fd) };
                    return Ok(Some((owned_fd, msg.bytes)));
                }
            }
            Ok(None)
        }()?;

        if let Some((fd, bytes)) = pair {
            let name = String::from_utf8_lossy(&buf[..bytes])
                .trim_end_matches(char::from(0))
                .to_string();
            return Ok(Some((fd, name)));
        }
    }
    Ok(None)
}

/// Receive and Respond to Notify Requests.
fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    // We dispatch requests to a thread pool for performance.
    rayon::ThreadPoolBuilder::new().build_global()?;

    info!(
        "SECCOMP Monitor Started! ({} at {} using {:?})",
        cli.profile, cli.instance, cli.mode
    );

    // Setup the socket. We run this as the user.
    user::set(Mode::Real)?;
    let monitor_path = RUNTIME_DIR
        .join("antimony")
        .join(&cli.instance)
        .join("monitor");
    let listener = UnixListener::bind(&monitor_path)?;

    // Ensure that we can record syscall info after the attached process dies.
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    // Shared DashSet for stats.
    let stats = DashMap::<String, Arc<DashMap<String, HashSet<i32>>>>::new();

    let audit = stats
        .entry("audit".to_string())
        .or_insert_with(|| Arc::new(DashMap::new()))
        .clone();

    let audit_term = term.clone();

    thread::spawn(move || audit_reader(audit_term, audit));

    while !term.load(Ordering::Relaxed) {
        match receive_fd(&listener) {
            Ok(Some((fd, name))) => {
                info!("New connection established with {name}!");

                let term_clone = term.clone();
                let profile = stats
                    .entry(name.clone())
                    .or_insert_with(|| Arc::new(DashMap::new()))
                    .clone();

                let profile_name = cli.profile.clone();
                thread::spawn(move || {
                    notify_reader(
                        term_clone,
                        profile,
                        fd,
                        profile_name.clone(),
                        AtomicBool::new(cli.mode == SeccompPolicy::Notify),
                    )
                });
            }
            Ok(None) | Err(Error::Errno(Errno::EINTR)) => continue,
            Err(e) => {
                error!("Failed to received fd: {e}");
                break;
            }
        }
    }

    info!("Storing syscall data.");

    // Once we're done, move to Effective to save the information.
    user::set(Mode::Effective)?;

    if !stats.is_empty() {
        let mut conn = syscalls::DB_POOL.get()?;
        let tx = conn.transaction()?;
        println!("\n=== Syscall Summary ===");

        // Make sure the binary is either in the profile's persist home, or
        // exists.
        let binary_exist = |path: &str| -> Result<bool> {
            Ok(if path.starts_with("/home/antimony") {
                let path = path.replace("/home/antimony", "*");
                !Spawner::new("/usr/bin/find")
                    .arg(DATA_HOME.join("antimony").to_string_lossy())?
                    .args(["-wholename", &path])?
                    .mode(user::Mode::Real)
                    .output(true)
                    .spawn()?
                    .output_all()?
                    .is_empty()
            } else if path.ends_with("flatpak-spawn") {
                true
            } else {
                Path::new(&path).exists()
            })
        };

        for (name, stats) in stats {
            debug!("Updating {name}");
            // Collect and insert syscall sets
            let binaries: HashSet<String> = stats
                .iter_mut()
                .filter_map(|mut entry| {
                    let binary = entry.key().clone();
                    let syscalls = entry.value_mut();

                    match binary_exist(&binary) {
                        Ok(true) => {
                            println!("{}: {} => {:?}", binary, syscalls.len(), syscalls);

                            // Insert into DB using the transaction
                            if let Err(e) = update_binary(&tx, &binary, syscalls.iter()) {
                                warn!("DB insert failed for {binary}: {e}");
                                return None;
                            }

                            if binary.contains("strace") {
                                None
                            } else {
                                Some(binary.clone())
                            }
                        }
                        _ => {
                            info!("Ignoring ephemeral binary {binary}");
                            None
                        }
                    }
                })
                .collect();

            if name != "audit" {
                update_profile(&tx, &name, &binaries).with_context(|| "Updating profile")?;
            }
        }
        println!("========================\n");
        tx.commit()?;

        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .with_context(|| "Flushing WAL")?;
    }
    info!("Finished: {}", cli.instance);
    Ok(())
}

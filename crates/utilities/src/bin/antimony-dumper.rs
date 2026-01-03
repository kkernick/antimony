//! A SECCOMP Notify application that reads the paths to `execve` to determine
//! binaries called by the application.
//!
//! The `attach` sub-command is used internally. You should call this like:
//!
//! ```bash
//! antimony-dumper run my-program 12345
//! ```
//!
//! Or whatever instance value you have.

use antimony::shared::{
    self,
    path::user_dir,
    syscalls::{self, Notifier},
    utility,
};
use anyhow::Result;
use caps::Capability;
use clap::{Parser, Subcommand};
use common::stream::receive_fd;
use dashmap::DashMap;
use inotify::{Inotify, WatchMask};
use nix::{
    libc::{EPERM, PR_SET_SECCOMP},
    sys::signal::{Signal::SIGKILL, kill},
    unistd::Pid,
};
use seccomp::{action::Action, attribute::Attribute, filter::Filter, notify::Pair};
use spawn::Spawner;
use std::{
    fs::{self, File},
    io::{self, Read, Seek},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::UnixListener,
    },
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[derive(Parser, Debug)]
#[command(name = "Antimony Dumper")]
#[command(version)]
#[command(about = "Dump information about binaries")]
pub struct Cli {
    /// What to do
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the program under a dumper.
    Run(RunArgs),

    /// Attach a dumper to an existing instance.
    Attach(AttachArgs),
}

#[derive(clap::Args, Debug, Default, Clone)]
pub struct RunArgs {
    /// The path to the binary to monitor
    #[arg(long)]
    path: String,

    /// The instance ID, for localizing the socket. This can be any value.
    #[arg(long)]
    instance: String,

    /// Do not enforce a timeout on the application.
    #[arg(long, default_value_t = false)]
    no_timeout: bool,
}

#[derive(clap::Args, Debug, Default, Clone)]
pub struct AttachArgs {
    /// The path to the socket established by `run`.
    socket: String,
}

/// The current process, to allow all syscalls coming from it.
static SELF: LazyLock<PathBuf> =
    LazyLock::new(|| fs::read_link("/proc/self/exe").expect("Failed to get self"));

/// Collect all paths from a syscall.
/// This is a naive approach that simply checks each argument for a valid
/// path.
pub fn collect_paths(pid: u32, args: &[u64; 6]) -> Result<Vec<String>> {
    let path = PathBuf::from(format!("/proc/{pid}/mem"));
    let mut mem_file = File::open(path)?;
    let mut ret = Vec::new();

    let mut read = |arg: u64| -> Result<String> {
        mem_file.seek(io::SeekFrom::Start(arg))?;
        let buffer = &mut [0u8; 256];
        let bytes_read = mem_file.read(&mut buffer[..])?;
        let end_pos = buffer.iter().position(|&b| b == 0).unwrap_or(bytes_read);
        let string = str::from_utf8(&buffer[..end_pos])?;
        if Path::new(string).exists() {
            Ok(string.to_string())
        } else {
            Err(anyhow::anyhow!("Not a path!"))
        }
    };

    for arg in args {
        if *arg == 0 {
            break;
        } else if let Ok(str) = read(*arg) {
            ret.push(str);
        }
    }

    Ok(ret)
}

/// Listen on the Kernel FD and find paths.
pub fn reader(term: Arc<AtomicBool>, fd: OwnedFd) -> Result<()> {
    let found = Arc::new(DashMap::<u32, Vec<String>>::new());

    while !term.load(Ordering::Relaxed) {
        let pair = Pair::new()?;
        match pair.recv(fd.as_raw_fd()) {
            Ok(Some(_)) => {
                let _ = pair.reply(fd.as_raw_fd(), |req, resp| {
                    let pid = req.pid;
                    let call = req.data.nr;
                    let args = req.data.args;

                    if let Ok(exe_path) = fs::read_link(format!("/proc/{pid}/exe"))
                        && exe_path == SELF.as_path()
                    {
                        resp.val = 0;
                        resp.error = 0;
                        resp.flags = 1;
                        return;
                    }

                    // We only care about exec.
                    if call == syscalls::get_name("execve")
                        && let Ok(paths) = collect_paths(pid, &args)
                        && !paths.is_empty()
                    {
                        let mut local_found = found.entry(pid).or_default();
                        for path in paths {
                            if !local_found.contains(&path) {
                                println!("{path}");
                                local_found.push(path);
                            }
                        }

                    // We bail on these syscalls, since they're seen
                    // as the program falling into a steady-state.
                    } else if call == syscalls::get_name("ppoll")
                        || call == syscalls::get_name("wait4")
                    {
                        term.store(true, Ordering::Relaxed);
                        let _ = kill(Pid::from_raw(pid as i32), SIGKILL);
                        resp.error = -EPERM;
                        resp.flags = 0;
                        return;
                    }

                    resp.val = 0;
                    resp.error = 0;
                    resp.flags = 1;

                    // Ignore SECCOMP and EXECVE.
                    if (((call == syscalls::get_name("prctl") && args[0] == PR_SET_SECCOMP as u64)
                        || call == syscalls::get_name("seccomp"))
                        && args[2] != 0)
                        || call == syscalls::get_name("execve")
                    {
                        resp.flags = 0;
                    }
                });
            }
            Ok(None) => continue,
            Err(_) => break,
        }
    }
    Ok(())
}

/// Collect syscall information from Kernel FDs.
pub fn collection(args: AttachArgs) -> Result<()> {
    let listener = UnixListener::bind(args.socket)?;

    // Ensure that we can record syscall info after the attached process dies.
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;
    while !term.load(Ordering::Relaxed) {
        match receive_fd(&listener) {
            Ok(Some((fd, _))) => {
                let term_clone = term.clone();
                thread::spawn(move || reader(term_clone, fd));
            }
            Ok(None) => continue,
            Err(_) => break,
        }
    }
    Ok(())
}

/// Run the application under a monitor.
pub fn runner(args: RunArgs) -> Result<()> {
    let name = match args.path.rfind('/') {
        Some(i) => &args.path[i + 1..],
        None => &args.path,
    };

    let path = user_dir(&args.instance);
    if !path.exists() {
        fs::create_dir_all(&path)?;
    }
    let socket = path.join(format!("dumper-{name}"));
    let sock_str = socket.to_string_lossy().into_owned();
    let mut inotify = Inotify::init()?;
    inotify.watches().add(&path, WatchMask::CREATE)?;

    // Setup the filter.
    let mut filter = Filter::new(Action::Notify)?;
    filter.set_notifier(Notifier::new(socket, name.to_string()));
    filter.set_attribute(Attribute::NoNewPrivileges(true))?;
    filter.set_attribute(Attribute::ThreadSync(true))?;
    filter.set_attribute(Attribute::BadArchAction(Action::KillProcess))?;

    // Create an attached version to listen.
    let _handle = Spawner::abs(utility("dumper"))
        .args(["attach", &sock_str])?
        .preserve_env(true)
        .new_privileges(true)
        .cap(Capability::CAP_SYS_PTRACE)
        .spawn()?;

    // Wait for it to be ready.
    let mut buffer = [0; 1024];
    let _ = inotify.read_events_blocking(&mut buffer)?;

    // Spawn the program under our Notify policy.
    Spawner::new(args.path)?
        .preserve_env(true)
        .output(spawn::StreamMode::Pipe)
        .error(spawn::StreamMode::Pipe)
        .seccomp(filter)
        .spawn()?
        .wait()?;

    fs::remove_file(sock_str)?;
    Ok(())
}

fn main() -> Result<()> {
    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;
    match Cli::parse().command {
        Command::Run(args) => runner(args)?,
        Command::Attach(args) => collection(args)?,
    };
    Ok(())
}

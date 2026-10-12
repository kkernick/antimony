//! A SECCOMP Notify application that reads the paths to `execve` to determine
//! binaries called by the application.
//!
//! The `attach` subcommand is used internally. You should call this like:
//!
//! ```bash
//! antimony-dumper run my-program 12345
//! ```
//!
//! Or whatever instance value you have.
#![allow(unused_crate_dependencies)]

use antimony::{
    fab::bin::in_bin,
    shared::{
        self, ThreadMap, create_path,
        stream::receive_fd,
        syscalls::{self, Notifier, get_num},
        utility,
        which::AntimonyWhich,
    },
};
use anyhow::{Result, anyhow};
use caps::Capability;
use clap::{Parser, Subcommand, ValueHint};
use inotify::{Inotify, WatchMask};
use log::info;
use nix::{
    libc::{EPERM, PR_SET_SECCOMP},
    sys::signal::{Signal::SIGKILL, kill},
    unistd::Pid,
};
use seccomp::{
    action::Action, attribute::Attribute, filter::Filter, notify::Pair, syscall::Syscall,
};
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
};

#[derive(Parser)]
#[command(name = "Antimony Dumper")]
#[command(version)]
#[command(about = "Dump information about binaries")]
pub struct Cli {
    /// What to do
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the program under a dumper.
    Run(RunArgs),

    /// Attach a dumper to an existing instance.
    Attach(AttachArgs),
}

#[derive(clap::Args, Default, Clone)]
pub struct RunArgs {
    /// The path to the binary to monitor
    #[arg(long, value_hint = ValueHint::CommandName)]
    path: String,

    /// The instance path for localizing the socket. This can be any directory.
    #[arg(long, value_hint = ValueHint::DirPath)]
    instance: String,

    /// Only collect from the specified syscall names.
    #[arg(long)]
    filter: Option<Vec<String>>,
}

#[derive(clap::Args, Default, Clone)]
pub struct AttachArgs {
    /// The path to the socket established by `run`.
    socket: String,

    /// Only collect from the specified syscall names.
    #[arg(long)]
    filter: Option<Vec<i32>>,
}

/// The current process, to allow all syscalls coming from it.
static SELF: LazyLock<PathBuf> =
    LazyLock::new(|| fs::read_link("/proc/self/exe").expect("Failed to get self"));

/// Collect all paths from a syscall.
/// This is a naive approach that simply checks each argument for a valid
/// path.
///
/// ## Errors
/// If fetching the memory map of the process fails, or it can not be read.
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
            Ok(string.to_owned())
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
///
/// ## Errors
/// If a new pair cannot be made.
#[allow(clippy::cast_possible_wrap, clippy::needless_pass_by_value)]
#[allow(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::significant_drop_tightening,
    reason = "Antimony expects basic syscalls to exist"
)]
pub fn reader(fd: OwnedFd, filter: Vec<i32>) -> Result<()> {
    let found = Arc::new(ThreadMap::<u32, Vec<String>>::default());
    let term = AtomicBool::new(false);
    while !term.load(Ordering::Relaxed) {
        let pair = Pair::new()?;
        match pair.recv(fd.as_raw_fd()) {
            Ok(Some(())) => {
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

                    if (filter.is_empty() || filter.contains(&call))
                        && let Ok(paths) = collect_paths(pid, &args)
                        && !paths.is_empty()
                    {
                        if call == syscalls::get_num("execve").unwrap() && !in_bin(&paths[0]) {
                            println!("{}", paths[0]);
                            term.store(true, Ordering::Relaxed);
                            let _ = kill(Pid::from_raw(pid as i32), SIGKILL);
                            resp.error = -EPERM;
                            resp.flags = 0;
                            return;
                        }

                        let mut local_found = found.entry(pid).or_default();
                        for path in paths {
                            if !local_found.contains(&path) {
                                info!(
                                    "{} => {path}",
                                    Syscall::get_name(call).unwrap_or_else(|_| format!("{call}"))
                                );
                                println!("{path}");
                                local_found.push(path);
                            }
                        }
                    }

                    resp.val = 0;
                    resp.error = 0;
                    resp.flags = 1;

                    // Ignore SECCOMP.
                    if ((call == syscalls::get_num("prctl").unwrap()
                        && args[0] == PR_SET_SECCOMP as u64)
                        || call == syscalls::get_num("seccomp").unwrap())
                        && args[2] != 0
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
///
/// ## Errors
/// If we cannot bind to the socket or register the signal handler
pub fn collection(args: AttachArgs) -> Result<()> {
    let listener = UnixListener::bind(args.socket)?;

    if let Ok(Some((fd, _))) = receive_fd(&listener) {
        reader(fd, args.filter.unwrap_or_default())
    } else {
        Err(anyhow!("Failed to receive SECCOMP FD"))
    }
}

/// Run the application under a monitor.
/// ## Errors
/// If the path cannot be created, we cannot watch the temporary path, or SECCOMP Filter initializes fails.
pub fn runner(args: RunArgs) -> Result<()> {
    let name = match args.path.rfind('/') {
        Some(i) => {
            if let Some(i) = i.checked_add(1)
                && let Some(arg) = args.path.get(i..)
            {
                arg
            } else {
                return Err(anyhow::anyhow!("Index error!"));
            }
        }
        None => &args.path,
    };

    let path = PathBuf::from(args.instance);
    create_path(path.as_path())?;

    let temp = temp::Builder::new()
        .within(path)
        .name(format!("dumper-{name}"))
        .owner(user::Mode::Real)
        .make(false)
        .create::<temp::File>()?;

    let socket = temp.full();
    let sock_str = socket.to_string_lossy().into_owned();
    info!("Listening on {sock_str}");

    let mut inotify = Inotify::init()?;
    inotify.watches().add(temp.path(), WatchMask::CREATE)?;

    // Set up the filter.
    let mut filter = Filter::new(Action::Notify)?;
    filter.set_notifier(Notifier::new(socket, name.to_owned()));
    filter.set_attribute(Attribute::NoNewPrivileges(true))?;
    filter.set_attribute(Attribute::ThreadSync(true))?;
    filter.set_attribute(Attribute::BadArchAction(Action::KillProcess))?;

    // Create an attached version to listen.
    let monitor = Spawner::abs(utility("dumper"))
        .args(["attach", &sock_str])
        .preserve_env(true)
        .new_privileges(true)
        .cap(Capability::CAP_SYS_PTRACE);

    if let Some(filter) = args.filter {
        filter
            .into_iter()
            .filter_map(|name| get_num(&name))
            .for_each(|syscall| {
                monitor.args_i(["--filter", &format!("{syscall}")]);
            });
    }
    let monitor = monitor.spawn()?;

    // Wait for it to be ready.
    let mut buffer = [0; 1024];
    'outer: loop {
        let events = inotify.read_events_blocking(&mut buffer)?;
        for event in events {
            if let Some(name) = event.name
                && name == temp.name()
            {
                break 'outer;
            }
        }
    }

    // Spawn the program under our Notify policy.
    let mut handle = Spawner::which::<AntimonyWhich>(args.path)?
        .preserve_env(true)
        .output(spawn::StreamMode::Pipe)
        .error(spawn::StreamMode::Pipe)
        .seccomp(filter)
        .spawn()?;

    // Wait for the monitor to finish.
    monitor.wait()?;

    // Terminate the child.
    handle.terminate()?;
    Ok(())
}

fn main() -> Result<()> {
    notify::init_error()?;
    notify::set_notifier(Box::new(shared::logger))?;
    match Cli::parse().command {
        Command::Run(args) => runner(args)?,
        Command::Attach(args) => collection(args)?,
    }
    Ok(())
}

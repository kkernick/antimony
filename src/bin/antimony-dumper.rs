use antimony::shared::{
    path::user_dir,
    syscalls::{self, Notifier},
};
use anyhow::Result;
use clap::{Parser, Subcommand};
use inotify::{Inotify, WatchMask};
use log::{error, trace, warn};
use nix::libc::PR_SET_SECCOMP;
use once_cell::sync::Lazy;
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
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Parser, Debug)]
#[command(name = "Antimony Dumper")]
#[command(version)]
#[command(about = "Dump information about binaries")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Run(RunArgs),

    Attach(AttachArgs),
}

#[derive(clap::Args, Debug, Default, Clone)]
pub struct RunArgs {
    path: String,

    instance: String,

    #[arg(short, long, default_value_t = false)]
    no_timeout: bool,
}

#[derive(clap::Args, Debug, Default, Clone)]
pub struct AttachArgs {
    socket: String,
}

pub static SELF: Lazy<PathBuf> =
    Lazy::new(|| fs::read_link("/proc/self/exe").expect("Failed to get self"));

pub fn collect_paths(pid: u32, args: &[u64; 6]) -> Result<Vec<String>> {
    let path = PathBuf::from(format!("/proc/{pid}/mem"));
    let mut mem_file = File::open(path)?;
    let mut ret = Vec::new();

    let mut read = |arg: u64| -> Result<String> {
        mem_file.seek(io::SeekFrom::Start(arg))?;
        let mut buffer = vec![0u8; 256];
        let bytes_read = mem_file.read(&mut buffer)?;
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

pub fn reader(term: Arc<AtomicBool>, fd: OwnedFd) -> Result<()> {
    while !term.load(Ordering::Relaxed) {
        let pair = Pair::new()?;
        match pair.recv(fd.as_raw_fd()) {
            Ok(Some(_)) => {
                let raw = fd.as_raw_fd();
                rayon::spawn(move || {
                    let _ = pair.reply(raw, |req, resp| {
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

                        if let Ok(paths) = collect_paths(pid, &args)
                            && !paths.is_empty()
                        {
                            if let Ok(name) = Syscall::get_name(call) {
                                trace!("{name} => {paths:?}");
                            }

                            for path in paths {
                                if ["/usr/bin", "/usr/lib", "/bin", "/lib"]
                                    .iter()
                                    .any(|r| path.starts_with(r))
                                {
                                    println!("{path}");
                                }
                            }
                        }

                        resp.val = 0;
                        resp.error = 0;
                        resp.flags = 1;

                        // Ignore SECCOMP and EXECVE.
                        if (((call == *syscalls::PRCTL && args[0] == PR_SET_SECCOMP as u64)
                            || call == *syscalls::SECCOMP)
                            && args[2] != 0)
                            || call == *syscalls::EXECVE
                        {
                            resp.flags = 0;
                        }
                    });
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

pub fn collection(args: AttachArgs) -> Result<()> {
    let listener = UnixListener::bind(args.socket)?;

    // Ensure that we can record syscall info after the attached process dies.
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;
    while !term.load(Ordering::Relaxed) {
        match syscalls::receive_fd(&listener) {
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

    let mut filter = Filter::new(Action::Notify)?;
    filter.set_notifier(Notifier::new(socket, name.to_string()));
    filter.set_attribute(Attribute::NoNewPrivileges(true))?;
    filter.set_attribute(Attribute::ThreadSync(true))?;
    filter.set_attribute(Attribute::BadArchAction(Action::KillProcess))?;

    let _handle = Spawner::new("/usr/bin/antimony-dumper")
        .args(["attach", &sock_str])?
        .preserve_env(true)
        .spawn()?;

    let mut buffer = [0; 1024];
    let _ = inotify.read_events_blocking(&mut buffer)?;

    if let Err(e) = Spawner::new(args.path)
        .preserve_env(true)
        .output(true)
        .error(true)
        .seccomp(filter)
        .spawn()?
        .wait(if args.no_timeout {
            None
        } else {
            Some(Duration::from_millis(10))
        })
    {
        warn!("Binary exited with an error: {e}");
    }

    fs::remove_file(sock_str)?;
    Ok(())
}

fn main() -> Result<()> {
    rayon::ThreadPoolBuilder::new().build_global()?;
    env_logger::init();
    match Cli::parse().command {
        Command::Run(args) => runner(args)?,
        Command::Attach(args) => collection(args)?,
    };
    Ok(())
}

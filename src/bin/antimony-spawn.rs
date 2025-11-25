//! This application is an implementation of flatpak-spawn for Antimony.
//! Some of its arguments are no-ops, and only exist for a compatible interface,
//! but it's largely feature complete. There are two important distinctions:
//!
//! The dichotomy between --sandbox and --host are vastly different. When
//! running in --sandbox, the requested command is run in a new Antimony
//! sandbox, constructed from the command line arguments. Because the
//! environment antimony-spawn runs in (Antimony itself) does not have
//! an Antimony installation, we have no features or profiles; therefore,
//! we pass the sandbox's lib folder directly--which has already been
//! reduced to only needing what the main application and its dependencies
//! require.
//!
//! --host does not run a command on the host. It just runs the command
//! directly in the sandbox. Allowing execution on the host, even
//! mediated, is a tremendous vulnerability, and trying to implement it
//! would require significant effort.
//!
//! --watch-bus is a no-op: antimony-spawn will either exit if the child
//! it runs does, or if it is hit with SIGTERM or SIGINT.
use anyhow::Result;
use clap::Parser;
use nix::unistd::chdir;
use spawn::Spawner;
use std::{
    borrow::Cow,
    env,
    os::fd::{FromRawFd, OwnedFd},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::sleep,
    time::Duration,
};

#[derive(Parser, Debug, Default)]
#[command(name = "Antimony-Spawn")]
#[command(version)]
#[command(about = "An implementation of flatpak-spawn for Antimony")]
pub struct Cli {
    /// The command to run.
    pub command: String,

    /// No-op
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// FD's to forward to the child
    #[arg(long)]
    pub forward_fd: Option<Vec<i32>>,

    /// Clear the environment of the child
    #[arg(long, default_value_t = false)]
    pub clear_env: bool,

    /// No-op
    #[arg(long, default_value_t = false)]
    pub watch_bus: bool,

    /// Environment variables to pass to the child.
    #[arg(long)]
    pub env: Option<Vec<String>>,

    /// No-op. The children never have network access.
    #[arg(long, default_value_t = false)]
    pub no_network: bool,

    /// Run in a new sandbox
    #[arg(long, default_value_t = false)]
    pub sandbox: bool,

    /// Files to pass ReadWrite
    #[arg(long)]
    pub sandbox_expose: Option<Vec<String>>,

    /// Files to pass ReadOnly
    #[arg(long)]
    pub sandbox_expose_ro: Option<Vec<String>>,

    /// Run in the current environment.
    #[arg(long, default_value_t = false)]
    pub host: bool,

    /// Directory to start the spawned command in.
    #[arg(long)]
    pub directory: Option<String>,

    /// Passthrough args.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}

fn main() -> Result<()> {
    // Set a new AT_HOME in temp.
    unsafe { env::set_var("AT_HOME", "/tmp/antimony-spawn") };

    // If we're running within Antimony itself
    if env::var("USER").is_err() {
        unsafe { env::set_var("USER", "antimony") };
    }

    rayon::ThreadPoolBuilder::new().build_global()?;
    env_logger::init();
    let cli = Cli::parse();

    // Change the current directory.
    if let Some(directory) = cli.directory {
        chdir(Path::new(&directory))?;
    }

    let handle = if cli.sandbox {
        // Construct the relevant arguments.
        let mut args = antimony::cli::run::Args {
            passthrough: cli.passthrough,
            ro: cli.sandbox_expose_ro,
            rw: cli.sandbox_expose,
            env: cli.env,
            libraries: Some(vec!["/usr/lib".to_string()]),
            ..Default::default()
        };

        let mut info = antimony::setup::setup(Cow::Owned(cli.command), &mut args)?;

        // Forward FDs.
        if let Some(fds) = cli.forward_fd {
            for fd in fds {
                info.handle.fd_i(unsafe { OwnedFd::from_raw_fd(fd) });
            }
        }

        // Preserve if want to
        if !cli.clear_env {
            info.handle.preserve_env_i(true);
        }

        // Give the sandbox's library
        #[rustfmt::skip]
        info.handle.args_i([
            "--ro-bind", "/etc", "/etc",
            "--ro-bind", "/usr/share", "/usr/share",
        ])?;

        // Post and spawn.
        info.handle.arg_i(info.profile.app_path(&info.name))?;
        info.handle.args_i(info.post)?;
        info.handle.spawn()?
    } else {
        // If --host, or no --sandbox, just spawn the command.
        let mut handle = Spawner::new(cli.command);
        if let Some(passthrough) = cli.passthrough {
            handle.args_i(passthrough)?;
        }
        if let Some(fds) = cli.forward_fd {
            for fd in fds {
                handle.fd_i(unsafe { OwnedFd::from_raw_fd(fd) });
            }
        }
        if !cli.clear_env {
            handle.preserve_env_i(true);
        }
        handle.spawn()?
    };

    // Hook the signals.
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    // Wait until either the child dies, or we are signaled to terminate.
    while !term.load(Ordering::Relaxed) && handle.alive()? {
        sleep(Duration::from_millis(100));
    }

    Ok(())
}

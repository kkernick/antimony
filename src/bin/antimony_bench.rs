//! For internal use only. Benchmark Antimony, with support for checking out
//! earlier versions.
//!
//! Note that early versions did not support non system installations very well,
//! and thus may fail when running in this benchmark, as it is only run
//! in user-mode. You can manually checkout the repo at that particular point,
//! and run the benchmarker at that iteration--it should work.
//!
//! You can also use the `--seccomp` mode, which uses a SECCOMP notifier filter to
//! to fix incompatibility in older versions (Note all benchmarks will fail, and
//! hyperfine will use `--ignore-failure`. This failure, however, occurs right on
//! sandbox init--so it isn't a premature exit per-say)
#![allow(unused_crate_dependencies)]

use antimony::{
    fab::lib::mount_roots,
    shared::{self, create_parent, env::HOME_PATH, stream::receive_fd, syscalls},
};
use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::socket::{self, ControlMessage, MsgFlags},
    unistd::chdir,
};
use seccomp::{action::Action, filter::Filter, notify::Pair, syscall::Syscall};
use signal_hook::{consts, flag};
use spawn::Spawner;
use std::{
    borrow::Cow,
    env::{self, current_exe},
    fs,
    io::IoSlice,
    iter,
    os::{
        fd::{AsFd, AsRawFd, IntoRawFd, OwnedFd},
        unix::{
            fs::symlink,
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    thread::{self, sleep},
    time::Duration,
};

#[derive(Parser)]
#[command(name = "Antimony-Bench")]
#[command(version)]
#[command(about = "A Utility for Benchmarking Antimony, using Hyperfine")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Run the benchmarker
    Run(RunArgs),

    /// Setup the namespace needed to benchmark
    Setup(SetupArgs),

    /// Run the bencher as a SECCOMP Companion.
    Monitor(MonitorArgs),
}

#[derive(Hash, PartialEq, Eq, Copy, Clone, ValueEnum)]
pub enum Benchmark {
    /// Run the profile with no cache
    Cold,

    /// Run the profile with a cache.
    Hot,

    /// This benchmark performs a refresh of all system integrated profiles (So the ones specified on the command line
    /// are not used). This is used to evaluate the shared cache of profiles, namely:
    ///     1.  Binary and library caches are shared. If one QT6 application scans /usr/lib/qt6, subsequent ones can use
    ///         those definitions for free. Similarly, parsed binaries are cached and shared.
    ///     2.  Non-SetUID installations created a shared folder for SOF/Bin. Once a single profile pulls in a resource
    ///         through a copy, subsequent requests are as fast as setuid, since they both just use hard-links.
    Refresh,
}

#[derive(clap::Args)]
pub struct SetupArgs {
    /// The profiles to benchmark. Defaults to integrated profiles.
    #[arg(value_delimiter = ' ', num_args = 1.., value_hint = ValueHint::CommandName)]
    pub profiles: Vec<String>,

    /// Checkout a specific state of the git tree, such as tags/1.0.0, or a commit ID.
    #[arg(long)]
    pub checkout: Option<String>,

    /// A recipe to build antimony with, and benchmark that artifact.
    #[arg(long)]
    pub recipe: String,

    /// Use a SECCOMP Notifier to spoof linkat for Antimony < 2.4.0. This will cause
    /// all benchmark attempts to fail, hence hyperfine is run with ignore-failure.
    #[arg(long, default_value_t = false)]
    pub seccomp: bool,

    /// Additional commands to pass to `antimony_builder`
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub builder_args: Option<Vec<String>>,

    /// Arguments to pass to the profile application.
    #[arg(last = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}

#[derive(clap::Args)]
pub struct RunArgs {
    /// The profiles to benchmark. Defaults to integrated profiles.
    #[arg(value_delimiter = ' ', num_args = 1.., value_hint = ValueHint::CommandName)]
    pub profiles: Vec<String>,

    /// The maximum amount of times hyperfine should run the profile.
    #[arg(long)]
    pub runs: Option<u64>,

    /// Ensure at least this many runs are performed.
    #[arg(long)]
    pub min: Option<u64>,

    #[arg(long, default_value_t = false)]
    pub output: bool,

    /// What benchmarks to run. By default, all
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub bench: Option<Vec<Benchmark>>,

    /// Additional commands to pass to antimony
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub antimony_args: Option<Vec<String>>,

    /// Additional commands to pass to hyperfine
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub hyperfine_args: Option<Vec<String>>,

    /// If we're running under SECCOMP
    #[arg(long, default_value_t = false)]
    pub seccomp: bool,

    /// Manually delete the `CACHE_DIR`  instead of using `antimony refresh`
    #[arg(long, default_value_t = false)]
    pub manual_refresh: bool,
}

#[derive(clap::Args)]
pub struct MonitorArgs {
    pub instance: String,
}

/// The Benchmark Notifier
///
/// Earlier versions of Antimony do not have a fallback mechanism should hardlinking to the SOF fail.
/// To fix this, `antimony_bench` install a SECCOMP Notify Filter that captures the `linkat` syscall
/// and performs a manual copy instead.
///
/// This is how newer implementations function, and ensure that even the first version can be properly
/// benchmarked.
pub struct Notifier {
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
    #[must_use]
    pub const fn new(path: PathBuf, name: String) -> Self {
        Self {
            path,
            name,
            stream: None,
        }
    }
}

#[allow(clippy::absolute_paths)]
impl seccomp::notify::Notifier for Notifier {
    fn exempt(&self) -> Vec<(Action, Syscall)> {
        vec![]
    }

    /// Setup the `UnixStream`. We wait for the Monitor to setup the socket.
    fn prepare(&mut self) -> Result<(), String> {
        let res = {
            while !self.path.exists() {
                sleep(Duration::from_millis(10));
            }
            match UnixStream::connect(&self.path) {
                Ok(stream) => {
                    self.stream = Some(stream);
                    Ok(())
                }
                Err(e) => Err(format!("Failed to connect to stream: {e}")),
            }
        };
        match res {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("Failed to switch user: {e}")),
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

/// Setup the environment needed to benchmark legacy versions.
fn setup(cli: SetupArgs) -> Result<()> {
    let root = Spawner::new("git")?
        .args(["rev-parse", "--show-toplevel"])
        .output(spawn::StreamMode::Pipe)
        .spawn()?
        .output_all()?;
    let root = root.strip_suffix('\n').unwrap_or(&root);
    chdir(root)?;

    let term = Arc::new(AtomicBool::new(false));
    flag::register(consts::SIGINT, Arc::clone(&term))?;

    if let Some(checkout) = &cli.checkout {
        Spawner::new("git")?.arg("stash").spawn()?.wait()?;

        // Checkout the desired state, but only for code and Cargo.
        Spawner::new("git")?
            .args(["checkout", checkout])
            .spawn()?
            .wait()?;

        // Reset to the original state
        Spawner::new("git")?
            .args(["reset", "--hard"])
            .spawn()?
            .wait()?;
    }

    let target_dir = env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| format!("{root}/target"));

    || -> Result<()> {
        #[rustfmt::skip]
        let handle = Spawner::new("bwrap")?.args([
            "--new-session", "--die-with-parent",
            "--proc", "/proc",
            "--dev", "/dev",
            "--dev", "/dev/dri",
            "--bind", "/tmp", "/tmp",

            "--bind", "/sys", "/sys",
            "--bind", "/run", "/run",
            "--bind", "/usr/bin", "/usr/bin",
            "--bind", "/etc", "/etc",

            "--symlink", "/usr/bin", "/bin",
            "--dir", "/usr/sbin",
            "--symlink", "/usr/sbin", "/sbin",

            "--ro-bind", &format!("{root}/config"), "/usr/share/antimony/config",

            // Compat symlinks. Older versions expected profiles/features to be in AT_HOME directly.
            "--symlink", "/usr/share/antimony/config/profiles", "/usr/share/antimony/profiles",
            "--symlink", "/usr/share/antimony/config/features", "/usr/share/antimony/features",

            "--ro-bind", &env::current_exe()?.to_string_lossy(), "/usr/sbin/antimony_bench",
        ]).preserve_env(true).new_privileges(true).env("PATH", "/usr/sbin:/usr/bin").env("NOTIFY", "none").env("AT_LANDLOCK", "false");

        let notify_socket = temp::Builder::new().create::<temp::Directory>()?;
        let _monitor = if cli.seccomp {
            let mut filter = Filter::new(Action::Allow)?;
            filter.add_rule(Action::Notify, Syscall::from_name("linkat")?)?;

            filter.set_notifier(Notifier::new(
                notify_socket.full().join("monitor"),
                String::from("antimony_bench"),
            ));
            handle.seccomp_i(filter);
            Some(
                Spawner::abs(current_exe()?.to_string_lossy())
                    .args(["monitor", &notify_socket.full().to_string_lossy()])
                    .output(spawn::StreamMode::Log(log::Level::Info))
                    .spawn()?,
            )
        } else {
            None
        };

        println!("Building recipe");
        let path = Spawner::abs(format!("{target_dir}/debug/antimony_build"))
            .args(["--recipe", &cli.recipe])
            .args(cli.builder_args.unwrap_or_default())
            .preserve_env(true)
            .output(spawn::StreamMode::Pipe)
            .new_privileges(true)
            .spawn()?
            .output_all()?;
        let path = path.strip_suffix('\n').unwrap_or(&path);
        let antimony: String = path.to_owned() + "/antimony";

        #[rustfmt::skip]
            handle.args_i([
                "--ro-bind", &antimony, "/usr/sbin/antimony",
                "--ro-bind", path, "/usr/share/antimony/utilities",
            ]);

        mount_roots("", &handle)?;

        handle.args_i(["/usr/sbin/antimony_bench", "run"]);
        handle.args_i(cli.profiles);
        handle.args_i(cli.passthrough.unwrap_or_default());
        if cli.seccomp {
            handle.arg_i("--seccomp");
        }

        handle.spawn()?.wait()?;
        Ok(())
    }()?;

    if cli.checkout.is_some() {
        // Undo the checkout
        Spawner::new("git")?
            .args(["checkout", "main"])
            .spawn()?
            .wait()?;

        // Reset to the original state
        Spawner::new("git")?
            .args(["reset", "--hard"])
            .spawn()?
            .wait()?;

        // Return uncommitted edits.
        Spawner::new("git")?
            .args(["stash", "pop"])
            .spawn()?
            .wait()?;
    }
    Ok(())
}

/// Run the benchmarker.
#[allow(clippy::too_many_lines)]
fn run(cli: RunArgs) -> Result<()> {
    let profiles = &cli.profiles;
    let benchmarks = cli
        .bench
        .clone()
        .unwrap_or_else(|| vec![Benchmark::Cold, Benchmark::Hot]);

    let mut args: Vec<Cow<'static, str>> = vec!["--shell=none"]
        .into_iter()
        .map(Cow::Borrowed)
        .collect();

    if cli.seccomp {
        args.push(Cow::Borrowed("--ignore-failure"));
    }
    if cli.output {
        args.push(Cow::Borrowed("--show-output"));
    }
    if let Some(h_args) = cli.hyperfine_args {
        args.extend(h_args.into_iter().map(Cow::Owned));
    }
    if let Some(runs) = cli.runs {
        args.extend([Cow::Borrowed("-M"), Cow::Owned(runs.to_string())]);
    }
    if let Some(min) = cli.min {
        args.extend([Cow::Borrowed("-m"), Cow::Owned(min.to_string())]);
    }

    #[rustfmt::skip]
    let post: Vec<String> = if cli.seccomp {
       iter::once("--disable-ipc").map(String::from)
       .collect()
    } else {
        ["--sandbox-args='# true !'", "--binaries", "true", "--conflicts", "daemon"].into_iter().map(String::from)
        .collect()
    };

    if benchmarks.contains(&Benchmark::Refresh) {
        let local = HOME_PATH.join(".local").join("bin");
        fs::create_dir_all(&local)?;
        for profile in profiles {
            let p = local.join(profile);
            symlink("/usr/bin/antimony", p)?;
        }
    }

    if benchmarks.contains(&Benchmark::Cold) {
        for profile in profiles {
            let mut command: Vec<String> = [
                "antimony",
                if cli.manual_refresh { "run" } else { "refresh" },
                profile,
            ]
            .into_iter()
            .map(String::from)
            .collect();
            command.extend(post.iter().cloned());

            if let Some(add) = &cli.antimony_args {
                command.extend(add.clone());
            }
            #[rustfmt::skip]
            let handle = Spawner::new("hyperfine")?
                .args([
                    "--command-name", &format!("Cold {profile}"),
                    "--warmup", "1",
                ]);

            if cli.manual_refresh {
                handle.args_i(["--prepare", "rm -fr /usr/share/antimony/cache"]);
            }

            handle
                .args(args.clone())
                .arg(command.join(" "))
                .preserve_env(true)
                .new_privileges(true)
                .spawn()?
                .wait()?;
            thread::sleep(Duration::from_millis(100));
        }
    }

    if benchmarks.contains(&Benchmark::Hot) {
        for profile in profiles {
            let mut command: Vec<String> = ["antimony", "run", profile]
                .into_iter()
                .map(String::from)
                .collect();
            command.extend(post.iter().cloned());

            if let Some(add) = &cli.antimony_args {
                command.extend(add.clone());
            }
            Spawner::new("hyperfine")?
                .args(["--command-name", &format!("Hot {profile}"), "--warmup", "1"])
                .args(args.clone())
                .arg(command.join(" "))
                .preserve_env(true)
                .new_privileges(true)
                .spawn()?
                .wait()?;
        }
        thread::sleep(Duration::from_millis(100));
    }

    if benchmarks.contains(&Benchmark::Refresh) {
        Spawner::new("hyperfine")?
            .args(["--command-name", "System Refresh", "--warmup", "1"])
            .args(args)
            .arg("antimony refresh")
            .preserve_env(true)
            .new_privileges(true)
            .spawn()?
            .wait()?;
    }
    Ok(())
}

/// Run the benchmarker under a SECCOMP filter to ignore linkat
///
/// Older versions of antimony bail of linkat fails, which it will unless we're running as the system user.
#[allow(
    clippy::unwrap_used,
    reason = "If your kernel doesn't have linkat you cannot use Antimony"
)]
fn monitor(cli: MonitorArgs) -> Result<()> {
    let monitor_path = PathBuf::from(cli.instance).join("monitor");
    create_parent(monitor_path.as_path())?;
    let listener = UnixListener::bind(&monitor_path)?;
    let linkat = syscalls::get_num("linkat").unwrap();

    loop {
        if let Some((fd, name)) = receive_fd(&listener)? {
            println!("New connection established with {name}!");

            let mut poll_fd = [PollFd::new(fd.as_fd(), PollFlags::POLLIN)];
            loop {
                let res = match poll(&mut poll_fd, PollTimeout::from(100u8)) {
                    Ok(_) => {
                        if let Some(revents) = poll_fd[0].revents()
                            && (revents.contains(PollFlags::POLLHUP)
                                || revents.contains(PollFlags::POLLERR))
                        {
                            Err(anyhow!("Peer closed"))
                        } else {
                            Ok(Some(()))
                        }
                    }
                    Err(Errno::EINTR | Errno::EAGAIN) => Ok(None),
                    Err(e) => Err(anyhow!("Poll failed: {e}. Aborting!")),
                };

                match res {
                    Ok(option) => {
                        if option.is_none() {
                            continue;
                        }
                    }
                    Err(e) => {
                        println!("{e}");
                        return Err(e);
                    }
                }

                // New pair for each loop, since we don't want to mediate access.
                let pair = Pair::new()?;

                match pair.recv(fd.as_raw_fd()) {
                    Ok(Some(())) => {
                        let raw = fd.as_raw_fd();

                        // Spawn a handler.
                        rayon::spawn(move || {
                            // Reply to the thread. Our handler just gets the name of the executable,
                            // resolves the syscall name, and permits the request.
                            let result = pair.reply(raw, |req, resp| {
                                // Get the binary name
                                let pid = req.pid;
                                let exe_path = match fs::read_link(format!("/proc/{pid}/exe")) {
                                    Ok(path) => Some(path),
                                    Err(e) => {
                                        println!("Invalid exe at PID {pid}: {e}");
                                        None
                                    }
                                };

                                let call = req.data.nr;

                                resp.val = 0;
                                resp.error = 0;
                                resp.flags = 1;

                                if let Some(exe) = exe_path
                                    && (exe.ends_with("bwrap") || exe.ends_with("hyperfine"))
                                    && call == linkat
                                {
                                    return;
                                }

                                if call == linkat {
                                    resp.flags = 0;
                                }
                            });

                            if let Err(e) = result {
                                println!("Failed to reply: {e}");
                            }
                        });
                    }
                    Ok(None) => continue,
                    Err(e) => {
                        println!("Fatal error: {e}");
                        break;
                    }
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;
    match cli.command {
        Command::Run(args) => run(args),
        Command::Setup(args) => setup(args),
        Command::Monitor(args) => monitor(args),
    }
}

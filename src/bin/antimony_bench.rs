//! For internal use only. Benchmark Antimony, with support for checking out
//! earlier versions.
//!
//! Note that early versions did not support non system installations very well,
//! and thus may fail when running in this benchmark, as it is only run
//! in user-mode. You can manually checkout the repo at that particular point,
//! and run the benchmarker at that iteration--it should work.
#![allow(unused_crate_dependencies)]

use antimony::{
    fab::lib::mount_roots,
    shared::{self, env::HOME_PATH},
};
use anyhow::Result;
use clap::{Parser, ValueEnum, ValueHint};
use nix::unistd::chdir;
use signal_hook::{consts, flag};
use spawn::Spawner;
use std::{
    borrow::Cow,
    env, fs,
    os::unix::fs::symlink,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::Duration,
};

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

#[derive(Parser, Default)]
#[command(name = "Antimony-Bench")]
#[command(version)]
#[command(about = "A Utility for Benchmarking Antimony, using Hyperfine")]
pub struct Cli {
    /// The profiles to benchmark. Defaults to integrated profiles.
    #[arg(value_delimiter = ' ', num_args = 1.., value_hint = ValueHint::CommandName)]
    pub profiles: Vec<String>,

    /// A recipe to build antimony with, and benchmark that artifact.
    #[arg(long)]
    pub recipe: String,

    /// The maximum amount of times hyperfine should run the profile.
    #[arg(long)]
    pub runs: Option<u64>,

    /// Ensure at least this many runs are performed.
    #[arg(long)]
    pub min: Option<u64>,

    /// Checkout a specific state of the git tree, such as tags/1.0.0, or a commit ID.
    #[arg(long)]
    pub checkout: Option<String>,

    #[arg(long, default_value_t = false)]
    pub output: bool,

    /// What benchmarks to run. By default, all
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub bench: Option<Vec<Benchmark>>,

    /// Additional commands to pass to `antimony_builder`
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub builder_args: Option<Vec<String>>,

    /// Additional commands to pass to antimony
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub antimony_args: Option<Vec<String>>,

    /// Additional commands to pass to hyperfine
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub hyperfine_args: Option<Vec<String>>,

    /// Internal flag
    #[arg(long, hide = true)]
    pub run: bool,
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let cli = Cli::parse();
    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;
    let profiles = &cli.profiles;

    if cli.run {
        let benchmarks = cli
            .bench
            .clone()
            .unwrap_or_else(|| vec![Benchmark::Cold, Benchmark::Hot]);

        let mut args: Vec<Cow<'static, str>> = vec!["--shell=none", "--time-unit=millisecond"]
            .into_iter()
            .map(Cow::Borrowed)
            .collect();

        #[rustfmt::skip]
        let sleep: Vec<String> = [
            "--sandbox-args='# true !'",
            "--binaries", "true",
            "--conflicts", "daemon"
        ]
        .into_iter()
        .map(String::from)
        .collect();

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
                let mut command: Vec<String> = ["antimony", "refresh", profile, "--hard", "--"]
                    .into_iter()
                    .map(String::from)
                    .collect();
                command.extend(sleep.iter().cloned());

                if let Some(add) = &cli.antimony_args {
                    command.push("--".to_owned());
                    command.extend(add.clone());
                }
                #[rustfmt::skip]
                Spawner::new("hyperfine")?
                    .args([
                        "--command-name", &format!("Cold {profile}"),
                        "--warmup", "1",
                    ])
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
                command.extend(sleep.iter().cloned());

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
    } else {
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
                "--bind", "/tmp", "/tmp",

                "--bind", "/sys", "/sys",
                "--bind", "/run", "/run",
                "--bind", "/usr/bin", "/usr/bin",
                "--bind", "/etc", "/etc",

                "--symlink", "/usr/bin", "/bin",
                "--dir", "/usr/sbin",
                "--symlink", "/usr/sbin", "/sbin",

                "--ro-bind", &format!("{root}/config"), "/usr/share/antimony/config",
                "--ro-bind", &env::current_exe()?.to_string_lossy(), "/usr/sbin/antimony_bench",
            ]).preserve_env(true).new_privileges(true).env("PATH", "/usr/sbin:/usr/bin");

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

            let args: Vec<_> = env::args().collect();
            handle.arg_i("/usr/sbin/antimony_bench");
            handle.args_i(&args[1..]);
            handle.arg("--run").spawn()?.wait()?;
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
    }
    Ok(())
}

#![allow(unused_crate_dependencies)]
//! For internal use only. Benchmark Antimony, with support for checking out
//! earlier versions.
//!
//! Note that early versions did not support non system installations very well,
//! and thus may fail when running in this benchmark, as it is only run
//! in user-mode. You can manually checkout the repo at that particular point,
//! and run the benchmarker at that iteration--it should work.

use antimony::{cli::refresh::installed_profiles, shared};
use anyhow::{Result, anyhow};
use clap::{Parser, ValueEnum, ValueHint};
use nix::unistd::chdir;
use signal_hook::{consts, flag};
use spawn::Spawner;
use std::{
    borrow::Cow,
    env,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Hash, Debug, PartialEq, Eq, Copy, Clone, ValueEnum)]
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

#[derive(Parser, Debug, Default)]
#[command(name = "Antimony-Bench")]
#[command(version)]
#[command(about = "A Utility for Benchmarking Antimony, using Hyperfine")]
pub struct Cli {
    /// The profiles to benchmark. Defaults to integrated profiles.
    #[arg(value_delimiter = ' ', num_args = 1.., value_hint = ValueHint::CommandName)]
    pub profiles: Option<Vec<String>>,

    /// A recipe to build antimony with, and benchmark that artifact. Defaults to using
    /// wherever `antimony` resolves to, and doesn't build.
    #[arg(long)]
    pub recipe: Option<String>,

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

    /// An optional temperature sensor to monitor.
    #[arg(long)]
    pub temp_sensor: Option<String>,

    /// An optional temperature to wait for cooldown. Usually, this has a precision of a thousandth, so 65000 = 65.0
    #[arg(long)]
    pub temp: Option<u64>,

    /// What benchmarks to run. By default, all
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub bench: Option<Vec<Benchmark>>,

    /// Give Antimony setuid like in a system installation.
    #[arg(long, default_value_t = false)]
    pub system: bool,

    /// How long to sleep for. Defaults to 1 second.
    #[arg(long)]
    pub sleep: Option<u32>,

    /// Where to point `AT_HOME`. If not set, defaults to the repository root.
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub home: Option<String>,

    /// Additional commands to pass to `antimony_builder`
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub builder_args: Option<Vec<String>>,

    /// Additional commands to pass to antimony
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub antimony_args: Option<Vec<String>>,

    /// Additional commands to pass to hyperfine
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub hyperfine_args: Option<Vec<String>>,
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let cli = Cli::parse();
    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;

    let profiles = match cli.profiles {
        Some(profiles) => profiles,
        None => installed_profiles()?,
    };

    let root = Spawner::new("git")?
        .args(["rev-parse", "--show-toplevel"])
        .output(spawn::StreamMode::Pipe)
        .spawn()?
        .output_all()?;
    let root = root.strip_suffix('\n').unwrap_or(&root);
    chdir(root)?;

    if cli.recipe.is_some() {
        // Set AT_HOME to our current config.
        if !cli.system {
            unsafe { env::set_var("AT_HOME", cli.home.unwrap_or_else(|| root.to_owned())) }
            unsafe { env::set_var("AT_FORCE_TEMP", "1") }
        }
    }

    let term = Arc::new(AtomicBool::new(false));
    flag::register(consts::SIGINT, Arc::clone(&term))?;

    if let Some(checkout) = &cli.checkout {
        // We need to impose a minimum to ensure --sandbox-args is supported. This also allows us to use --hard without
        // recourse. Unfortunately, there's no real way to benchmark the older versions with how we benchmark now,
        // since we're not effectively *always* running under the "real" mode.
        //
        // What is the point of benchmarking if the test is not real in the first place?
        match checkout.strip_prefix("tags/") {
            Some(version) => {
                let split = version
                    .split('.')
                    .filter_map(|f| f.parse::<u32>().ok())
                    .collect::<Vec<_>>();
                if (split[0] < 2) || (split[0] == 2 && split[1] < 4) {
                    return Err(anyhow!(
                        "This version of the benchmark requires Antimony 2.4.0. If you need to benchmark older versions, you can checkout the repo at 4.2.1, but note output between these versions are not comparable."
                    ));
                }
            }
            None => {
                return Err(anyhow!("Checkout only works with tagged versions!"));
            }
        }

        // Stash our working edits
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

    let antimony = || -> Result<String> {
        let benchmarks = cli
            .bench
            .unwrap_or_else(|| vec![Benchmark::Cold, Benchmark::Hot]);

        let mut args: Vec<Cow<'static, str>> = vec!["--shell=none", "--time-unit=microsecond"]
            .into_iter()
            .map(Cow::Borrowed)
            .collect();

        let timeout = cli.sleep.unwrap_or(1);
        let sleep: Vec<String> = [
            &format!("--sandbox-args='# sleep {timeout} !'"),
            "--binaries",
            "sleep",
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

        let target_dir = env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| format!("{root}/target"));

        let antimony = if let Some(recipe) = &cli.recipe {
            println!("Building recipe");
            let antimony = Spawner::abs(format!("{target_dir}/debug/antimony_build"))
                .args(["--recipe", recipe])
                .args(cli.builder_args.unwrap_or_default())
                .preserve_env(true)
                .output(spawn::StreamMode::Pipe)
                .new_privileges(true)
                .spawn()?
                .output_all()?;
            let antimony: String =
                antimony.strip_suffix('\n').unwrap_or(&antimony).to_owned() + "/antimony";
            if cli.system {
                Spawner::abs("/usr/bin/sudo")
                    .args(["chown", "antimony:antimony", &antimony])
                    .new_privileges(true)
                    .spawn()?
                    .wait()?;

                Spawner::abs("/usr/bin/sudo")
                    .args(["chmod", "ug+s", &antimony])
                    .new_privileges(true)
                    .spawn()?
                    .wait()?;

                Spawner::abs("/usr/bin/sudo")
                    .args([
                        "mount",
                        "--bind",
                        &format!("{root}/config"),
                        "/usr/share/antimony/config",
                    ])
                    .new_privileges(true)
                    .spawn()?
                    .wait()?;

                if !Path::new("/usr/share/antimony/profiles").exists() {
                    Spawner::abs("/usr/bin/sudo")
                        .args([
                            "ln",
                            "-s",
                            "/usr/share/antimony/config/profiles",
                            "/usr/share/antimony/profiles",
                        ])
                        .new_privileges(true)
                        .spawn()?
                        .wait()?;
                }

                if !Path::new("/usr/share/antimony/features").exists() {
                    Spawner::abs("/usr/bin/sudo")
                        .args([
                            "ln",
                            "-s",
                            "/usr/share/antimony/config/features",
                            "/usr/share/antimony/features",
                        ])
                        .new_privileges(true)
                        .spawn()?
                        .wait()?;
                }
            }
            antimony
        } else {
            "antimony".to_owned()
        };

        println!("Using: {antimony}");

        for profile in &profiles {
            if benchmarks.contains(&Benchmark::Cold) {
                let mut command: Vec<String> = [&antimony, "refresh", profile, "--hard", "--"]
                    .into_iter()
                    .map(String::from)
                    .collect();
                command.extend(sleep.iter().cloned());

                if let Some(add) = &cli.antimony_args {
                    command.push("--".to_owned());
                    command.extend(add.clone());
                }
                Spawner::new("hyperfine")?
                    .args([
                        "--command-name",
                        &format!("Cold {profile}"),
                        "--warmup",
                        "1",
                    ])
                    .args(args.clone())
                    .arg(command.join(" "))
                    .preserve_env(true)
                    .new_privileges(true)
                    .spawn()?
                    .wait()?;
            }

            if benchmarks.contains(&Benchmark::Hot) {
                let mut command: Vec<String> = [&antimony, "run", profile]
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
        }

        if benchmarks.contains(&Benchmark::Refresh) {
            Spawner::new("hyperfine")?
                .args(["--command-name", "System Refresh", "--warmup", "1"])
                .args(args)
                .arg(format!("{antimony} refresh"))
                .preserve_env(true)
                .new_privileges(true)
                .spawn()?
                .wait()?;
        }
        Ok(antimony)
    }();

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

    if cli.system {
        Spawner::abs("/usr/bin/sudo")
            .args(["umount", "/usr/share/antimony/config"])
            .new_privileges(true)
            .spawn()?
            .wait()?;

        if Path::new("/usr/share/antimony/profiles").is_symlink() {
            Spawner::abs("/usr/bin/sudo")
                .args(["rm", "/usr/share/antimony/profiles"])
                .new_privileges(true)
                .spawn()?
                .wait()?;
        }

        if Path::new("/usr/share/antimony/features").is_symlink() {
            Spawner::abs("/usr/bin/sudo")
                .args(["rm", "/usr/share/antimony/features"])
                .new_privileges(true)
                .spawn()?
                .wait()?;
        }
    }

    let antimony = antimony?;
    if cli.system {
        Spawner::abs("/usr/bin/sudo")
            .args(["rm", &antimony])
            .new_privileges(true)
            .spawn()?
            .wait()?;
    }
    Ok(())
}

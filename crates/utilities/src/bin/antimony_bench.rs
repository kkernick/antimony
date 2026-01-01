use std::{
    borrow::Cow,
    env,
    fs::read_to_string,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    thread::sleep,
    time::Duration,
};

use antimony::shared;
use anyhow::Result;
use clap::{Parser, ValueEnum};
use dialoguer::Input;
use nix::unistd::chdir;
use spawn::Spawner;

#[derive(Hash, Debug, PartialEq, Eq, Copy, Clone, ValueEnum)]
pub enum Benchmark {
    /// Run the profile with no cache
    Cold,

    /// Run the profile with a cache.
    Hot,

    /// Run the profile with a cache, but with the real execution pipeline.
    /// --dry disables some parts of sandbox setup, as its primary purpose is just to populate caches.
    /// This means things like creating the proxy and waiting for it, creating and applying SECCOMP filters, and
    /// others parts are skipped over. This is important for ensuring refresh is fast, but means that the Hot
    /// benchmark does not reflect the time it takes for end users to get their application from startup.
    ///
    /// This benchmark runs the sandbox as usual, but passes the dry feature, which simply spawns a shell in the
    /// environment, then immediately exits.
    Real,

    /// This benchmark performs a refresh of all system integrated profiles (So the ones specified on the command line
    /// are not used). This is used to evaluate the shared cache of profiles, namely:
    ///     1.  Binary and library caches are shared. If one QT6 application scans /usr/lib/qt6, subsequent ones can use
    ///         those definitions for free. Similarly, parsed binaries are cached and shared.
    ///     2.  Non-SetUID installations created a shared folder for SOF/Bin. Once a single profile pulls in a resource
    ///         through a copy, subsequent requests are as fast as SetUID, since they both just use hard-links.
    Refresh,
}

#[derive(Parser, Debug, Default)]
#[command(name = "Antimony-Bench")]
#[command(version)]
#[command(about = "A Utility for Benchmarking Antimony, using Hyperfine")]
pub struct Cli {
    /// The profiles to benchmark.
    #[arg(value_delimiter = ' ', num_args = 1..)]
    pub profiles: Vec<String>,

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

    /// Pause the benchmark after each invocation to allow inspection of the cache.
    #[arg(long, default_value_t = false)]
    pub inspect: bool,

    /// Additional commands to pass to antimony
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub antimony_args: Option<Vec<String>>,

    /// Additional commands to pass to hyperfine
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub hyperfine_args: Option<Vec<String>>,
}

fn cooldown(sensor: &Option<String>, target: &Option<u64>, inspect: bool) -> Result<()> {
    if inspect {
        let prompt = Input::<String>::new()
            .allow_empty(true)
            .with_prompt("The benchmarker has been paused to inspect the profile cache. Press any key to continue. Type quit to abort early").report(false).interact()?;

        if prompt.to_lowercase() == "quit" {
            return Err(anyhow::anyhow!("User aborted"));
        }
    }

    if let Some(sensor) = &sensor
        && Path::new(sensor).exists()
        && let Some(target) = target
    {
        println!("Waiting for system to cool down");

        loop {
            let temp = read_to_string(sensor)?;
            if let Some(first) = temp.lines().next()
                && let Ok(degrees) = first.parse::<u64>()
            {
                if degrees <= *target {
                    break;
                }
                sleep(Duration::from_secs(1));
            } else {
                eprintln!("Could not parse sensor'");
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;

    let root = Spawner::new("git")?
        .args(["rev-parse", "--show-toplevel"])?
        .output(spawn::StreamMode::Pipe)
        .spawn()?
        .output_all()?;
    let root = &root[..root.len() - 1];
    chdir(root)?;

    let _cache = if cli.recipe.is_some() {
        // Set AT_HOME to our current config.
        unsafe { env::set_var("AT_HOME", format!("{root}/config")) }
        unsafe { env::set_var("AT_FORCE_TMP", "1") }

        Some(
            temp::Builder::new()
                .name("cache")
                .within(Path::new(root).join("config"))
                .create::<temp::Directory>()?,
        )
    } else {
        unsafe { env::set_var("AT_SYSTEM_MODE", "1") }
        None
    };

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    if let Some(checkout) = &cli.checkout {
        // Stash our working edits
        Spawner::new("git")?.arg("stash")?.spawn()?.wait()?;

        // Checkout the desired state, but only for code and Cargo.
        Spawner::new("git")?
            .args([
                "checkout",
                checkout,
                "src",
                "config",
                "crates",
                "Cargo.toml",
                "Cargo.lock",
            ])?
            .spawn()?
            .wait()?;
    }

    let result = || -> Result<()> {
        let benchmarks =
            cli.bench
                .unwrap_or(vec![Benchmark::Cold, Benchmark::Hot, Benchmark::Real]);

        let mut args: Vec<Cow<'static, str>> = vec!["--shell=none", "--time-unit=millisecond"]
            .into_iter()
            .map(Cow::Borrowed)
            .collect();
        if cli.output {
            args.push(Cow::Borrowed("--show-output"));
        }
        if let Some(h_args) = cli.hyperfine_args {
            args.extend(h_args.into_iter().map(Cow::Owned))
        }
        if let Some(runs) = cli.runs {
            args.extend([Cow::Borrowed("-M"), Cow::Owned(runs.to_string())])
        }
        if let Some(min) = cli.min {
            args.extend([Cow::Borrowed("-m"), Cow::Owned(min.to_string())])
        }

        let antimony = if let Some(recipe) = cli.recipe {
            match recipe.as_str() {
                "pgo" => {
                    Spawner::abs(format!("{root}/pgo"))
                        .preserve_env(true)
                        .spawn()?
                        .wait()?;
                    format!("{root}/target/x86_64-unknown-linux-gnu/release/antimony")
                }
                "bolt" => {
                    Spawner::abs(format!("{root}/bolt"))
                        .preserve_env(true)
                        .spawn()?
                        .wait()?;
                    format!(
                        "{root}/target/x86_64-unknown-linux-gnu/release/antimony-bolt-optimized"
                    )
                }
                recipe if recipe == "release" || recipe == "dev" => {
                    Spawner::new("cargo")?
                        .args(["build", "--profile", recipe])?
                        .preserve_env(true)
                        .spawn()?
                        .wait()?;
                    format!(
                        "{root}/target/{}/antimony",
                        if recipe == "dev" { "debug" } else { &recipe }
                    )
                }
                path => path.to_string(),
            }
        } else {
            "antimony".to_string()
        };

        println!("Using: {antimony}");
        for profile in &cli.profiles {
            cooldown(&cli.temp_sensor, &cli.temp, cli.inspect)?;

            if benchmarks.contains(&Benchmark::Cold) {
                let mut command: Vec<String> = [&antimony, "refresh", profile, "--dry", "--hard"]
                    .into_iter()
                    .map(String::from)
                    .collect();
                if let Some(add) = &cli.antimony_args {
                    command.push("--".to_string());
                    command.extend(add.clone());
                }
                Spawner::new("hyperfine")?
                    .args([
                        "--command-name",
                        &format!("Cold {profile}"),
                        "--warmup",
                        "1",
                    ])?
                    .args(args.clone())?
                    .arg(command.join(" "))?
                    .preserve_env(true)
                    .new_privileges(true)
                    .spawn()?
                    .wait()?;

                cooldown(&cli.temp_sensor, &cli.temp, cli.inspect)?;
            }

            if benchmarks.contains(&Benchmark::Hot) {
                let mut command: Vec<String> = [&antimony, "run", profile, "--dry"]
                    .into_iter()
                    .map(String::from)
                    .collect();
                if let Some(add) = &cli.antimony_args {
                    command.extend(add.clone());
                }
                Spawner::new("hyperfine")?
                    .args(["--command-name", &format!("Hot {profile}"), "--warmup", "1"])?
                    .args(args.clone())?
                    .arg(command.join(" "))?
                    .preserve_env(true)
                    .new_privileges(true)
                    .spawn()?
                    .wait()?;

                cooldown(&cli.temp_sensor, &cli.temp, cli.inspect)?;
            }

            if benchmarks.contains(&Benchmark::Real) {
                let mut command: Vec<String> = [&antimony, "run", profile]
                    .into_iter()
                    .map(String::from)
                    .collect();
                if let Some(add) = &cli.antimony_args {
                    command.extend(add.clone());
                }
                command.push("--features=dry".to_string());

                Spawner::new("hyperfine")?
                    .args([
                        "--command-name",
                        &format!("Real {profile}"),
                        "--warmup",
                        "1",
                    ])?
                    .args(args.clone())?
                    .arg(command.join(" "))?
                    .preserve_env(true)
                    .new_privileges(true)
                    .spawn()?
                    .wait()?;
            }
        }

        if benchmarks.contains(&Benchmark::Refresh) {
            Spawner::new("hyperfine")?
                .args(["--command-name", "System Refresh", "--warmup", "1"])?
                .args(args)?
                .arg(format!("{antimony} refresh"))?
                .preserve_env(true)
                .new_privileges(true)
                .spawn()?
                .wait()?;
        }
        Ok(())
    }();

    if cli.checkout.is_some() {
        // Undo the checkout
        Spawner::new("git")?
            .args([
                "checkout",
                "-",
                "src",
                "config",
                "crates",
                "Cargo.toml",
                "Cargo.lock",
            ])?
            .spawn()?
            .wait()?;

        // Reset to the original state
        Spawner::new("git")?
            .args(["reset", "--hard"])?
            .spawn()?
            .wait()?;

        // Return uncommitted edits.
        Spawner::new("git")?
            .args(["stash", "pop"])?
            .spawn()?
            .wait()?;
    }

    result
}

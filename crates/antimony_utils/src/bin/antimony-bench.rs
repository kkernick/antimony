use std::{
    borrow::Cow,
    env,
    fs::read_to_string,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    thread::sleep,
    time::Duration,
};

use anyhow::Result;
use clap::{Parser, ValueEnum};
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

    /// Additional commands to pass to antimony
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub antimony_args: Option<Vec<String>>,

    /// Additional commands to pass to hyperfine
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub hyperfine_args: Option<Vec<String>>,
}

fn cooldown(sensor: &Option<String>, target: &Option<u64>) -> Result<()> {
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

    let root = Spawner::new("git")
        .args(["rev-parse", "--show-toplevel"])?
        .output(spawn::StreamMode::Pipe)
        .spawn()?
        .output_all()?;
    let root = &root[..root.len() - 1];
    chdir(root)?;

    // Set AT_HOME to our current config.
    unsafe { env::set_var("AT_HOME", format!("{root}/config")) }

    let temp = "/tmp/antimony";
    std::fs::create_dir_all(temp)?;
    symlink(temp, format!("{root}/config/cache"))?;

    let term = Arc::new(AtomicBool::new(false));

    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    if let Some(checkout) = &cli.checkout {
        // Stash our working edits
        Spawner::new("git").arg("stash")?.spawn()?.wait()?;

        // Checkout the desired state, but only for code and Cargo.
        Spawner::new("git")
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

        let antimony = if let Some(recipe) = cli.recipe {
            match recipe.as_str() {
                "pgo" => {
                    Spawner::new(format!("{root}/pgo"))
                        .preserve_env(true)
                        .spawn()?
                        .wait()?;
                    format!("{root}/target/x86_64-unknown-linux-gnu/release/antimony")
                }
                "bolt" => {
                    Spawner::new(format!("{root}/bolt"))
                        .preserve_env(true)
                        .spawn()?
                        .wait()?;
                    format!(
                        "{root}/target/x86_64-unknown-linux-gnu/release/antimony-bolt-optimized"
                    )
                }
                recipe if recipe == "release" || recipe == "dev" => {
                    Spawner::new("cargo")
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
            cooldown(&cli.temp_sensor, &cli.temp)?;

            if benchmarks.contains(&Benchmark::Cold) {
                let mut command: Vec<String> = [&antimony, "refresh", profile, "--dry"]
                    .into_iter()
                    .map(String::from)
                    .collect();
                if let Some(add) = &cli.antimony_args {
                    command.extend(add.clone());
                }
                Spawner::new("hyperfine")
                    .args([
                        "--command-name",
                        &format!("Cold {profile}"),
                        "--warmup",
                        "1",
                    ])?
                    .args(args.clone())?
                    .arg(command.join(" "))?
                    .preserve_env(true)
                    .spawn()?
                    .wait()?;

                cooldown(&cli.temp_sensor, &cli.temp)?;
            }

            if benchmarks.contains(&Benchmark::Hot) {
                let mut command: Vec<String> = [&antimony, "run", profile, "--dry"]
                    .into_iter()
                    .map(String::from)
                    .collect();
                if let Some(add) = &cli.antimony_args {
                    command.extend(add.clone());
                }
                Spawner::new("hyperfine")
                    .args(["--command-name", &format!("Hot {profile}"), "--warmup", "1"])?
                    .args(args.clone())?
                    .arg(command.join(" "))?
                    .preserve_env(true)
                    .spawn()?
                    .wait()?;

                cooldown(&cli.temp_sensor, &cli.temp)?;
            }

            if benchmarks.contains(&Benchmark::Real) {
                let mut command: Vec<String> = [&antimony, "run", profile, "--features=dry"]
                    .into_iter()
                    .map(String::from)
                    .collect();
                if let Some(add) = &cli.antimony_args {
                    command.extend(add.clone());
                }
                Spawner::new("hyperfine")
                    .args([
                        "--command-name",
                        &format!("Real {profile}"),
                        "--warmup",
                        "1",
                    ])?
                    .args(args.clone())?
                    .arg(command.join(" "))?
                    .preserve_env(true)
                    .spawn()?
                    .wait()?;
            }
        }
        Ok(())
    }();

    let cache = PathBuf::from(format!("{root}/config/cache"));
    if cache.exists() {
        std::fs::remove_dir_all(&cache)?;
    }
    std::fs::remove_dir_all(temp)?;

    if cli.checkout.is_some() {
        // Undo the checkout
        Spawner::new("git")
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
        Spawner::new("git")
            .args(["reset", "--hard"])?
            .spawn()?
            .wait()?;

        // Return uncommitted edits.
        Spawner::new("git")
            .args(["stash", "pop"])?
            .spawn()?
            .wait()?;
    }

    result
}

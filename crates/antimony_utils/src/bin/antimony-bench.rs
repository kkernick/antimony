use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    thread::sleep,
    time::Duration,
};

use antimony_utils::set_capabilities;
use anyhow::Result;
use clap::Parser;
use inotify::{Inotify, WatchMask};
use nix::unistd::chdir;
use spawn::Spawner;

#[derive(Parser, Debug, Default)]
#[command(name = "Antimony-Bench")]
#[command(version)]
#[command(about = "A Utility for Benchmarking Antimony, using Hyperfine")]
pub struct Cli {
    /// The profiles to benchmark.
    #[arg(value_delimiter = ' ', num_args = 1..)]
    pub profiles: Vec<String>,

    /// How long to wait between each profile.
    #[arg(long)]
    pub cool: Option<u64>,

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

    /// Use sudo to give the built executable (If any) the capabilities needed to
    /// run in /usr/share/antimony, and make hard-links.
    #[arg(long, default_value_t = false)]
    pub privileged: bool,

    /// Additional commands to pass to antimony
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub antimony_args: Option<Vec<String>>,

    /// Additional commands to pass to hyperfine
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub hyperfine_args: Option<Vec<String>>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = Spawner::new("git")
        .args(["rev-parse", "--show-toplevel"])?
        .output(spawn::StreamMode::Pipe)
        .spawn()?
        .output_all()?;
    let root = &root[..root.len() - 1];
    chdir(root)?;

    let term = Arc::new(AtomicBool::new(false));

    if let Some(checkout) = &cli.checkout {
        signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

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

    let mut args: Vec<String> = vec!["--shell=none", "--time-unit=millisecond"]
        .into_iter()
        .map(String::from)
        .collect();

    if let Some(h_args) = cli.hyperfine_args {
        args.extend(h_args)
    }
    if let Some(runs) = cli.runs {
        args.extend(["-M".to_string(), runs.to_string()])
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
                format!("{root}/target/x86_64-unknown-linux-gnu/release/antimony-bolt-optimized")
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

    let path = PathBuf::from(&antimony);
    let privilege_handle = if cli.privileged && path.exists() {
        let mut inotify = Inotify::init()?;
        inotify
            .watches()
            .add(path.parent().unwrap(), WatchMask::CREATE)?;

        let handle = set_capabilities(root, &path)?;
        let mut buffer = [0; 1024];
        let _ = inotify.read_events_blocking(&mut buffer)?;
        Some(handle)
    } else {
        None
    };

    println!("Using: {antimony}");
    for profile in &cli.profiles {
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
                &format!("Local Refresh {profile}"),
                "--warmup",
                "1",
            ])?
            .args(args.clone())?
            .arg(command.join(" "))?
            .preserve_env(true)
            .spawn()?
            .wait()?;

        let mut command: Vec<String> = [&antimony, "run", profile, "--dry"]
            .into_iter()
            .map(String::from)
            .collect();
        if let Some(add) = &cli.antimony_args {
            command.extend(add.clone());
        }

        Spawner::new("hyperfine")
            .args([
                "--command-name",
                &format!("Cached {profile}"),
                "--warmup",
                "1",
            ])?
            .args(args.clone())?
            .arg(command.join(" "))?
            .preserve_env(true)
            .spawn()?
            .wait()?;

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
                &format!("Cached (Real) {profile}"),
                "--warmup",
                "1",
            ])?
            .args(args.clone())?
            .arg(command.join(" "))?
            .preserve_env(true)
            .spawn()?
            .wait()?;

        if cli.profiles.len() > 1 {
            println!("Waiting for system to cool down");
            sleep(Duration::from_secs(cli.cool.unwrap_or(5)));
        }
    }

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

    if let Some(mut handle) = privilege_handle {
        writeln!(handle)?;
    }

    Ok(())
}

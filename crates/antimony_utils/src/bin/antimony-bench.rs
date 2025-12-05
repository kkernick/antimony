use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
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
    /// The profile to benchmark.
    pub profile: String,

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::init();

    let root = Spawner::new("git")
        .args(["rev-parse", "--show-toplevel"])?
        .output(true)
        .spawn()?
        .output_all()?;
    let root = &root[..root.len() - 1];
    chdir(root)?;

    // Hook SIGTERM and SIGINT
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
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
    Spawner::new("hyperfine")
        .args(["--command-name", &format!("Local Refresh {}", cli.profile)])?
        .args(args.clone())?
        .arg([&antimony, "refresh", &cli.profile, "--dry"].join(" "))?
        .preserve_env(true)
        .spawn()?
        .wait()?;

    Spawner::new("hyperfine")
        .args([
            "--command-name",
            &format!("Cached {}", cli.profile),
            "--warmup",
            "1",
        ])?
        .args(args)?
        .arg([&antimony, "run", &cli.profile, "--dry"].join(" "))?
        .preserve_env(true)
        .spawn()?
        .wait()?;

    if cli.checkout.is_some() {
        // Undo the checkout
        Spawner::new("git")
            .args(["checkout", "-", "src", "config", "Cargo.toml", "Cargo.lock"])?
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

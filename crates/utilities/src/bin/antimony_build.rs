//! Build antimony.
//! This is just a cargo wrapper that uses predefined values.

use antimony::shared::env::HOME_PATH;
use anyhow::Result;
use clap::Parser;
use spawn::{Spawner, StreamMode};
use std::fs;

#[derive(Parser, Debug, Default)]
#[command(name = "Antimony-Build")]
#[command(version)]
#[command(about = "A Utility for Building Antimony")]
pub struct Cli {
    /// A recipe to build antimony with, and benchmark that artifact. Defaults to using
    /// wherever `antimony` resolves to, and doesn't build.
    #[arg(long)]
    pub recipe: String,

    /// Build with cargo nightly.
    #[arg(long, default_value_t = false)]
    pub nightly: bool,

    /// Build with native CPU Flags
    #[arg(long, default_value_t = false)]
    pub native: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    notify::init_error()?;

    let root = Spawner::new("git")?
        .args(["rev-parse", "--show-toplevel"])?
        .output(StreamMode::Pipe)
        .error(StreamMode::Discard)
        .spawn()?
        .output_all()?;
    let root = &root[..root.len() - 1];

    let target_dir = if let Ok(var) = std::env::var("CARGO_TARGET_DIR") {
        var
    } else {
        format!("{root}/target")
    };

    let mut rust_flags = Vec::new();
    if cli.native {
        rust_flags.push("-Ctarget-cpu=native");
    }

    let mut cargo_flags = Vec::new();
    let mut post_flags = Vec::new();
    if cli.nightly {
        cargo_flags.push("+nightly");
        post_flags.push("-Zbuild-std=std,panic_abort")
    }

    let target = "x86_64-unknown-linux-gnu";
    if cli.recipe == "pgo" {
        eprintln!("Compiling instrumented binary");
        Spawner::new("cargo")?
            .env("RUSTFLAGS", rust_flags.join(" "))?
            .new_privileges(true)
            .preserve_env(true)
            .env("RUST_LOG", "")?
            .error(StreamMode::Log(log::Level::Warn))
            .output(StreamMode::Log(log::Level::Info))
            .args(cargo_flags.clone())?
            .args(["pgo", "build", "--", "--target", target])?
            .args(post_flags.clone())?
            .spawn()?
            .wait()?;

        let instrumented = format!("{target_dir}/{target}/release/antimony");
        eprintln!("Using {instrumented}");

        eprintln!("Performing Refresh Profiling");
        Spawner::abs(&instrumented)
            .args(["refresh", "--hard"])?
            .new_privileges(true)
            .preserve_env(true)
            .env(
                "LLVM_PROFILE_FILE",
                format!("{target_dir}/pgo-profiles/antimony_%m_%p.profraw"),
            )?
            .error(StreamMode::Log(log::Level::Warn))
            .output(StreamMode::Log(log::Level::Info))
            .spawn()?
            .wait()?;

        let profiles = fs::read_dir(HOME_PATH.join(".local").join("bin"))?.filter_map(|e| {
            if let Ok(entry) = e
                && let Ok(link) = entry.path().read_link()
                && &link == "/usr/bin/antimony"
            {
                Some(entry.file_name().to_string_lossy().into_owned())
            } else {
                None
            }
        });

        eprintln!("Peforming Benchmark Profiling");
        Spawner::abs(format!("{root}/scripts/cargo-bencher"))
            .args(profiles)?
            .args(["--recipe", &instrumented, "--bench", "real"])?
            .new_privileges(true)
            .preserve_env(true)
            .env(
                "LLVM_PROFILE_FILE",
                format!("{target_dir}/pgo-profiles/antimony_%m_%p.profraw"),
            )?
            .error(StreamMode::Log(log::Level::Warn))
            .output(StreamMode::Log(log::Level::Info))
            .spawn()?
            .wait()?;

        eprintln!("Compiling Final Binary");
        Spawner::new("cargo")?
            .env("RUSTFLAGS", rust_flags.join(" "))?
            .new_privileges(true)
            .preserve_env(true)
            .env("RUST_LOG", "")?
            .error(StreamMode::Log(log::Level::Warn))
            .output(StreamMode::Log(log::Level::Info))
            .args(cargo_flags)?
            .args([
                "pgo",
                "optimize",
                "build",
                "--",
                "--target",
                target,
                "--workspace",
            ])?
            .args(post_flags)?
            .spawn()?
            .wait()?;
    } else {
        eprintln!("Compiling Binary");
        Spawner::new("cargo")?
            .env("RUSTFLAGS", rust_flags.join(" "))?
            .new_privileges(true)
            .preserve_env(true)
            .args(cargo_flags)?
            .arg("build")?
            .args(["--target", target, "--workspace", "--profile", &cli.recipe])?
            .args(post_flags)?
            .error(StreamMode::Log(log::Level::Warn))
            .output(StreamMode::Log(log::Level::Info))
            .spawn()?
            .wait()?;
    }

    println!(
        "{target_dir}/{target}/{}",
        match cli.recipe.as_str() {
            "dev" => "debug",
            "pgo" => "release",
            recipe => recipe,
        }
    );

    Ok(())
}

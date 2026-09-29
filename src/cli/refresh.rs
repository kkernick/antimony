//! Refresh installed profiles.
#![allow(clippy::missing_errors_doc, dead_code)]

use crate::{
    cli::run,
    shared::{
        Set,
        env::{CACHE_DIR, HOME_PATH, RUNTIME_DIR},
        profile::{self, Profile},
        remove_directory,
        store::{self, Object, mem},
        syscalls,
    },
};
use anyhow::Result;
use clap::ValueHint;
use console::{Term, style};
use log::info;
use std::fs;
use user::as_effective;

#[derive(clap::Args, Default)]
pub struct Args {
    /// Run a profile, but refresh its contents.
    /// If not defined, all profiles are refreshed, but nothing is run.
    #[arg(value_hint = ValueHint::CommandName)]
    pub profile: Option<String>,

    /// Just delete the cache, don't repopulate.
    #[arg(short, long)]
    pub dry: bool,

    /// Delete the entire Cache directory. Will break any instance currently running!
    #[arg(long)]
    pub hard: bool,

    /// Run arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,

    /// Refresh the SECCOMP database by removing nonexistent binaries and optimizing.
    /// Overrides PROFILE, --hard and --dry
    #[arg(short, long)]
    pub seccomp: bool,
}
impl Args {
    /// Refresh a profile and all it's configurations
    #[allow(clippy::needless_pass_by_value)]
    fn refresh(name: &str, config: Option<String>, terminal: &Term) -> Result<()> {
        let args = run::Args {
            profile: name.to_owned(),
            dry: true,
            refresh: true,
            config: config.clone(),
            ..Default::default()
        };
        let result = args.refresh();
        terminal.clear_last_lines(1)?;
        if let Err(e) = result {
            eprintln!("{}", style(format!("Failed to refresh {name}: {e}")).red());
        }
        if config.is_none()
            && let Ok(profile) = store::load::<Profile, profile::Error>(name, Object::Profile)
            && let Some(configuration) = profile.configuration
        {
            for (conf, _) in configuration {
                println!("Refreshing {} ({conf})", style(&name).bold());
                Self::refresh(name, Some(conf), terminal)?;
            }
        }
        Ok(())
    }

    pub fn run(self) -> Result<()> {
        if self.seccomp {
            syscalls::clean_database()?;
            syscalls::CONNECTION.with_borrow_mut(|conn| -> Result<()> {
                conn.execute("VACUUM;", [])?;
                conn.execute("ANALYZE;", [])?;
                info!("Optimized!");
                Ok(())
            })?;
            return Ok(());
        }

        if self.hard {
            as_effective!(Result<()>, {
                for cache in fs::read_dir(CACHE_DIR.as_path())? {
                    let cache = cache?.path();
                    if cache.is_dir() {
                        fs::remove_dir_all(cache)?;
                    } else {
                        fs::remove_file(cache)?;
                    }
                }
                Ok(())
            })?;
            fs::remove_dir_all(RUNTIME_DIR.join("antimony"))?;
        } else if self.profile.is_none() && let Ok(iter) = fs::read_dir(CACHE_DIR.join("run")) {
            for hash in iter.filter_map(Result::ok) {
                let saved: Set<String> = fs::read_dir(hash.path())?
                    .filter_map(Result::ok)
                    .filter_map(|f| f.file_name().into_string().ok())
                    .collect();

                let session: Set<String> = fs::read_dir(RUNTIME_DIR.join("antimony"))?
                    .filter_map(Result::ok)
                    .filter_map(|f| f.file_name().into_string().ok())
                    .collect();

                as_effective!(Result<()>, {
                    for stale in saved.difference(&session) {
                        let cache = CACHE_DIR.join("run").join(stale);
                        remove_directory(cache.as_path())?;
                    }
                    Ok(())
                })?;
            }
        }

        // The cache is in-memory for all refresh operations.
        // When profile-specific, the cache is flushed right after starting the sandbox.
        // When refreshing everything, the cache is flushed after everything has gone.
        store::CACHE.lock().replace(false);

        // If a single profile exist, refresh it and it alone.
        if let Some(profile) = self.profile {
            let mut args = self
                .passthrough
                .map_or_else(run::Args::default, |passthrough| {
                    run::Args::from_vec(&profile, passthrough)
                });

            args.refresh = true;
            args.dry = self.dry;
            args.profile = profile;
            args.run()?;

        // If not dry, repopulate the cache.
        } else if !self.dry {
            let bin = HOME_PATH.join(".local").join("bin");
            let profiles: Vec<String> = fs::read_dir(bin)?
                .filter_map(|file| {
                    if let Ok(file) = file {
                        let file = file.path();
                        if let Ok(dest) = fs::read_link(&file)
                            && dest.ends_with("antimony")
                            && let Some(name) = file.file_name()
                        {
                            let name = name.to_string_lossy();
                            return Some(name.into_owned());
                        }
                    }
                    None
                })
                .collect();
            let terminal = Term::stdout();

            profiles.into_iter().try_for_each(|name| -> Result<()> {
                println!("Refreshing {}", style(&name).bold());
                Self::refresh(&name, None, &terminal)?;
                Ok(())
            })?;

            println!("Flushing to disk");
            mem::flush();
            terminal.clear_last_lines(1)?;
        }
        Ok(())
    }
}

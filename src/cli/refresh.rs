//! Refresh installed profiles.

use crate::{
    cli::{self, run, run_vec},
    shared::{
        Set,
        env::{CACHE_DIR, HOME_PATH, RUNTIME_DIR},
        profile::{self, Profile},
        store::{self, Object, mem},
    },
};
use anyhow::Result;
use log::{debug, info};
use std::fs;
use user::{as_effective, as_real};

#[derive(clap::Args, Default)]
pub struct Args {
    /// Run a profile, but refresh its contents.
    /// If not defined, all profiles are refreshed, but nothing is run.
    pub profile: Option<String>,

    /// Just delete the cache, don't repopulate.
    #[arg(short, long, default_value_t = false)]
    pub dry: bool,

    /// Delete the entire Cache directory. Will break any instance currently running!
    #[arg(long, default_value_t = false)]
    pub hard: bool,

    /// Run arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}
impl cli::Run for Args {
    fn run(self) -> Result<()> {
        if self.hard {
            for cache in fs::read_dir(CACHE_DIR.as_path())? {
                let cache = cache?.path();
                if cache.is_dir() {
                    fs::remove_dir_all(cache)?;
                } else {
                    fs::remove_file(cache)?;
                }
            }
        } else if self.profile.is_none() {
            for hash in fs::read_dir(CACHE_DIR.join("run"))?.filter_map(|f| f.ok()) {
                let saved: Set<String> = fs::read_dir(hash.path())?
                    .filter_map(|f| f.ok())
                    .filter_map(|f| f.file_name().into_string().ok())
                    .collect();

                let session: Set<String> = fs::read_dir(RUNTIME_DIR.join("antimony"))?
                    .filter_map(|f| f.ok())
                    .filter_map(|f| f.file_name().into_string().ok())
                    .collect();

                as_effective!(Result<()>, {
                    for stale in saved.difference(&session) {
                        info!("Removing stale SOF cache {stale}");
                        fs::remove_dir_all(CACHE_DIR.join("run").join(stale))?;
                    }
                    Ok(())
                })??;
            }
        }

        // The cache is in-memory for all refresh operations.
        // When profile-specific, the cache is flushed right after starting the sandbox.
        // When refreshing everything, the cache is flushed after everything has gone.
        store::CACHE.lock().replace(false);

        // If a single profile exist, refresh it and it alone.
        if let Some(profile) = self.profile {
            let mut args = if let Some(passthrough) = self.passthrough {
                run_vec(&profile, passthrough)
            } else {
                run::Args::default()
            };

            args.refresh = true;
            args.dry = self.dry;
            args.profile = profile.clone();
            args.run()?;

        // If not dry, repopulate the cache.
        } else if !self.dry {
            let profiles = installed_profiles()?;
            profiles.into_iter().try_for_each(|name| -> Result<()> {
                println!("Refreshing {name}");

                let profile = store::load::<Profile, profile::Error>(&name, Object::Profile, true)?;

                let args = run::Args {
                    profile: name.clone(),
                    dry: true,
                    refresh: true,
                    ..Default::default()
                };
                args.refresh()?;

                for (conf, _) in profile.configuration {
                    let args = run::Args {
                        profile: name.clone(),
                        dry: true,
                        refresh: true,
                        config: Some(conf),
                        ..Default::default()
                    };
                    args.refresh()?;
                }
                Ok(())
            })?;

            println!("Flushing to disk");
            mem::flush();
        }
        Ok(())
    }
}

pub fn installed_profiles() -> Result<Vec<String>> {
    let profiles: Vec<String> = as_real!(Result<Vec<String>>, {
        let bin = HOME_PATH.join(".local").join("bin");
        debug!("Refreshing local binaries");

        Ok(fs::read_dir(bin)?
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
            .collect())
    })??;
    Ok(profiles)
}

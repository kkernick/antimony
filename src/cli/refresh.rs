//! Refresh installed profiles.

use crate::{
    cli::{self, integrate, run, run_vec},
    shared::env::{AT_HOME, CACHE_DIR, HOME_PATH},
};
use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use log::debug;
use std::{fs, time::Duration};
use user::as_real;

#[derive(clap::Args, Debug, Default)]
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

    /// Integrate all profiles as well.
    #[arg(short, long, default_value_t = false)]
    pub integrate: bool,

    /// Run arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}
impl cli::Run for Args {
    fn run(self) -> Result<()> {
        user::set(user::Mode::Effective)?;

        if self.hard {
            let _ = fs::remove_dir_all(CACHE_DIR.as_path());
        } else {
            let _ = fs::remove_dir_all(CACHE_DIR.join(".proxy"));
            let _ = fs::remove_dir_all(CACHE_DIR.join(".direct"));
            let _ = fs::remove_dir_all(CACHE_DIR.join(".seccomp"));
        }

        let _ = fs::remove_file(AT_HOME.join("db").join("cache.db"));
        let _ = fs::remove_file(AT_HOME.join("db").join("cache.db-wal"));
        let _ = fs::remove_file(AT_HOME.join("db").join("cache.db-shm"));

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

            if self.integrate {
                integrate::integrate(integrate::Args {
                    profile,
                    ..Default::default()
                })?;
            }

        // If not dry, repopulate the cache.
        } else if !self.dry {
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

            // DO NOT TRY AND RUN THIS IN PARALLEL. ANTIMONY WILL
            // CAUSE A KERNEL PANIC IF YOU RUN IT IN PARALLEL!
            let pb = ProgressBar::new(profiles.len() as u64);
            pb.set_style(
                ProgressStyle::default_spinner()
                    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                    .template(" {spinner} {msg} [{wide_bar}] {eta_precise} ")?,
            );
            pb.enable_steady_tick(Duration::from_millis(100));
            pb.wrap_iter(profiles.into_iter())
                .try_for_each(|name| -> Result<()> {
                    pb.set_message(format!("Refreshing {name}"));

                    let args = run::Args {
                        profile: name.clone(),
                        dry: true,
                        refresh: true,
                        ..Default::default()
                    };

                    user::set(user::Mode::Effective)?;
                    args.run()?;

                    if self.integrate {
                        debug!("Integrating {name}");
                        pb.set_message(format!("Integrating {name}"));
                        user::set(user::Mode::Real)?;
                        integrate::integrate(integrate::Args {
                            profile: name,
                            ..Default::default()
                        })?;
                    }
                    Ok(())
                })?;
        }
        Ok(())
    }
}

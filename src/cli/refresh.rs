//! Refresh installed profiles.
use crate::{
    aux::env::{AT_HOME, HOME_PATH},
    cli,
    setup::{self, cleanup, setup},
};
use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use log::debug;
use std::{borrow::Cow, fs, time::Duration};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Run a profile, but refresh its contents.
    /// If not defined, all profiles are refreshed, but nothing is run.
    profile: Option<String>,

    /// Just delete the cache, don't repopulate.
    #[arg(short, long, default_value_t = false)]
    dry: bool,

    /// Delete the entire Cache directory. Will break any instance currently running!
    #[arg(long, default_value_t = false)]
    hard: bool,

    /// Integrate all profiles as well.
    #[arg(short, long, default_value_t = false)]
    integrate: bool,

    /// Use a configuration within the profile.
    #[arg(short, long)]
    pub config: Option<String>,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let cache = AT_HOME.join("cache");

        if self.hard {
            let _ = fs::remove_dir_all(&cache);
        } else {
            // Cached definitions can be removed safely.
            let _ = fs::remove_dir_all(cache.join(".bin"));
            let _ = fs::remove_dir_all(cache.join(".lib"));

            // This seems to be safe. Even if instances are in use,
            // deleting the source does not affect either the proxy,
            // or trying to open direct files.
            let _ = fs::remove_dir_all(cache.join(".proxy"));
            let _ = fs::remove_dir_all(cache.join(".direct"));
            let _ = fs::remove_dir_all(cache.join(".seccomp"));
        }

        // If a single profile exist, refresh it and it alone.
        if let Some(profile) = self.profile {
            let mut args = cli::run::Args {
                dry: self.dry,
                config: self.config,
                refresh: true,
                ..Default::default()
            };
            let info = setup::setup(Cow::Borrowed(&profile), &mut args)?;
            cli::run::run(info, &mut args)?;

            if self.integrate {
                cli::integrate::integrate(cli::integrate::Args {
                    profile,
                    ..Default::default()
                })?;
            }

        // If not dry, repopulate the cache.
        } else if !self.dry {
            user::set(user::Mode::Real)?;
            let bin = HOME_PATH.join(".local").join("bin");
            debug!("Refreshing local binaries");

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
            user::revert()?;

            // DO NOT TRY AND RUN THIS IN PARALLEL. ANTIMONY WILL
            // CAUSE A KERNEL PANIC IF YOU RUN IT IN PARALLEL!
            let pb = ProgressBar::new(profiles.len() as u64);
            pb.set_style(
                ProgressStyle::default_spinner()
                    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                    .template(" {spinner} {msg} [{wide_bar}] {eta_precise} ")?,
            );
            pb.enable_steady_tick(Duration::from_millis(100));

            pb.wrap_iter(profiles.into_iter()).try_for_each(|name| {
                pb.set_message(format!("Refreshing {name}"));
                let mut args = cli::run::Args {
                    dry: true,
                    refresh: true,
                    ..Default::default()
                };
                let info = setup(Cow::Borrowed(&name), &mut args)?;
                cleanup(info.instance)?;

                if self.integrate {
                    user::set(user::Mode::Real)?;
                    pb.set_message(format!("Integrating {name}"));
                    cli::integrate::integrate(cli::integrate::Args {
                        profile: name,
                        ..Default::default()
                    })
                    .and_then(|_| {
                        user::revert().map_err(|_| anyhow!("Failed to revert privilege!"))
                    })
                } else {
                    Ok(())
                }
            })?;
        }
        Ok(())
    }
}

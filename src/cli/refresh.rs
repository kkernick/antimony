//! Refresh installed profiles.
use crate::{
    cli::{self, run_vec},
    setup::{self, cleanup, setup},
    shared::env::{CACHE_DIR, HOME_PATH},
};
use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use log::debug;
use std::{borrow::Cow, fs, time::Duration};
use user::try_run_as;

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
impl super::Run for Args {
    fn run(self) -> Result<()> {
        user::set(user::Mode::Effective)?;

        if self.hard {
            let _ = fs::remove_dir_all(CACHE_DIR.as_path());
        } else {
            // Cached definitions can be removed safely.
            let _ = fs::remove_dir_all(CACHE_DIR.join(".bin"));
            let _ = fs::remove_dir_all(CACHE_DIR.join(".lib"));

            // This seems to be safe. Even if instances are in use,
            // deleting the source does not affect either the proxy,
            // or trying to open direct files.
            let _ = fs::remove_dir_all(CACHE_DIR.join(".proxy"));
            let _ = fs::remove_dir_all(CACHE_DIR.join(".direct"));
            let _ = fs::remove_dir_all(CACHE_DIR.join(".seccomp"));
        }

        // If a single profile exist, refresh it and it alone.
        if let Some(profile) = self.profile {
            let mut args = if let Some(passthrough) = self.passthrough {
                run_vec(&profile, passthrough)
            } else {
                Box::new(cli::run::Args::default())
            };

            args.refresh = true;
            args.dry = self.dry;
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
            let profiles: Vec<String> = try_run_as!(user::Mode::Real, Result<Vec<String>>, {
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
            })?;

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
                    debug!("Integrating {name}");
                    try_run_as!(user::Mode::Real, Result<()>, {
                        pb.set_message(format!("Integrating {name}"));
                        cli::integrate::integrate(cli::integrate::Args {
                            profile: name,
                            ..Default::default()
                        })?;
                        Ok(())
                    })
                } else {
                    Ok(())
                }
            })?;
        }
        Ok(())
    }
}

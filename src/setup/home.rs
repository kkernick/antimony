use crate::shared::{env::OVERLAY, profile::HomePolicy};
use anyhow::{Result, anyhow};
use log::debug;
use std::fs::{self, File};

pub fn setup(args: &mut super::Args) -> Result<Option<String>> {
    if let Some(home) = &args.profile.home {
        let home_dir = home.path(&args.name);

        if home.lock.unwrap_or(false) && !args.args.dry {
            let lock = File::open(&home_dir)?;
            match lock.try_lock() {
                Ok(_) => args.handle.fd_i(lock),
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(anyhow!(
                        "This profile only allows a single instance to run per user, and its home folder is currently locked by another instance."
                    ));
                }
                Err(e) => return Err(anyhow!("Failed to get lock on home folder: {e}")),
            }
        }

        match home.policy.unwrap_or_default() {
            HomePolicy::None => Ok(None),
            policy => {
                let home_str = home_dir.to_string_lossy();
                debug!("Setting up home at {home_dir:?}");
                user::run_as!(user::Mode::Real, fs::create_dir_all(&home_dir))?;

                debug!("Adding args");

                let dest = match &home.path {
                    Some(path) => path,
                    None => "/home/antimony",
                };

                match policy {
                    HomePolicy::Enabled => {
                        args.handle.args_i(["--bind", &home_str, dest])?;
                    }
                    _ => {
                        if *OVERLAY {
                            if policy == HomePolicy::Overlay {
                                #[rustfmt::skip]
                                args.handle.args_i([
                                    "--overlay-src", &home_str,
                                    "--tmp-overlay", dest,
                                ])?;
                            } else {
                                let work = args.sys_dir.join("work");
                                let work_str = work.to_string_lossy();
                                fs::create_dir_all(&work)?;

                                #[rustfmt::skip]
                                args.handle.args_i([
                                    "--overlay-src", &work_str,
                                    "--overlay-src", &home_str,
                                    "--ro-overlay", dest,
                                ])?;
                            }
                        } else {
                            return Err(anyhow!("Bubblewrap version too old for overlays!"));
                        }
                    }
                }

                Ok(Some(home_str.into_owned()))
            }
        }
    } else {
        Ok(None)
    }
}

use crate::shared::profile::HomePolicy;
use anyhow::{Result, anyhow};
use log::debug;
use std::{
    fs::{self, File, TryLockError},
    sync::Arc,
};
use user::as_real;

pub fn setup(args: &Arc<super::Args>) -> Result<Option<String>> {
    if let Some(home) = &args.profile.lock().home.take()
        && let Some(policy) = home.policy
        && policy != HomePolicy::None
    {
        let home_dir = home.path(&args.name);
        debug!("Home directory at {}", home_dir.display());

        // If we explicitly disable the lock, unlock the home.
        if let Some(lock) = home.lock
            && !lock
            && home_dir.exists()
        {
            debug!("Unlocking home");
            as_real!(File::open(&home_dir)?.unlock())??;
        }

        if home.lock.unwrap_or(false) && !args.args.dry && home_dir.exists() {
            let (file, lock) = as_real!(Result<(File, Result<(), TryLockError>)>, {
                let file = File::open(&home_dir)?;
                let lock = file.try_lock();
                Ok((file, lock))
            })??;

            match lock {
                Ok(_) => args.handle.fd_i(file),
                Err(TryLockError::WouldBlock) => {
                    return Err(anyhow!(
                        "This profile only allows a single instance to run per user, and its home folder is currently locked by another instance."
                    ));
                }
                Err(e) => return Err(anyhow!("Failed to get lock on home folder: {e}")),
            }
        }

        let home_str = home_dir.to_string_lossy();
        if !home_dir.exists() {
            as_real!(fs::create_dir_all(&home_dir))??;
        }

        let dest = match &home.path {
            Some(path) => path,
            None => "/home/antimony",
        };

        match policy {
            HomePolicy::Enabled => {
                args.handle.args_i(["--bind", &home_str, dest])?;
            }
            _ => {
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
            }
        }
        Ok(Some(home_str.into_owned()))
    } else {
        Ok(None)
    }
}

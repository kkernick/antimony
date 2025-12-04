use crate::shared::{
    env::{DATA_HOME, OVERLAY},
    profile::HomePolicy,
};
use anyhow::Result;
use log::{debug, error};
use std::{fs, process::exit};

pub fn setup(args: &mut super::Args) -> Result<Option<String>> {
    if let Some(home) = &args.profile.home {
        let home_dir = DATA_HOME.join("antimony").join(match &home.name {
            Some(name) => name,
            None => args.name.as_ref(),
        });

        match home.policy.unwrap_or_default() {
            HomePolicy::None => Ok(None),
            policy => {
                let home_str = home_dir.to_string_lossy();
                user::run_as!(user::Mode::Real, Result<()>, {
                    debug!("Setting up home at {home_dir:?}");
                    fs::create_dir_all(&home_dir)?;

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
                                    user::run_as!(
                                        user::Mode::Effective,
                                        fs::create_dir_all(&work)
                                    )?;

                                    #[rustfmt::skip]
                                args.handle.args_i([
                                    "--overlay-src", &work_str,
                                    "--overlay-src", &home_str,
                                    "--ro-overlay", dest,
                                ])?;
                                }
                            } else {
                                error!("Bubblewrap version too old for overlays!");
                                exit(1);
                            }
                        }
                    }
                    Ok(())
                })?;
                Ok(Some(home_str.into_owned()))
            }
        }
    } else {
        Ok(None)
    }
}

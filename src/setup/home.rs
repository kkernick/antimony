use std::process::exit;

use crate::aux::{
    env::{DATA_HOME, OVERLAY},
    profile::HomePolicy,
};
use anyhow::Result;
use log::{debug, error};

pub fn setup(args: &mut super::Args) -> Result<()> {
    if let Some(home) = &args.profile.home {
        let home_dir = DATA_HOME.join("antimony").join(match &home.name {
            Some(name) => name,
            None => args.name.as_ref(),
        });

        match home.policy.unwrap_or_default() {
            HomePolicy::None => {}
            policy => {
                let saved = user::save()?;
                user::set(user::Mode::Real)?;
                debug!("Setting up home");
                std::fs::create_dir_all(&home_dir)?;
                let home_str = home_dir.to_string_lossy();
                match policy {
                    HomePolicy::Enabled => {
                        args.handle
                            .args_i(["--bind", &home_str, "/home/antimony"])?;
                    }
                    _ => {
                        if *OVERLAY {
                            args.handle.args_i([
                                "--overlay-src",
                                &home_str,
                                "--tmp-overlay",
                                "/home/antimony",
                            ])?;
                        } else {
                            error!("Bubblewrap version too old for overlays!");
                            exit(1);
                        }
                    }
                };
                user::restore(saved)?;
            }
        }
    }
    Ok(())
}

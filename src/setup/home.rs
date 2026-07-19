#![allow(clippy::missing_docs_in_private_items)]

use crate::shared::{
    env::SESSION_BUS,
    profile::home::{HomeLockPolicy, HomePolicy},
    utility,
};
use anyhow::{Result, anyhow};
use heck::ToTitleCase;
use inotify::{Inotify, WatchMask};
use nix::sys::signal::Signal::SIGKILL;
use spawn::{Spawner, StreamMode};
use std::{
    fs::{self, File, TryLockError},
    io::ErrorKind,
};

#[allow(clippy::too_many_lines)]
pub fn setup(args: &mut super::Args) -> Result<Option<String>> {
    if args.profile.lockdown.unwrap_or(false) {
        return Ok(None);
    }

    if let Some(home) = &args.profile.home
        && let Some(mut policy) = home.policy
        && policy != HomePolicy::None
    {
        let home_dir = home.path(&args.name);
        if home.lock.unwrap_or(false)
            && !args.run.dry
            && home_dir.exists()
            && policy != HomePolicy::Overlay
        {
            let file = File::open(&home_dir)?;
            let lock = file.try_lock();
            let mut cont = false;
            let error = Err(anyhow!(
                "This profile only allows a single instance to run per user, and its home folder is currently locked by another instance."
            ));

            match lock {
                Ok(()) => args.handle.fd_i(file),
                Err(TryLockError::WouldBlock) => {
                    {
                        match home.lock_policy.unwrap_or_default() {
                            HomeLockPolicy::Notify => {
                                // Attach a notify watch on the directory to see if the running instance closes it.
                                let mut inotify = Inotify::init()?;
                                let wd = inotify.watches().add(&home_dir, WatchMask::CLOSE)?;
                                let mut buffer = [0; 1024];
                                let title = args.name.to_title_case();
                                let mut prompt = Spawner::abs(utility("notify"))
                            .env("DBUS_SESSION_BUS_ADDRESS", SESSION_BUS.as_str())
                            .mode(user::Mode::Real)
                            .output(StreamMode::Pipe)
                            .args([
                                "--title",
                                &format!("{title} is Locked"),
                                "--body",
                                &format!(
                                    "{title}'s home folder has been locked by another instance. If you can confirm no such \
                                    instance exists, it's possible it was terminated before the lock could be removed. However, \
                                    bypassing the lock when another instance is running may cause issues. You can also skip mounting \
                                    the home folder for this instance, or mount it on an overlay."
                                ),
                                "--timeout",
                                "10000",
                                "--action", "Ignore",
                                "--action", "Unlock",
                                "--action", "Skip",
                                "--action", "Overlay",
                                "--action", "Abort"
                            ])
                            .spawn()?;

                                while prompt.alive()?.is_some() {
                                    match inotify.read_events(&mut buffer) {
                                        Ok(events) => {
                                            for event in events {
                                                if event.wd == wd {
                                                    break;
                                                }
                                            }
                                        }
                                        Err(error) if error.kind() == ErrorKind::WouldBlock => {
                                            continue;
                                        }
                                        _ => return Err(anyhow!("Error while reading events")),
                                    }
                                }

                                inotify.watches().remove(wd)?;

                                if prompt.alive()?.is_none() {
                                    let choice = prompt.output_all()?;
                                    match choice.as_str() {
                                        "Ignore\n" => cont = true,
                                        "Unlock\n" => {
                                            File::open(&home_dir)?.unlock()?;
                                            cont = true;
                                        }
                                        "Skip\n" => return Ok(None),
                                        "Overlay\n" => {
                                            policy = HomePolicy::Overlay;
                                            cont = true;
                                        }
                                        "Abort\n" => return error,
                                        _ => {}
                                    }
                                } else {
                                    prompt.signal(SIGKILL)?;
                                }
                            }
                            HomeLockPolicy::Abort => return error,
                            HomeLockPolicy::Overlay => {
                                policy = HomePolicy::Overlay;
                                cont = true;
                            }
                        }
                    }

                    match file.try_lock() {
                        Ok(()) => args.handle.fd_i(file),
                        Err(_) => {
                            if !cont {
                                return error;
                            }
                        }
                    }
                }
                Err(e) => return Err(anyhow!("Failed to get lock on home folder: {e}")),
            }
        }

        let home_str = home_dir.to_string_lossy();
        if !home_dir.exists() && !args.run.dry {
            fs::create_dir_all(&home_dir)?;
        }

        let dest = home.path.as_ref().map_or("/home/antimony", |path| path);

        match policy {
            HomePolicy::Enabled => {
                args.handle.args_i(["--bind", &home_str, dest]);
            }
            _ => {
                if policy == HomePolicy::Overlay {
                    #[rustfmt::skip]
                                args.handle.args_i([
                                    "--overlay-src", &home_str,
                                    "--tmp-overlay", dest,
                                ]);
                } else {
                    let work = args.sys_dir.join("work");
                    let work_str = work.to_string_lossy();
                    fs::create_dir_all(&work)?;

                    #[rustfmt::skip]
                    args.handle.args_i([
                        "--overlay-src", &work_str,
                        "--overlay-src", &home_str,
                        "--ro-overlay", dest,
                    ]);
                }
            }
        }
        Ok(Some(home_str.into_owned()))
    } else {
        Ok(None)
    }
}

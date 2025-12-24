mod env;
mod fab;
mod files;
mod home;
mod post;
mod proxy;
mod syscalls;
mod wait;

use crate::{
    cli::run::mounted,
    debug_timer,
    shared::{
        Set,
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR},
        path::{user_dir, which_exclude},
        profile::Profile,
    },
};
use ahash::HashSetExt;
use anyhow::{Result, anyhow};
use dbus::{
    Message,
    blocking::{BlockingSender, LocalConnection},
    strings::{BusName, Interface, Member},
};
use inotify::{Inotify, WatchDescriptor};
use log::{debug, info};
use rand::RngCore;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    time::Duration,
};
use user::try_run_as;

struct Args<'a> {
    pub profile: Profile,
    pub name: Cow<'a, str>,
    pub handle: Spawner,
    pub inotify: Inotify,
    pub watches: Set<WatchDescriptor>,
    pub sys_dir: PathBuf,
    pub instance: String,
    pub args: &'a mut super::cli::run::Args,
}

pub struct Info {
    pub name: String,
    pub handle: Spawner,
    pub post: Vec<String>,
    pub profile: Profile,
    pub instance: PathBuf,
    pub home: Option<String>,
    pub sys_dir: PathBuf,
}

pub fn setup<'a>(name: Cow<'a, str>, args: &'a mut super::cli::run::Args) -> Result<Info> {
    let profile = debug_timer!("::profile", {
        let profile = match Profile::new(&name, args.config.take()) {
            Ok(profile) => profile,
            Err(e) => {
                debug!("No profile: {name}: {e}, assuming binary");
                Profile {
                    path: Some(which_exclude(&name)?),
                    ..Default::default()
                }
            }
        };

        let cmd_profile = Profile::from_args(args)?;
        profile.base(cmd_profile)
    })?;

    let hash = profile.hash_str();
    let mut sys_dir = CACHE_DIR.join(&hash);

    // The instance is a unique, random string used in $XDG_RUNTIME_HOME for user facing configuration.
    let instance = debug_timer!("::instance", {
        loop {
            let mut bytes = [0; 8];
            rand::rng().fill_bytes(&mut bytes);
            let instance = bytes
                .iter()
                .map(|byte| format!("{byte:02x?}"))
                .collect::<Vec<String>>()
                .join("");

            if !sys_dir.join("instances").join(&instance).exists() {
                break instance;
            }
        }
    });

    debug_timer!("::cache", {
        let busy = |path: &Path| -> bool {
            match path.read_dir() {
                Ok(mut iter) => iter.next().is_some(),
                Err(_) => false,
            }
        };

        let refresh_dir = CACHE_DIR.join(format!("{hash}-refresh"));

        if refresh_dir.exists() {
            if !busy(&sys_dir.join("instances")) && !busy(&refresh_dir.join("instances")) {
                debug!("Updating to refreshed definitions");
                fs::remove_dir_all(&sys_dir)?;
                fs::rename(&refresh_dir, &sys_dir)?;

                debug!("Removing stale command caches.");
                Spawner::new("find")
                    .args([&sys_dir.to_string_lossy(), "-name", "cmd.cache", "-delete"])?
                    .spawn()?
                    .wait()?;
            } else {
                debug!("Using refresh directory");
                sys_dir = refresh_dir.clone();
            }
        }

        // If we're told to refresh an existing cache
        if args.refresh && sys_dir.exists() {
            // If it's not busy, just remove the directory outright.
            if !busy(&sys_dir.join("instances")) {
                fs::remove_dir_all(&sys_dir)?;
            } else if sys_dir == refresh_dir {
                return Err(anyhow!(
                    "Already refreshed! Please close all active instances to commit changes!"
                ));
            } else {
                info!("Instance is busy. Refreshing in a new location");
                sys_dir = refresh_dir;
            }
        }
    });

    if !sys_dir.exists() {
        fs::create_dir_all(&sys_dir)?;
    }

    let instances = sys_dir.join("instances");
    if !instances.exists() {
        fs::create_dir(&instances)?;
    }

    let runtime = RUNTIME_STR.as_str();

    debug_timer!("::document_wakeup", {
        try_run_as!(user::Mode::Real, Result<()>, {
            if let Some(ipc) = &profile.ipc
                && !ipc.disable.unwrap_or(false)
                && !mounted(&format!("{runtime}/doc"))
            {
                let connection = LocalConnection::new_session()?;
                let msg = Message::new_method_call(
                    BusName::from("org.freedesktop.portal.Documents\0"),
                    dbus::Path::from("/org/freedesktop/portal/documents\0"),
                    Interface::from("org.freedesktop.DBus.Peer\0"),
                    Member::from("Ping\0"),
                );

                if let Ok(msg) = msg {
                    connection.send_with_reply_and_block(msg, Duration::from_secs(1))?;
                } else {
                    return Err(anyhow!("Failed to send ping to Document Portal"));
                }
            }

            // The user dir is at XDG_RUNTIME_DIR, and contains user-facing files.
            fs::create_dir_all(user_dir(&instance).as_path())?;
            Ok(())
        })?;
    });

    symlink(user_dir(&instance), instances.join(&instance))?;

    // Start the command.
    #[rustfmt::skip]
    let handle = Spawner::new("/usr/bin/bwrap")
        .args([
            "--new-session", "--die-with-parent", "--clearenv",
            "--proc", "/proc",
            "--dev", "/dev",
            "--tmpfs", "/tmp",
            "--dir", runtime,
            "--chmod", "0700", runtime,
            "--setenv", "HOME", "/home/antimony",
            "--setenv", "PATH", "/usr/bin",
            "--setenv", "USER", "antimony",
            "--setenv", "DESKTOP_FILE_ID", &profile.id(&name),
            "--setenv", "XDG_RUNTIME_DIR", RUNTIME_STR.as_str(),
        ])?
        .mode(user::Mode::Real);

    debug!("Initializing inotify handle");
    let inotify = try_run_as!(user::Mode::Real, Inotify::init())?;
    let watches = Set::new();

    let mut a = Args {
        profile,
        name: name.clone(),
        handle,
        inotify,
        watches,
        sys_dir: sys_dir.clone(),
        instance,
        args,
    };

    debug_timer!("::proxy", proxy::setup(&mut a))?;
    let home = debug_timer!("::home", home::setup(&mut a))?;

    debug_timer!("::file", files::setup(&mut a))?;
    debug_timer!("::env", env::setup(&mut a));
    debug_timer!("::fab", fab::setup(&mut a))?;
    debug_timer!("::syscalls", syscalls::setup(&mut a))?;

    let post = debug_timer!("::post", post::setup(&mut a))?;
    debug_timer!("::wait", wait::setup(&mut a))?;

    Ok(Info {
        name: name.into_owned(),
        handle: a.handle,
        post,
        profile: a.profile,
        instance: instances.join(a.instance),
        home,
        sys_dir,
    })
}

pub fn cleanup(instance: PathBuf) -> Result<()> {
    debug!("Cleaning up!");

    let user_dir = fs::read_link(&instance)?;
    fs::remove_file(&instance)?;

    try_run_as!(user::Mode::Real, Result<()>, {
        let runtime = RUNTIME_DIR.join(".flatpak").join(&instance);
        if runtime.exists() {
            fs::remove_dir_all(runtime)?;
        }

        if user_dir.exists() {
            debug!("Removing instance at {user_dir:?}");
            fs::remove_dir_all(user_dir)?;
        }

        Ok(())
    })?;

    debug!("Goodbye!");
    Ok(())
}

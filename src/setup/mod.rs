mod env;
mod fab;
mod files;
mod home;
mod post;
mod proxy;
mod syscalls;
mod wait;

use crate::{
    aux::{
        env::{AT_HOME, RUNTIME_DIR, RUNTIME_STR, USER_NAME},
        path::{user_dir, which_exclude},
        profile::Profile,
    },
    cli::run::mounted,
};
use anyhow::{Result, anyhow};
use inotify::{Inotify, WatchDescriptor};
use log::debug;
use rand::RngCore;
use spawn::Spawner;
use std::{borrow::Cow, collections::HashSet, path::PathBuf};
use zbus::blocking;

struct Args<'a> {
    pub profile: Profile,
    pub name: Cow<'a, str>,
    pub handle: Spawner,
    pub inotify: Inotify,
    pub watches: HashSet<WatchDescriptor>,
    pub sys_dir: PathBuf,
    pub instance: String,
    pub args: &'a mut super::cli::run::Args,
}

pub struct Info {
    pub handle: Spawner,
    pub post: Vec<String>,
    pub profile: Profile,
    pub instance: String,
}

pub fn setup<'a>(name: Cow<'a, str>, args: &'a mut super::cli::run::Args) -> Result<Info> {
    let mut profile = match Profile::new(&name) {
        Ok(profile) => profile,
        Err(crate::aux::profile::Error::NotFound(_, reason)) => {
            debug!("No profile: {name}: {reason}, assuming binary");
            Profile {
                path: Some(which_exclude(&name)?),
                ..Default::default()
            }
        }
        Err(e) => return Err(e.into()),
    };

    if let Some(config) = args.config.take() {
        match profile.configuration.take() {
            Some(mut configs) => match configs.remove(&config) {
                Some(conf) => {
                    profile = profile.base(conf)?;
                }
                None => return Err(anyhow!("Specified configuration does not exist!")),
            },
            None => return Err(anyhow!("No configurations defined!")),
        }
    };

    let cmd_profile = Profile::from_args(args);
    profile = profile.base(cmd_profile)?;

    // The instance is a unique, random string used in $XDG_RUNTIME_HOME for user facing configuration.
    let mut bytes = [0; 5];
    rand::rng().fill_bytes(&mut bytes);
    let instance = hex::encode(bytes);

    let runtime = RUNTIME_STR.as_str();

    // Get the system directory, which contains the SOF.
    let sys_dir = AT_HOME.join("cache").join(profile.hash_str());
    std::fs::create_dir_all(&sys_dir)?;

    user::set(user::Mode::Real)?;
    if profile.ipc.is_some() && !mounted(&format!("{runtime}/doc")) {
        let connection = blocking::Connection::session()?;
        let proxy = blocking::Proxy::new(
            &connection,
            "org.freedesktop.portal.Documents",
            "/org/freedesktop/portal/documents",
            "org.freedesktop.DBus.Peer",
        )?;
        proxy.call_method("Ping", &())?;
    }

    // The user dir is at XDG_RUNTIME_DIR, and contains user-facing files.
    std::fs::create_dir_all(user_dir(&instance).as_path())?;
    user::revert()?;

    debug!("Creating system cache");
    let user_cache = sys_dir.join(USER_NAME.as_str());

    debug!("Integrating features");
    let profile = profile.integrate(&name, &user_cache)?;

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
            "--setenv", "DESKTOP_FILE_ID", &profile.id(&name),
            "--setenv", "XDG_RUNTIME_DIR", RUNTIME_STR.as_str(),
        ])?
        .mode(user::Mode::Real);

    debug!("Initializing inotify handle");
    user::set(user::Mode::Real)?;
    let inotify = Inotify::init()?;
    let watches = HashSet::new();
    user::revert()?;

    let mut a = Args {
        profile,
        name,
        handle,
        inotify,
        watches,
        sys_dir,
        instance,
        args,
    };

    proxy::setup(&mut a)?;
    home::setup(&mut a)?;
    files::setup(&mut a)?;
    env::setup(&mut a);
    fab::setup(&mut a)?;

    syscalls::setup(&mut a)?;

    let post = post::setup(&mut a)?;

    wait::setup(&mut a)?;

    Ok(Info {
        handle: a.handle,
        post,
        profile: a.profile,
        instance: a.instance,
    })
}

pub fn cleanup(instance: String) -> Result<()> {
    debug!("Cleaning up!");
    user::set(user::Mode::Real)?;

    let runtime = RUNTIME_DIR.join(".flatpak").join(&instance);
    if runtime.exists() {
        std::fs::remove_dir_all(runtime)?;
    }

    let user_dir = user_dir(&instance);
    if user_dir.exists() {
        std::fs::remove_dir_all(user_dir)?;
    }
    debug!("Goodbye!");
    Ok(())
}

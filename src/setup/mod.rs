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
    shared::{
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR, USER_NAME},
        package::extract,
        path::{user_dir, which_exclude},
        profile::Profile,
    },
};
use anyhow::{Result, anyhow};
use inotify::{Inotify, WatchDescriptor};
use log::{debug, info};
use rand::RngCore;
use spawn::Spawner;
use std::{
    borrow::Cow,
    collections::HashSet,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
use user::try_run_as;
use zbus::blocking;

struct Args<'a> {
    pub profile: Profile,
    pub name: Cow<'a, str>,
    pub handle: Spawner,
    pub inotify: Inotify,
    pub watches: HashSet<WatchDescriptor>,
    pub sys_dir: PathBuf,
    pub instance: String,
    pub package: Option<PathBuf>,
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

pub fn setup<'a>(mut name: Cow<'a, str>, args: &'a mut super::cli::run::Args) -> Result<Info> {
    // The instance is a unique, random string used in $XDG_RUNTIME_HOME for user facing configuration.
    let mut bytes = [0; 5];
    rand::rng().fill_bytes(&mut bytes);
    let instance = hex::encode(bytes);

    let (mut profile, package) = if name.ends_with(".sb") {
        let src = PathBuf::from(name.clone().into_owned());
        let file_name = src.file_stem().unwrap().to_string_lossy();

        let package_path = CACHE_DIR.join(format!("package-{file_name}"));

        if !package_path.exists() {
            debug!("Extracting package");
            extract(&src, &package_path)?;
        }

        let file_name = src.file_stem().unwrap().to_string_lossy();
        name = Cow::Owned(file_name.into_owned());

        let profile = package_path.join("profile.toml");
        let profile = Profile::new(&profile.to_string_lossy())?;
        (profile, Some(package_path))
    } else {
        let profile = match Profile::new(&name) {
            Ok(profile) => profile,
            Err(e) => {
                debug!("No profile: {name}: {e}, assuming binary");
                Profile {
                    path: Some(which_exclude(&name)?),
                    ..Default::default()
                }
            }
        };
        (profile, None)
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

    let runtime = RUNTIME_STR.as_str();
    let busy = |path: &Path| -> bool {
        match path.read_dir() {
            Ok(mut iter) => iter.next().is_some(),
            Err(_) => false,
        }
    };

    // Get the system directory, which contains the SOF.
    let hash = profile.hash_str();

    let mut sys_dir = CACHE_DIR.join(&hash);
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

    fs::create_dir_all(&sys_dir)?;
    let instances = sys_dir.join("instances");

    fs::create_dir_all(&instances)?;

    try_run_as!(user::Mode::Real, Result<()>, {
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
        fs::create_dir_all(user_dir(&instance).as_path())?;
        Ok(())
    })?;

    symlink(user_dir(&instance), instances.join(&instance))?;

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
            "--setenv", "USER", "antimony",
            "--setenv", "DESKTOP_FILE_ID", &profile.id(&name),
            "--setenv", "XDG_RUNTIME_DIR", RUNTIME_STR.as_str(),
        ])?
        .mode(user::Mode::Real, true);

    debug!("Initializing inotify handle");
    let inotify = try_run_as!(user::Mode::Real, Inotify::init())?;
    let watches = HashSet::new();

    let mut a = Args {
        profile,
        name: name.clone(),
        handle,
        inotify,
        watches,
        sys_dir: sys_dir.clone(),
        instance,
        package,
        args,
    };

    proxy::setup(&mut a)?;
    let home = home::setup(&mut a)?;
    files::setup(&mut a)?;
    env::setup(&mut a);
    fab::setup(&mut a)?;
    syscalls::setup(&mut a)?;
    let post = post::setup(&mut a)?;
    wait::setup(&mut a)?;

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

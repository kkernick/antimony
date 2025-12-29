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
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR},
        profile::Profile,
    },
    timer,
};
use anyhow::{Result, anyhow};
use dashmap::DashSet;
use dbus::{
    Message,
    blocking::{BlockingSender, LocalConnection},
    strings::{BusName, Interface, Member},
};
use inotify::{Inotify, WatchDescriptor};
use log::{debug, info};
use parking_lot::Mutex;
use spawn::{Handle, Spawner};
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use temp::Temp;
use user::try_run_as;

struct Args<'a> {
    pub profile: Mutex<Profile>,
    pub id: String,
    pub name: Cow<'a, str>,
    pub handle: Spawner,
    pub inotify: Mutex<Inotify>,
    pub watches: DashSet<WatchDescriptor>,
    pub sys_dir: PathBuf,
    pub instance: &'a Temp,
    pub args: &'a mut super::cli::run::Args,
}

pub struct Info {
    pub name: String,
    pub handle: Spawner,
    pub post: Vec<String>,
    pub profile: Profile,
    pub instance: Temp,
    pub home: Option<String>,
    pub sys_dir: PathBuf,
}

pub fn setup<'a>(name: Cow<'a, str>, args: &'a mut super::cli::run::Args) -> Result<Info> {
    let profile = timer!("::profile", {
        let profile = match Profile::new(&name, args.config.take()) {
            Ok(profile) => profile,
            Err(e) => {
                debug!("No profile: {name}: {e}, assuming binary");
                Profile {
                    path: Some(which::which(name.clone())?.to_string()),
                    ..Default::default()
                }
            }
        };

        let cmd_profile = Profile::from_args(args)?;
        profile.base(cmd_profile)
    })?;

    let hash = profile.hash_str();
    debug!("Profile Hash: {hash}");
    let mut sys_dir = CACHE_DIR.join(&hash);

    // The instance is a unique, random string used in $XDG_RUNTIME_HOME for user facing configuration.
    let mut instance = temp::Builder::new()
        .within(PathBuf::from(RUNTIME_DIR.as_path()).join("antimony"))
        .owner(user::Mode::Real)
        .create::<temp::Directory>()?;

    timer!("::cache", {
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
                Spawner::abs("/usr/bin/find")
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

    timer!("::document_wakeup", {
        if let Some(ipc) = &profile.ipc
            && !ipc.disable.unwrap_or(false)
        {
            try_run_as!(user::Mode::Real, Result<()>, {
                if !mounted(&format!("{runtime}/doc")) {
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
                Ok(())
            })?;

            instance.associate(
                temp::Builder::new()
                    .within(RUNTIME_DIR.join(".flatpak"))
                    .name(instance.name())
                    .owner(user::Mode::Real)
                    .create::<temp::Directory>()?,
            );
        }
    });

    instance.link(instances.join(instance.name()), user::Mode::Effective)?;

    // Start the command.
    #[rustfmt::skip]
    let handle = Spawner::abs("/usr/bin/bwrap")
        .name(&args.profile)
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
    let inotify = Mutex::new(try_run_as!(user::Mode::Real, Inotify::init())?);
    let watches = DashSet::new();
    let id = profile.id(&name);

    let a = Arc::new(Args {
        profile: Mutex::new(profile),
        id,
        name: name.clone(),
        handle,
        inotify,
        watches,
        sys_dir: sys_dir.clone(),
        instance: &instance,
        args,
    });

    let (proxy, pair) = timer!(
        "::setup_total",
        rayon::join(
            || timer!("::proxy", proxy::setup(Arc::clone(&a))),
            || -> Result<(Option<String>, Option<Handle>)> {
                timer!("::setup_rest", {
                    let a = Arc::clone(&a);
                    let home = timer!("::home", home::setup(&a))?;
                    timer!("::file", files::setup(&a))?;
                    timer!("::env", env::setup(&a));
                    timer!("::fab", fab::setup(&a))?;
                    let monitor = timer!("::syscalls", syscalls::setup(&a))?;
                    Ok((home, monitor))
                })
            },
        )
    );

    let (home, monitor) = pair?;
    let mut a = Arc::into_inner(a).expect("Failed to unwrap fabricator");

    if let Some((proxy, arguments)) = proxy? {
        a.handle.associate(proxy);
        a.handle.args_i(arguments)?;
    }
    if let Some(monitor) = monitor {
        a.handle.associate(monitor);
    }

    let post = timer!("::post", post::setup(&mut a))?;

    timer!(
        "::wait",
        wait::setup(a.watches, a.inotify.into_inner(), &mut a.handle, a.args.dry)
    )?;

    Ok(Info {
        name: name.into_owned(),
        handle: a.handle,
        post,
        profile: a.profile.into_inner(),
        instance,
        home,
        sys_dir,
    })
}

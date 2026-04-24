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
    fab::lib::ROOTS,
    shared::{
        Set,
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR},
        profile::Profile,
        store::mem,
        utility,
    },
    timer,
};
use anyhow::{Result, anyhow};
use dbus::{
    Message,
    blocking::{BlockingSender, LocalConnection},
    strings::{BusName, Interface, Member},
};
use inotify::{Inotify, WatchDescriptor};
use log::{debug, info, warn};
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use temp::Temp;
use user::as_real;

/// The information passed to the various setup functions.
struct Args<'a> {
    pub profile: Profile,
    pub id: String,
    pub name: Cow<'a, str>,
    pub handle: Spawner,
    pub inotify: Inotify,
    pub watches: Set<WatchDescriptor>,
    pub sys_dir: PathBuf,
    pub instance: &'a Temp,
    pub args: &'a mut super::cli::run::Args,
}

/// The information passed back to `run`.
pub struct Info {
    pub name: String,
    pub handle: Spawner,
    pub post: Vec<String>,
    pub profile: Profile,
    pub instance: Temp,
    pub home: Option<String>,
    pub sys_dir: PathBuf,
}

/// The main function within antimony. It takes a name, and spits out a sandbox ready to run.
pub fn setup<'a>(
    name: Cow<'a, str>,
    mut args: &'a mut super::cli::run::Args,
    flush_defer: bool,
) -> Result<Info> {
    let (mut profile, hash) = timer!(
        "::profile_load",
        Profile::new(&name, args.config.take(), Some(&mut args), false)
    )?;

    let mut sys_dir = CACHE_DIR.join("run").join(&hash);
    let mut instances = RUNTIME_DIR.join("antimony").join(&hash);
    if let Some(libraries) = &mut profile.libraries {
        libraries.roots.drain().for_each(|root| {
            if Path::new(&root).exists() {
                let _ = ROOTS.insert(root);
            }
        });
    }

    // Refreshing logic tries to avoid deleting an SOF from underneath a running instance.
    // If another instance is running this profile, we defer to a refresh folder, which new
    // instances will be redirected to, and which will replace the original once all instances
    // have been closed. If you run into the situation where you have two instances, each running
    // on a different SOF, and try and refresh *again*, Antimony will throw an error.
    timer!("::cache", {
        let busy = |path: &Path| -> bool {
            match path.read_dir() {
                Ok(mut iter) => iter.next().is_some(),
                Err(e) => {
                    warn!("Error reading instance directory {}: {e}", path.display());
                    false
                }
            }
        };

        let refresh_dir = RUNTIME_DIR.join("antimony").join(format!("{hash}r"));
        let refresh_sof = CACHE_DIR.join("run").join(format!("{hash}r"));

        if refresh_sof.exists() {
            if as_real!(!busy(&instances) && !busy(&refresh_dir))? {
                debug!("Updating to refreshed definitions");

                if refresh_dir.exists() {
                    as_real!(fs::remove_dir_all(&refresh_dir))??;
                }
                if sys_dir.exists() {
                    fs::remove_dir_all(&sys_dir)?;
                }

                fs::rename(&refresh_sof, &sys_dir)?;

                debug!("Removing stale command caches.");
                Spawner::abs("/usr/bin/find")
                    .args([&sys_dir.to_string_lossy(), "-name", "cmd.cache", "-delete"])
                    .spawn()?
                    .wait()?;
            } else {
                debug!("Using refresh directory");
                sys_dir = refresh_sof.clone();
                instances = refresh_dir.clone();
            }
        }

        // If we're told to refresh an existing cache
        if args.refresh && sys_dir.exists() {
            // If it's not busy, just remove the directory outright.
            if !as_real!(busy(&instances))? {
                info!(
                    "No running instances in {}. Safe to refresh.",
                    instances.display()
                );
                fs::remove_dir_all(&sys_dir)?;
            } else if sys_dir == refresh_sof {
                return Err(anyhow!(
                    "Already refreshed! Please close all active instances to commit changes!"
                ));
            } else {
                info!("Instance is busy. Refreshing in a new location");
                sys_dir = refresh_sof;
                instances = refresh_dir;
            }
        }
    });

    // The instance is a unique, random string used in $XDG_RUNTIME_HOME for user facing configuration.
    let mut instance = timer!(
        "::instance_dir",
        temp::Builder::new()
            .within(instances)
            .owner(user::Mode::Real)
            .create::<temp::Directory>()
    )?;

    if !sys_dir.exists() {
        fs::create_dir_all(&sys_dir)?;
    }
    let runtime = RUNTIME_STR.as_str();

    // The Document Portal doesn't run until something pings it. If this is the
    // first sandbox running, that introduces a significant lag on the wait() call,
    // so we ping it immediately.
    timer!("::document_wakeup", {
        if let Some(ipc) = &profile.ipc
            && !ipc.disable.unwrap_or(false)
        {
            if !mounted(&format!("{runtime}/doc")) {
                as_real!(Result<()>, {
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

                    Ok(())
                })??;
            }

            // Associate the flatpak dir with our instance so they're deleted together.
            instance.associate(
                temp::Builder::new()
                    .within(RUNTIME_DIR.join(".flatpak"))
                    .name(instance.name())
                    .owner(user::Mode::Real)
                    .create::<temp::Directory>()?,
            );
        }
    });

    // Start the command.
    let handle = timer!("::spawn_handle", {
        #[rustfmt::skip]
        let handle = Spawner::abs(
            if profile.lockdown.unwrap_or(false) {
                utility("lockdown")
            } else {
                "/usr/bin/bwrap".to_string()
            }
        )
        .name(&args.profile)
        .args([
            "--new-session", "--die-with-parent", "--clearenv",
            "--proc", "/proc",
            "--dev", "/dev",
            "--tmpfs", "/tmp",
            "--dir", runtime,
            "--chmod", "0700", runtime,
            "--setenv", "HOME", "/home/antimony",
            "--dir", "/home/antimony",
            "--setenv", "PATH", "/usr/bin",
            "--setenv", "USER", "antimony",
            "--setenv", "DESKTOP_FILE_ID", &profile.id(&name),
            "--setenv", "XDG_RUNTIME_DIR", RUNTIME_STR.as_str(),
        ])
        .mode(user::Mode::Real);

        if profile.new_privileges.unwrap_or(false) {
            handle.new_privileges_i(true);
        }

        if let Some(dir) = &profile.dir {
            handle.args_i(["--chdir", dir]);
        }
        handle
    });

    debug!("Initializing inotify handle");
    let inotify = timer!("::inotify", as_real!(Inotify::init()))??;
    let watches = Set::default();
    let id = profile.id(&name);

    let mut a = Args {
        profile,
        id,
        name: name.clone(),
        handle,
        inotify,
        watches,
        sys_dir: sys_dir.clone(),
        instance: &instance,
        args,
    };

    timer!("::proxy", proxy::setup(&mut a))?;
    let home = timer!("::home", home::setup(&mut a))?;
    timer!("::env", env::setup(&a));
    timer!("::fab", fab::setup(&mut a))?;
    timer!("::file", files::setup(&mut a))?;
    timer!("::syscalls", syscalls::setup(&a))?;

    // If we're dry-running, and are running under a single profile, flush as
    // soon as possible--as then we don't waste time waiting for the writing
    // to finish. We can't rely on the user interacting with the application
    // to conceal the flush, so we have to do it early.
    if !flush_defer && a.args.dry {
        timer!("::flush", mem::flush());
    }

    let post = timer!("::post", post::setup(&mut a))?;

    // Unfortunately, the proxy is slower than Antimony, so we need to wait for it
    // to be ready. I should probably just write my own.
    timer!(
        "::wait",
        wait::setup(a.watches, a.inotify, &mut a.handle, a.args.dry)
    )?;

    Ok(Info {
        name: name.into_owned(),
        handle: a.handle,
        post,
        profile: a.profile,
        instance,
        home,
        sys_dir,
    })
}

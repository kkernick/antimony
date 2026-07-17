#![allow(clippy::missing_docs_in_private_items, clippy::missing_errors_doc)]

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
    fab::{find_folders, lib::ROOTS},
    shared::{
        Set,
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR},
        find::{DirType, recursive_crawl},
        package::{Package, get_profile},
        profile::{Profile, seccomp::SeccompPolicy},
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
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use temp::Temp;
use user::as_effective;

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

    /// If the boolean is false, we are *creating* the package. If true, we are *using* the package.
    pub package: Option<(Package, bool)>,
    pub run: &'a mut super::cli::run::Args,
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
    pub package: Option<(Package, bool)>,
}

/// The main function within antimony. It takes a name, and spits out a sandbox ready to run.
#[allow(clippy::too_many_lines)]
pub fn setup<'a>(
    mut name: Cow<'a, str>,
    mut args: &'a mut super::cli::run::Args,
    flush_defer: bool,
    mut package: Option<(Package, bool)>,
) -> Result<Info> {
    let (mut profile, hash, profile_args) = if let Some((_, true)) = package {
        let path = PathBuf::from(name.into_owned());
        let (mut profile, profile_path) = get_profile(&path)?;
        name = Cow::Owned(profile_path.file_stem().map_or_else(
            || profile_path.to_string_lossy().into_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        ));

        let hash = profile.hash_str(&None);

        // These do not work without a system installation.
        profile.seccomp = Some(SeccompPolicy::Disabled);
        profile.lockdown = Some(false);

        let mut profile_args = Vec::new();
        let root = path.join("root");
        if root.exists() {
            #[rustfmt::skip]
            profile_args.extend([
                "--overlay-src", &root.to_string_lossy(),
                "--tmp-overlay", "/",
            ].map(String::from));
        }

        let bin = path.join("bin");
        let bin_str = bin.to_string_lossy();

        let lib = path.join("lib");
        let lib_str = lib.to_string_lossy();

        // The package uses a single library root, and has no idea what the
        // host system's layout is. We therefore can't try to mount the host
        // libraries in the same way we can do binaries.
        //
        // Instead, the outer namespace simply overlays a bunch of hard-coded
        // roots to /usr/lib, which we then overlay below the package libraries.
        //
        // Antimony will itself enable system libraries if it's running as a package (
        // So you can run external applications. It still performs SOF, but needs you
        // system libraries to do it).

        let root_libs = root.join("usr").join("lib");
        let mut lib_roots = vec![lib_str];
        let no_sof = profile
            .libraries
            .as_ref()
            .map_or_else(|| false, |libraries| libraries.no_sof.unwrap_or_default());

        if root_libs.exists() {
            let root_str = root_libs.to_string_lossy();
            lib_roots.push(root_str);
        }

        if no_sof {
            lib_roots.insert(0, Cow::Borrowed("/usr/lib"));
        }

        if lib_roots.len() == 1
            && let Some(root) = lib_roots.first()
        {
            profile_args.extend(["--ro-bind", root, "/usr/lib"].map(String::from));
        } else {
            for lib in lib_roots {
                profile_args.extend(["--overlay-src", &lib].map(String::from));
            }
            profile_args.extend(["--ro-overlay", "/usr/lib"].map(String::from));
        }

        #[rustfmt::skip]
        profile_args.extend([
            "--bind", if profile.binaries.contains("/usr/bin") {"/usr/bin"} else {&bin_str}, "/usr/bin",
            "--symlink", "/usr/bin", "/bin",
            "--symlink", "/usr/bin", "/sbin",
            "--symlink", "/usr/bin", "/usr/sbin",
            "--symlink", "/usr/lib", "/lib",
            "--symlink", "/usr/lib", "/usr/lib64",
            "--symlink", "/usr/lib64", "/lib64"
        ].map(String::from));

        let links = path.join("links");
        if links.exists() {
            links.read_dir()?.filter_map(Result::ok).for_each(|dest| {
                if let Ok(src) = dest.path().read_link() {
                    profile_args.extend(
                        [
                            "--symlink",
                            &src.to_string_lossy(),
                            &dest.file_name().to_string_lossy().replace('-', "/"),
                        ]
                        .map(String::from),
                    );
                }
            });
        }

        if no_sof {
            for path in find_folders(&name) {
                profile_args.extend(["--ro-bind", &path, &path].map(String::from));
            }
        }

        profile = profile.base(Profile::from_args(args)?)?;
        package = Some((Package::default(), true));
        (profile, hash, profile_args)
    } else {
        let (profile, hash) = Profile::new(&name, args.config.take(), Some(&mut args), false)?;
        (profile, hash, Vec::new())
    };

    let mut sys_dir = CACHE_DIR.join("run").join(&hash);
    let mut instances = RUNTIME_DIR.join("antimony").join(&hash);
    if let Some(libraries) = &mut profile.libraries {
        libraries.roots.drain().for_each(|root| {
            if Path::new(&root).exists() {
                let _ = ROOTS.insert(Cow::Owned(root));
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
            if !busy(&instances) && !busy(&refresh_dir) {
                debug!("Updating to refreshed definitions");

                as_effective!(Result<()>, {
                    if refresh_dir.exists() {
                        fs::remove_dir_all(&refresh_dir)?;
                    }
                    if sys_dir.exists() {
                        fs::remove_dir_all(&sys_dir)?;
                    }

                    fs::rename(&refresh_sof, &sys_dir)?;
                    Ok(())
                })??;

                debug!("Removing stale command caches.");
                let mut crawled = recursive_crawl(&sys_dir.to_string_lossy(), Some(2))?;
                if let Some(files) = crawled.remove(&DirType::File) {
                    as_effective!(
                        files
                            .into_par_iter()
                            .filter(|path| path.ends_with("cmd.cache"))
                            .try_for_each(fs::remove_file)
                    )??;
                }
            } else {
                debug!("Using refresh directory");
                #[allow(clippy::assigning_clones)]
                {
                    sys_dir = refresh_sof.clone();
                    instances = refresh_dir.clone();
                }
            }
        }

        // If we're told to refresh an existing cache
        if args.refresh && sys_dir.exists() {
            // If it's not busy, just remove the directory outright.
            if !busy(&instances) {
                info!(
                    "No running instances in {}. Safe to refresh.",
                    instances.display()
                );
                as_effective!(fs::remove_dir_all(&sys_dir))??;
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
        as_effective!(fs::create_dir_all(&sys_dir))??;
    }
    let runtime = RUNTIME_STR.as_str();

    // The Document Portal doesn't run until something pings it. If this is the
    // first sandbox running, that introduces a significant lag on the wait() call,
    // so we ping it immediately.
    timer!("::document_wakeup", {
        if !args.dry
            && let Some(ipc) = &profile.ipc
            && !ipc.disable.unwrap_or(false)
        {
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
                "/usr/bin/bwrap".to_owned()
            }
        )
        .name(&args.profile)
        .mode(user::Mode::Real)
        .args(profile_args)
        .args([
            "--new-session", "--die-with-parent",
            "--proc", "/proc",
            "--dev", "/dev",
            "--tmpfs", "/tmp",
            "--dir", runtime,
            "--chmod", "0700", runtime,
            "--setenv", "PATH", "/usr/bin",
        ]);

        if profile.preserve_env.unwrap_or(false) {
            handle.preserve_env_i(true);
        } else {
            #[rustfmt::skip]
            handle.args_i([
                "--clearenv",
                "--dir", "/home/antimony",
                "--setenv", "USER", "antimony",
                "--setenv", "HOME", "/home/antimony",
                "--setenv", "DESKTOP_FILE_ID", &profile.id(&name),
                "--setenv", "XDG_RUNTIME_DIR", RUNTIME_STR.as_str(),
            ]);
        }

        if let Some(dir) = &profile.dir {
            handle.args_i(["--chdir", dir]);
        }
        handle
    });

    debug!("Initializing inotify handle");
    let inotify = timer!("::inotify", Inotify::init())?;
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
        run: args,
        package,
    };

    timer!("::proxy", proxy::setup(&mut a))?;
    let home = timer!("::home", home::setup(&mut a))?;
    timer!("::env", env::setup(&mut a))?;
    timer!("::fab", fab::setup(&mut a))?;
    timer!("::file", files::setup(&mut a))?;

    if a.package.is_none() {
        timer!("::syscalls", syscalls::setup(&a))?;
    }

    // If we're dry-running, and are running under a single profile, flush as
    // soon as possible--as then we don't waste time waiting for the writing
    // to finish. We can't rely on the user interacting with the application
    // to conceal the flush, so we have to do it early.
    if !flush_defer && a.run.dry && a.package.is_none() {
        timer!("::flush", mem::flush());
    }

    let post = timer!("::post", post::setup(&mut a))?;
    timer!(
        "::wait",
        wait::setup(a.watches, a.inotify, &a.handle, a.run.dry)
    )?;

    Ok(Info {
        name: name.into_owned(),
        handle: a.handle,
        post,
        profile: a.profile,
        package: a.package,
        instance,
        home,
        sys_dir,
    })
}

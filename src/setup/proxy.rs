use crate::{
    fab::lib::{add_sof, get_libraries},
    shared::{
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR},
        path::user_dir,
        profile::{Namespace, Portal, Profile},
        syscalls,
    },
};
use anyhow::Result;
use inotify::WatchMask;
use log::debug;
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    env,
    fs::{self, File},
    io::Write,
    path::Path,
};
use user::{self, try_run_as};
use which::which;

pub fn run(
    sys_dir: &Path,
    profile: &mut Profile,
    instance: &str,
    info: &Path,
    id: &str,
) -> Result<Spawner> {
    let runtime = RUNTIME_DIR.to_string_lossy();
    let resolve = which("xdg-dbus-proxy")?.to_string_lossy().into_owned();
    let sof = CACHE_DIR.join(".proxy");

    debug!("Creating proxy directory");
    let proxy = user_dir(instance).join("proxy");
    try_run_as!(user::Mode::Real, fs::create_dir_all(&proxy))?;

    debug!("Creating SOF");

    // Create an SOF for the proxy.
    // It's shared between every application and instance.
    // Performed before we drop to the user.
    if !sof.exists() {
        let libraries = get_libraries(Cow::Borrowed(&resolve))?;

        libraries
            .into_par_iter()
            .try_for_each(|library| add_sof(&sof, Cow::Owned(library)))?;
    }

    #[rustfmt::skip]
    let mut proxy = Spawner::new("/usr/bin/bwrap")
        .mode(user::Mode::Real, true).args([
            "--new-session",
            "--ro-bind", &resolve, &resolve,
            "--clearenv",
            "--disable-userns",
            "--assert-userns-disabled",
            "--unshare-all",
            "--unshare-user",
            "--die-with-parent",
            "--dir", &runtime,
            "--bind", &format!("{runtime}/bus"), &format!("{runtime}/bus"),
            "--ro-bind", &info.to_string_lossy(), "/.flatpak-info",
            "--symlink", "/.flatpak-info", &format!("{runtime}/flatpak-info"),
            "--bind", &proxy.to_string_lossy(), &format!("{runtime}/app/{id}"),
        ])?;

    let app_dir = RUNTIME_DIR.join("app").join(id);
    try_run_as!(user::Mode::Real, fs::create_dir_all(&app_dir))?;

    let sof_str = sof.to_string_lossy();
    proxy.args_i(["--ro-bind-try", &format!("{sof_str}/lib"), "/usr/lib"])?;

    let path = &format!("{sof_str}/lib64");
    if Path::new(path).exists() {
        proxy.args_i(["--ro-bind-try", path, "/usr/lib64"])?;
    } else {
        proxy.args_i(["--symlink", "/usr/lib", "/usr/lib64"])?;
    }

    proxy.args_i([
        "--symlink",
        "/usr/lib",
        "/lib",
        "--symlink",
        "/usr/lib64",
        "/lib64",
    ])?;

    // Setup SECCOMP.
    if let Some(policy) = profile.seccomp {
        let (filter, fd) = syscalls::new("xdg-dbus-proxy", instance, policy, &None)?;
        proxy.seccomp_i(filter);
        if let Some(fd) = fd {
            proxy.fd_arg_i("--seccomp", fd)?;
        }
    }

    proxy.args_i([
        "--",
        &resolve,
        &env::var("DBUS_SESSION_BUS_ADDRESS")?,
        &app_dir.join("bus").to_string_lossy(),
        "--filter",
    ])?;

    if let Ok(log) = env::var("RUST_LOG")
        && log == "debug"
    {
        proxy.arg_i("--log")?;
    }

    let cache = sys_dir.join("proxy.cache");
    if cache.exists() {
        proxy.cache_read(&cache)?;
    } else {
        proxy.cache_start()?;

        let permit_call = |portal: &str| -> String {
            let path = portal.replace(".", "/").to_ascii_lowercase();
            format!("--call={portal}=org.freedesktop.DBus.Properties.*@{path}")
        };

        if let Some(ipc) = profile.ipc.take() {
            if !ipc.portals.is_empty() {
                let desktop = "org.freedesktop.portal.Desktop";
                let path = "/org/freedesktop/portal/desktop";
                proxy.args_i([
                    format!("--call={desktop}=org.freedesktop.DBus.Properties.*@{path}/*"),
                    format!(
                        "--call={desktop}=org.freedesktop.DBus.Introspectable.Introspect@{path}"
                    ),
                ])?;

                for portal in &ipc.portals {
                    if portal == &Portal::Settings {
                        proxy.arg_i(format!("--broadcast={desktop}=org.freedesktop.portal.Settings.SettingChanged@{path}"))?;
                    }
                    proxy.args_i([
                        format!("--call={desktop}=org.freedesktop.portal.{portal:?}.*@{path}"),
                        format!("--talk=org.freedesktop.portal.{portal:?}"),
                        format!("--call={desktop}=org.freedesktop.{portal:?}.*@{path}"),
                        format!("--talk=org.freedesktop.{portal:?}"),
                    ])?;
                }
            }
            for portal in &ipc.see {
                proxy.args_i([format!("--see={portal}"), permit_call(portal)])?;
            }
            for portal in &ipc.talk {
                proxy.args_i([format!("--talk={portal}"), permit_call(portal)])?;
            }
            for portal in &ipc.own {
                proxy.args_i([format!("--own={portal}"), permit_call(portal)])?;
            }
            for portal in &ipc.call {
                proxy.arg_i(format!("--call={portal}"))?;
            }
        }
        proxy.cache_write(&cache)?;
    }
    Ok(proxy)
}

pub fn setup(args: &mut super::Args) -> Result<()> {
    debug!("Setting up proxy");
    let runtime = RUNTIME_STR.as_str();

    // Run the proxy
    if let Some(ipc) = &args.profile.ipc {
        // Add the system bus.
        let system_bus = ipc.system_bus.unwrap_or(false);
        if system_bus {
            args.handle.args_i([
                "--ro-bind",
                "/var/run/dbus/system_bus_socket",
                "/var/run/dbus/system_bus_socket",
            ])?;
        }

        let instance = &args.instance;
        let id = args.profile.id(&args.name);
        let user_dir_str = user_dir(&args.instance).to_string_lossy().into_owned();
        let info = user_dir(instance).join(".flatpak-info");

        // Create the flatpak-info
        try_run_as!(user::Mode::Real, Result<()>, {
            debug!("Creating flatpak info");
            let out = fs::File::create_new(&info)?;

            // https://docs.flatpak.org/en/latest/flatpak-command-reference.html
            #[rustfmt::skip]
            let mut info_contents: Vec<String> = vec![
                "[Application]",
                &format!("name={id}"),
                "[Instance]",
                &format!("instance-id={instance}"),
                "app-path=/usr",
                "[Context]",
                "sockets=session-bus;system-bus;",
            ].into_iter().map(|e| e.to_string()).collect();
            if let Some(ns) = &args.profile.namespaces
                && ns.contains(&Namespace::Net)
            {
                info_contents.push("shared=network;".to_string());
            }
            write!(&out, "{}", info_contents.join("\n"))?;

            // Add the required files.
            #[rustfmt::skip]
            args.handle.args_i([
                "--bind", &format!("{runtime}/doc"), &format!("{runtime}/doc"),
                "--ro-bind", "/run/dbus", "/run/dbus",
                "--setenv", "DBUS_SESSION_BUS_ADDRESS", &format!("unix:path=/run/user/{}/bus", user::USER.real),
                "--ro-bind", &format!("{user_dir_str}/.flatpak-info"), "/.flatpak-info",
                "--symlink", "/.flatpak-info", &format!("{runtime}/flatpak-info"),
            ])?;
            Ok(())
        })?;

        try_run_as!(user::Mode::Real, Result<()>, {
            debug!("Creating flatpak directory");
            let flatpak_dir = RUNTIME_DIR.join(".flatpak").join(instance);
            fs::create_dir_all(&flatpak_dir)?;
            args.handle.fd_arg_i(
                "--json-status-fd",
                File::create(flatpak_dir.join("bwrapinfo.json"))?,
            )?;
            Ok(())
        })?;

        debug!("Setting up user bus");
        let user_bus = ipc.user_bus.unwrap_or(false);
        // Either mount the bus directly
        if user_bus {
            args.handle.args_i([
                "--ro-bind",
                &format!("{}/bus", RUNTIME_STR.as_str()),
                &format!("{}/bus", RUNTIME_STR.as_str()),
            ])?;

        // Or mediate via the proxy.
        } else if !ipc.disable.unwrap_or(false) {
            let proxy = run(&args.sys_dir, &mut args.profile, &args.instance, &info, &id)?;
            args.handle.args_i([
                "--ro-bind",
                &format!("{user_dir_str}/proxy/bus"),
                &format!("{runtime}/bus"),
            ])?;

            if !args.args.dry {
                try_run_as!(user::Mode::Real, Result<()>, {
                    debug!("Creating proxy watch");
                    args.watches.insert(
                        args.inotify
                            .watches()
                            .add(user_dir(&args.instance).join("proxy"), WatchMask::CREATE)?,
                    );
                    Ok(())
                })?;
                args.handle.associate(proxy.spawn()?);
            }
        }
    }
    Ok(())
}
